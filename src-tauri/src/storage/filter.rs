use super::{i64_from_usize, sql_error};
use crate::error::AppError;
use crate::model::{AnalysisSettings, FrequencyMode, ImportedRecordsQuery, PersonQuery};
use rusqlite::{params_from_iter, types::Value, Connection};
use std::collections::{HashMap, HashSet};
pub(crate) fn build_person_filter(session_id: &str, query: &PersonQuery) -> (String, Vec<Value>) {
    let mut clauses = vec!["p.session_id = ?".to_string()];
    let mut values = vec![Value::Text(session_id.to_string())];

    let search = normalize(&query.search);
    if !search.is_empty() {
        if search.chars().count() >= 3 {
            // Fast path: FTS5 trigram MATCH (prefix/substring for ≥3 chars).
            // Quote the query to avoid FTS5 boolean parsing. We use `rowid IN (...)`
            // rather than `EXISTS (... fts.rowid = p.rowid ...)` so the planner drives
            // the FTS5 doclist (a small candidate set) and joins to people on rowid —
            // the EXISTS form forced a people scan with a per-row MATCH eval.
            clauses.push(
                "p.rowid IN (
                    SELECT rowid FROM people_search_fts WHERE people_search_fts MATCH ?
                    UNION
                    SELECT rowid FROM people_search_fts_v2 WHERE people_search_fts_v2 MATCH ?
                 ) \
                 AND p.search_text LIKE ? ESCAPE '\\'"
                    .into(),
            );
            values.push(Value::Text(fts_trigram_query(&search)));
            values.push(Value::Text(fts_trigram_query(&search)));
            values.push(Value::Text(contains_pattern(&search)));
        } else {
            // Fallback for ≤2-char queries: trigram tokenizer floor; LIKE contains stays correct.
            clauses.push("p.search_text LIKE ? ESCAPE '\\'".into());
            values.push(Value::Text(contains_pattern(&search)));
        }
    }
    if !query.level.trim().is_empty() && query.level != "全部等级" {
        clauses.push("p.level = ?".into());
        values.push(Value::Text(query.level.clone()));
    }
    match query.alert_state.as_str() {
        "仅预警人员" => clauses.push("p.alert_count > 0".into()),
        "未预警人员" => clauses.push("p.alert_count = 0".into()),
        _ => {}
    }
    if let Some(min_age) = query.min_age {
        clauses.push("p.age >= ?".into());
        values.push(Value::Integer(i64_from_usize(min_age)));
    }
    if let Some(max_age) = query.max_age {
        clauses.push("p.age <= ?".into());
        values.push(Value::Integer(i64_from_usize(max_age)));
    }
    if !query.gender.trim().is_empty() {
        clauses.push("p.gender = ?".into());
        values.push(Value::Text(query.gender.clone()));
    }

    for hotel in split_hotel_terms(&query.hotel_search) {
        // Hotel-name fuzzy match is ordered-subsequence (`%a%b%c%`), NOT substring
        // contains. FTS5 trigram MATCH implements substring contains, so it cannot
        // serve as a sound prefilter here (false-negatives like `商务b` against
        // `商务宾馆b`). Keep the LIKE-only path; per-person hotel cardinality is
        // small so the EXISTS correlated scan stays bounded.
        clauses.push(
            "EXISTS (SELECT 1 FROM person_hotels ph \
             WHERE ph.session_id = p.session_id AND ph.person_key = p.person_key \
             AND ph.hotel_name_norm LIKE ? ESCAPE '\\')"
                .into(),
        );
        values.push(Value::Text(fuzzy_pattern(&hotel)));
    }

    let hotel_regions = [
        (
            "phr.province_norm",
            split_filter_terms(&query.hotel_province),
        ),
        ("phr.city_norm", split_filter_terms(&query.hotel_city)),
        ("phr.county_norm", split_filter_terms(&query.hotel_county)),
    ];
    let region_clauses = hotel_regions
        .iter()
        .filter_map(|(column, terms)| contains_any_clause(column, terms, &mut values))
        .collect::<Vec<_>>();
    if !region_clauses.is_empty() {
        clauses.push(format!(
            "EXISTS (SELECT 1 FROM person_hotel_regions phr \
             WHERE phr.session_id = p.session_id AND phr.person_key = p.person_key AND {})",
            region_clauses.join(" AND ")
        ));
    }

    // Household include filters use OR within one split field and AND across fields.
    let household_splits = [
        (
            "p.household_province_norm",
            split_filter_terms(&query.household_province),
        ),
        (
            "p.household_city_norm",
            split_filter_terms(&query.household_city),
        ),
        (
            "p.household_county_norm",
            split_filter_terms(&query.household_county),
        ),
    ];
    for (column, terms) in household_splits {
        if let Some(clause) = contains_any_clause(column, &terms, &mut values) {
            clauses.push(clause);
        }
    }
    let excluded = [
        (
            "p.household_province_norm",
            split_filter_terms(&query.exclude_household_province),
        ),
        (
            "p.household_city_norm",
            split_filter_terms(&query.exclude_household_city),
        ),
        (
            "p.household_county_norm",
            split_filter_terms(&query.exclude_household_county),
        ),
    ];
    let excluded_clauses = excluded
        .iter()
        .filter_map(|(column, terms)| contains_any_clause(column, terms, &mut values))
        .collect::<Vec<_>>();
    if !excluded_clauses.is_empty() {
        clauses.push(format!("NOT ({})", excluded_clauses.join(" OR ")));
    }
    (clauses.join(" AND "), values)
}

pub(crate) fn build_records_filter(
    session_id: &str,
    query: &ImportedRecordsQuery,
    settings: &AnalysisSettings,
) -> (String, Vec<Value>) {
    let mut clauses = vec![
        "session_id = ?".to_string(),
        "check_in IS NOT NULL".to_string(),
    ];
    let mut values = vec![Value::Text(session_id.to_string())];
    if settings.frequency_mode == FrequencyMode::Selected {
        if let Some(start) = settings.frequency_start {
            clauses.push("check_in >= ?".into());
            values.push(Value::Text(start.format("%Y-%m-%d %H:%M:%S").to_string()));
        }
        if let Some(end) = settings.frequency_end {
            clauses.push("check_in <= ?".into());
            values.push(Value::Text(end.format("%Y-%m-%d %H:%M:%S").to_string()));
        }
    }

    let search = normalize(&query.search);
    if !search.is_empty() {
        if search.chars().count() >= 3 {
            // FTS5 trigram MATCH: indexed substring for ≥3 chars. rowid-in-subselect
            // drives the FTS5 doclist; ON-clause on rowid drives a seek into records.
            clauses.push(
                "rowid IN (
                    SELECT rowid FROM records_search_fts WHERE records_search_fts MATCH ?
                    UNION
                    SELECT rowid FROM records_search_fts_v2 WHERE records_search_fts_v2 MATCH ?
                 ) \
                 AND search_text LIKE ? ESCAPE '\\'"
                    .into(),
            );
            values.push(Value::Text(fts_trigram_query(&search)));
            values.push(Value::Text(fts_trigram_query(&search)));
            values.push(Value::Text(contains_pattern(&search)));
        } else {
            clauses.push("search_text LIKE ? ESCAPE '\\'".into());
            values.push(Value::Text(contains_pattern(&search)));
        }
    }
    if let Some(min_age) = query.min_age {
        clauses.push("age >= ?".into());
        values.push(Value::Integer(i64_from_usize(min_age)));
    }
    if let Some(max_age) = query.max_age {
        clauses.push("age <= ?".into());
        values.push(Value::Integer(i64_from_usize(max_age)));
    }
    if !query.gender.trim().is_empty() {
        clauses.push("gender = ?".into());
        values.push(Value::Text(query.gender.clone()));
    }

    for hotel in split_hotel_terms(&query.hotel_search) {
        // Records hotel-name filter uses ordered-subsequence `fuzzy_pattern`; trigram
        // MATCH (substring contains) cannot serve as a prefilter without losing matches.
        // The records scan is bounded by session_id partition; filter is on the indexed
        // (session_id, hotel_name_norm) range plus the LIKE post-filter.
        clauses.push("hotel_name_norm LIKE ? ESCAPE '\\'".into());
        values.push(Value::Text(fuzzy_pattern(&hotel)));
    }

    // Hotel jurisdiction uses OR within one split field and AND across fields.
    for (column, terms) in [
        (
            "hotel_province_norm",
            split_filter_terms(&query.hotel_province),
        ),
        ("hotel_city_norm", split_filter_terms(&query.hotel_city)),
        ("hotel_county_norm", split_filter_terms(&query.hotel_county)),
    ] {
        if let Some(clause) = contains_any_clause(column, &terms, &mut values) {
            clauses.push(clause);
        }
    }

    // Household include filters use the same split-field semantics as people queries.
    let household_splits = [
        (
            "household_province_norm",
            split_filter_terms(&query.household_province),
        ),
        (
            "household_city_norm",
            split_filter_terms(&query.household_city),
        ),
        (
            "household_county_norm",
            split_filter_terms(&query.household_county),
        ),
    ];
    for (column, terms) in household_splits {
        if let Some(clause) = contains_any_clause(column, &terms, &mut values) {
            clauses.push(clause);
        }
    }
    let excluded = [
        (
            "household_province_norm",
            split_filter_terms(&query.exclude_household_province),
        ),
        (
            "household_city_norm",
            split_filter_terms(&query.exclude_household_city),
        ),
        (
            "household_county_norm",
            split_filter_terms(&query.exclude_household_county),
        ),
    ];
    let excluded_clauses = excluded
        .iter()
        .filter_map(|(column, terms)| contains_any_clause(column, terms, &mut values))
        .collect::<Vec<_>>();
    if !excluded_clauses.is_empty() {
        clauses.push(format!("NOT ({})", excluded_clauses.join(" OR ")));
    }

    (clauses.join(" AND "), values)
}

pub(crate) fn records_count_source(query: &ImportedRecordsQuery) -> &'static str {
    if has_filter_terms(&query.hotel_province)
        || has_filter_terms(&query.hotel_city)
        || has_filter_terms(&query.hotel_county)
    {
        "records INDEXED BY idx_records_hotel_region"
    } else if has_filter_terms(&query.household_province)
        || has_filter_terms(&query.household_city)
        || has_filter_terms(&query.household_county)
        || has_filter_terms(&query.exclude_household_province)
        || has_filter_terms(&query.exclude_household_city)
        || has_filter_terms(&query.exclude_household_county)
    {
        "records INDEXED BY idx_records_household_split"
    } else if has_filter_terms(&query.hotel_search) {
        "records INDEXED BY idx_records_hotel_name"
    } else {
        "records"
    }
}

pub(crate) fn fast_record_filter_count(
    connection: &Connection,
    session_id: &str,
    query: &ImportedRecordsQuery,
    settings: &AnalysisSettings,
) -> Result<Option<i64>, AppError> {
    if settings.frequency_mode == FrequencyMode::Selected
        || !query.search.trim().is_empty()
        || query.min_age.is_some()
        || query.max_age.is_some()
        || !query.gender.trim().is_empty()
        || has_filter_terms(&query.exclude_household_province)
        || has_filter_terms(&query.exclude_household_city)
        || has_filter_terms(&query.exclude_household_county)
    {
        return Ok(None);
    }

    let hotel_regions = [
        ("hotel_province", split_filter_terms(&query.hotel_province)),
        ("hotel_city", split_filter_terms(&query.hotel_city)),
        ("hotel_county", split_filter_terms(&query.hotel_county)),
    ];
    let household_regions = [
        (
            "household_province",
            split_filter_terms(&query.household_province),
        ),
        ("household_city", split_filter_terms(&query.household_city)),
        (
            "household_county",
            split_filter_terms(&query.household_county),
        ),
    ];
    let active_regions = hotel_regions
        .into_iter()
        .chain(household_regions)
        .filter(|(_, terms)| !terms.is_empty())
        .collect::<Vec<_>>();
    let hotel_terms = split_hotel_terms(&query.hotel_search);

    match (active_regions.as_slice(), hotel_terms.as_slice()) {
        ([(filter_kind, terms)], []) => {
            let mut clauses = vec!["session_id = ?".to_string(), "filter_kind = ?".to_string()];
            let mut values = vec![
                Value::Text(session_id.to_string()),
                Value::Text((*filter_kind).to_string()),
            ];
            if let Some(clause) = contains_any_clause("value_norm", terms, &mut values) {
                clauses.push(clause);
            }
            let sql = format!(
                "SELECT COALESCE(SUM(record_count), 0) FROM record_filter_counts WHERE {}",
                clauses.join(" AND ")
            );
            let total = connection
                .query_row(&sql, params_from_iter(values.iter()), |row| row.get(0))
                .map_err(sql_error)?;
            Ok(Some(total))
        }
        ([], terms) if !terms.is_empty() => {
            let mut clauses = vec![
                "session_id = ?".to_string(),
                "filter_kind = 'hotel_name'".to_string(),
            ];
            let mut values = vec![Value::Text(session_id.to_string())];
            for term in terms {
                clauses.push("value_norm LIKE ? ESCAPE '\\'".into());
                values.push(Value::Text(fuzzy_pattern(term)));
            }
            let sql = format!(
                "SELECT COALESCE(SUM(record_count), 0) FROM record_filter_counts WHERE {}",
                clauses.join(" AND ")
            );
            let total = connection
                .query_row(&sql, params_from_iter(values.iter()), |row| row.get(0))
                .map_err(sql_error)?;
            Ok(Some(total))
        }
        _ => Ok(None),
    }
}

pub(crate) fn increment_record_filter_count(
    counts: &mut HashMap<(String, String), i64>,
    filter_kind: &str,
    value_norm: &str,
) {
    if value_norm.is_empty() {
        return;
    }
    *counts
        .entry((filter_kind.to_string(), value_norm.to_string()))
        .or_insert(0) += 1;
}

pub(crate) fn split_hotel_terms(value: &str) -> Vec<String> {
    split_filter_terms(value)
}

pub(crate) fn split_filter_terms(value: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    value
        .split([',', '，', '、', ';', '；', '\n', '\r'])
        .filter_map(|part| {
            let term = normalize(part);
            if term.is_empty() || !seen.insert(term.clone()) {
                None
            } else {
                Some(term)
            }
        })
        .collect()
}

pub(crate) fn has_filter_terms(value: &str) -> bool {
    value
        .split([',', '，', '、', ';', '；', '\n', '\r'])
        .any(|part| !normalize(part).is_empty())
}

pub(crate) fn contains_any_clause(
    column: &str,
    terms: &[String],
    values: &mut Vec<Value>,
) -> Option<String> {
    if terms.is_empty() {
        return None;
    }
    let mut clauses = Vec::with_capacity(terms.len());
    for term in terms {
        clauses.push(format!("{column} LIKE ? ESCAPE '\\'"));
        values.push(Value::Text(contains_pattern(term)));
    }
    Some(format!("({})", clauses.join(" OR ")))
}

pub(crate) fn normalize(value: &str) -> String {
    let mut normalized = value.trim().to_lowercase();
    normalized.retain(|character| !character.is_whitespace());
    normalized
}

pub(crate) fn contains_pattern(value: &str) -> String {
    format!("%{}%", escape_like(value))
}

pub(crate) fn fuzzy_pattern(value: &str) -> String {
    let mut pattern = String::from("%");
    for character in value.chars() {
        match character {
            '%' | '_' | '\\' => pattern.push('\\'),
            _ => {}
        }
        pattern.push(character);
        pattern.push('%');
    }
    pattern
}

pub(crate) fn fts_trigram_query(value: &str) -> String {
    // Compact detail=none FTS tables accept only three-character tokens. Use every
    // overlapping trigram as an AND candidate set; the source-table LIKE predicate
    // then confirms ordering and exact substring semantics.
    let characters = value.chars().collect::<Vec<_>>();
    characters
        .windows(3)
        .map(|window| {
            let token = window.iter().collect::<String>().replace('"', "\"\"");
            format!("\"{token}\"")
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

pub(crate) fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
