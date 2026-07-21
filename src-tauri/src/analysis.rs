use crate::model::{
    AlertSummary, AnalysisSettings, AnalysisStats, PersonAnalysis, PersonSummary, Record,
};
use chrono::{Duration, NaiveDate, NaiveDateTime};
use std::collections::{BTreeMap, HashMap};

pub fn analyze_records(
    records: &[Record],
    settings: &AnalysisSettings,
) -> (Vec<PersonAnalysis>, AnalysisStats) {
    let mut grouped: HashMap<&str, Vec<&Record>> = HashMap::new();
    for record in records {
        grouped.entry(&record.person_key).or_default().push(record);
    }

    let mut analyses: Vec<PersonAnalysis> = grouped
        .into_values()
        .filter_map(|group| analyze_person(group, settings))
        .collect();
    analyses.sort_by(|left, right| {
        right
            .summary
            .score
            .cmp(&left.summary.score)
            .then_with(|| right.summary.total_records.cmp(&left.summary.total_records))
            .then_with(|| left.summary.name.cmp(&right.summary.name))
    });

    let scoped: Vec<&Record> = records
        .iter()
        .filter(|record| {
            within_analysis_scope(record, settings) && within_analysis_time_window(record, settings)
        })
        .collect();
    let stats = AnalysisStats {
        records: analyses.iter().map(|item| item.summary.total_records).sum(),
        people: analyses.len(),
        alerted: analyses
            .iter()
            .filter(|item| !item.alerts.is_empty())
            .count(),
        high: analyses
            .iter()
            .filter(|item| item.summary.level == "高风险")
            .count(),
        issues: scoped
            .iter()
            .filter(|record| !record.issues.is_empty())
            .count(),
    };
    (analyses, stats)
}

fn analyze_person(
    mut records: Vec<&Record>,
    settings: &AnalysisSettings,
) -> Option<PersonAnalysis> {
    records.retain(|record| {
        within_analysis_scope(record, settings) && within_analysis_time_window(record, settings)
    });
    records.sort_by_key(|record| record.check_in.unwrap_or(NaiveDateTime::MIN));
    let first = *records.first()?;

    let mut by_day: BTreeMap<NaiveDate, Vec<&Record>> = BTreeMap::new();
    for record in &records {
        if let Some(check_in) = record.check_in {
            by_day.entry(check_in.date()).or_default().push(record);
        }
    }

    let mut alerts = Vec::new();
    let mut overlap_days = 0;
    let mut sequential_days = 0;
    let mut location_cache = HashMap::new();

    let overlap_pairs_by_day = overlapping_stay_pairs(&records);
    for (day, day_records) in &by_day {
        let evidence_ids = day_records
            .iter()
            .map(|record| record.uid)
            .collect::<Vec<_>>();
        if let Some(pairs) = overlap_pairs_by_day.get(day) {
            overlap_days += 1;
            let different_place_count = pairs
                .iter()
                .filter(|(first, second)| {
                    different_accommodation_cached(first, second, &mut location_cache)
                })
                .count();
            let pair_labels = pairs
                .iter()
                .take(4)
                .map(|(first_record, second)| {
                    format!(
                        "{} {} 与 {} {}",
                        fallback(&first_record.hotel_name, "未填旅馆"),
                        fallback(&first_record.room_no, "未填房间"),
                        fallback(&second.hotel_name, "未填旅馆"),
                        fallback(&second.room_no, "未填房间"),
                    )
                })
                .collect::<Vec<_>>();
            let evidence_ids = pairs
                .iter()
                .flat_map(|(first, second)| [first.uid, second.uid])
                .fold(Vec::new(), |mut ids, uid| {
                    if !ids.contains(&uid) {
                        ids.push(uid);
                    }
                    ids
                });
            alerts.push(AlertSummary {
                kind: "overlap".into(),
                severity: if different_place_count > 0 {
                    "高"
                } else {
                    "中"
                }
                .into(),
                score: overlap_score(pairs.len(), different_place_count),
                title: format!("{} 入住时间重叠", day),
                detail: format!(
                    "{} 对记录存在入住到退房时间交叉；其中 {} 对酒店或房号不同：{}",
                    pairs.len(),
                    different_place_count,
                    pair_labels.join("；")
                ),
                evidence_count: evidence_ids.len(),
                evidence_ids,
            });
        } else if day_records.len() > 3 {
            sequential_days += 1;
            alerts.push(AlertSummary {
                kind: "same_day_many".into(),
                severity: "中".into(),
                score: (25 + ((day_records.len() - 4) as u32) * 5).min(45),
                title: format!("{} 辖区内非重合入住超过 3 次", day),
                detail: format!(
                    "{} 条有效记录未发现入住时间重叠；不足 10 分钟的记录已排除。",
                    day_records.len()
                ),
                evidence_count: evidence_ids.len(),
                evidence_ids,
            });
        }
    }

    let week_records = max_window_records(&records, 7);
    let month_records = max_window_records(&records, 30);
    let year_records = max_window_records(&records, 365);
    let use_selected_window =
        settings.frequency_start.is_some() || settings.frequency_end.is_some();
    if use_selected_window && records.len() > settings.frequency_threshold {
        alerts.push(frequency_alert(
            "window_frequency",
            "时间窗口",
            &records,
            settings.frequency_threshold,
        ));
    } else if !use_selected_window {
        for (kind, label, window_records, threshold) in [
            (
                "week_frequency",
                "7 天",
                &week_records,
                settings.week_threshold,
            ),
            (
                "month_frequency",
                "30 天",
                &month_records,
                settings.month_threshold,
            ),
            (
                "year_frequency",
                "365 天",
                &year_records,
                settings.year_threshold,
            ),
        ] {
            if window_records.len() > threshold {
                alerts.push(frequency_alert(kind, label, window_records, threshold));
            }
        }
    }

    let score = alerts.iter().map(|alert| alert.score).sum::<u32>().min(100);
    let level = level_from_score(score).to_string();
    let summary = PersonSummary {
        person_key: first.person_key.clone(),
        name: fallback(&first.name, "未填").to_string(),
        id_no: first.id_no.clone(),
        phone: first.phone.clone(),
        household_region: first.household_region.clone(),
        age: first.age,
        gender: first.gender.clone(),
        total_records: records.len(),
        max_week_count: week_records.len(),
        max_month_count: month_records.len(),
        max_year_count: year_records.len(),
        overlap_days,
        sequential_days,
        score,
        level,
        alert_count: alerts.len(),
        alert_titles: alerts.iter().map(|alert| alert.title.clone()).collect(),
        hotel_names: records.iter().fold(Vec::new(), |mut names, record| {
            if !record.hotel_name.trim().is_empty() && !names.contains(&record.hotel_name) {
                names.push(record.hotel_name.clone());
            }
            names
        }),
    };
    Some(PersonAnalysis { summary, alerts })
}

fn frequency_alert(kind: &str, label: &str, records: &[&Record], threshold: usize) -> AlertSummary {
    let count = records.len();
    AlertSummary {
        kind: kind.into(),
        severity: if count >= threshold + 2 { "高" } else { "中" }.into(),
        score: (45 + ((count - threshold) as u32) * 6).min(80),
        title: format!("{}内入住 {} 次", label, count),
        detail: format!("{}内超过页面设置阈值 {} 次。", label, threshold),
        evidence_count: count,
        evidence_ids: records.iter().map(|record| record.uid).collect(),
    }
}

fn overlap_score(pair_count: usize, different_place_count: usize) -> u32 {
    (20 + pair_count as u32 * 2 + different_place_count as u32 * 5).min(35)
}

fn overlapping_stay_pairs<'a>(
    records: &[&'a Record],
) -> BTreeMap<NaiveDate, Vec<(&'a Record, &'a Record)>> {
    let mut pairs = BTreeMap::new();
    for (index, first) in records.iter().enumerate() {
        let Some(first_start) = first.check_in else {
            continue;
        };
        let first_end = effective_end(first);
        for second in records.iter().skip(index + 1) {
            let Some(second_start) = second.check_in else {
                continue;
            };
            if second_start >= first_end {
                break;
            }
            if first_start < effective_end(second) && intervals_overlap(first, second) {
                pairs
                    .entry(second_start.date())
                    .or_insert_with(Vec::new)
                    .push((*first, *second));
            }
        }
    }
    pairs
}

pub fn within_analysis_scope(record: &Record, settings: &AnalysisSettings) -> bool {
    let jurisdiction = [
        (
            &settings.province,
            [
                &record.province,
                &record.region,
                &record.hotel_name,
                &record.address,
            ],
        ),
        (
            &settings.city,
            [
                &record.city,
                &record.region,
                &record.hotel_name,
                &record.address,
            ],
        ),
        (
            &settings.county,
            [
                &record.county,
                &record.region,
                &record.hotel_name,
                &record.address,
            ],
        ),
    ];
    if jurisdiction.iter().any(|(needle, fields)| {
        !needle.trim().is_empty() && !fields.iter().any(|value| contains(value, needle))
    }) {
        return false;
    }

    let include_household = [
        (&settings.household_province, &record.household_province),
        (&settings.household_city, &record.household_city),
        (&settings.household_county, &record.household_county),
    ];
    if include_household
        .iter()
        .any(|(needle, value)| !needle.trim().is_empty() && !contains(value, needle))
    {
        return false;
    }
    let exclude_household = [
        (
            &settings.exclude_household_province,
            &record.household_province,
        ),
        (&settings.exclude_household_city, &record.household_city),
        (&settings.exclude_household_county, &record.household_county),
    ];
    if exclude_household
        .iter()
        .any(|(needle, value)| !needle.trim().is_empty() && contains(value, needle))
    {
        return false;
    }
    if settings
        .min_age
        .is_some_and(|minimum| record.age.is_none_or(|age| age < minimum))
    {
        return false;
    }
    if settings
        .max_age
        .is_some_and(|maximum| record.age.is_none_or(|age| age > maximum))
    {
        return false;
    }
    if !settings.gender.is_empty() && record.gender != settings.gender {
        return false;
    }
    true
}

pub fn within_analysis_time_window(record: &Record, settings: &AnalysisSettings) -> bool {
    let Some(check_in) = record.check_in else {
        return false;
    };
    if settings
        .frequency_start
        .is_some_and(|start| check_in < start)
    {
        return false;
    }
    if settings.frequency_end.is_some_and(|end| check_in > end) {
        return false;
    }
    true
}

pub fn intervals_overlap(first: &Record, second: &Record) -> bool {
    let (Some(first_start), Some(second_start)) = (first.check_in, second.check_in) else {
        return false;
    };
    let first_end = effective_end(first);
    let second_end = effective_end(second);
    first_start < second_end && second_start < first_end
}

fn different_accommodation_cached(
    first: &Record,
    second: &Record,
    cache: &mut HashMap<u64, (String, String)>,
) -> bool {
    let first_location = cache
        .entry(first.uid)
        .or_insert_with(|| (compact(&first.hotel_name), compact(&first.room_no)))
        .clone();
    let second_location = cache
        .entry(second.uid)
        .or_insert_with(|| (compact(&second.hotel_name), compact(&second.room_no)));
    (!first_location.0.is_empty()
        && !second_location.0.is_empty()
        && first_location.0 != second_location.0)
        || (!first_location.1.is_empty()
            && !second_location.1.is_empty()
            && first_location.1 != second_location.1)
}

fn max_window_records<'a>(records: &[&'a Record], days: i64) -> Vec<&'a Record> {
    let ordered = records
        .iter()
        .copied()
        .filter(|record| record.check_in.is_some())
        .collect::<Vec<_>>();
    let mut best = (0, 0);
    let mut end = 0;
    for start in 0..ordered.len() {
        let window_end =
            ordered[start].check_in.unwrap_or(NaiveDateTime::MIN) + Duration::days(days);
        while end < ordered.len()
            && ordered[end]
                .check_in
                .is_some_and(|value| value <= window_end)
        {
            end += 1;
        }
        if end - start > best.1 - best.0 {
            best = (start, end);
        }
    }
    ordered[best.0..best.1].to_vec()
}

fn effective_end(record: &Record) -> NaiveDateTime {
    let start = record.check_in.unwrap_or(NaiveDateTime::MIN);
    record
        .check_out
        .filter(|end| *end > start)
        .unwrap_or(start + Duration::days(1))
}

fn level_from_score(score: u32) -> &'static str {
    match score {
        80.. => "高风险",
        55..=79 => "中风险",
        30..=54 => "关注",
        _ => "正常",
    }
}

fn contains(value: &str, needle: &str) -> bool {
    compact(value).contains(&compact(needle))
}
fn compact(value: &str) -> String {
    value.split_whitespace().collect::<String>().to_lowercase()
}
fn fallback<'a>(value: &'a str, default: &'a str) -> &'a str {
    if value.trim().is_empty() {
        default
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Record;
    use chrono::NaiveDate;

    fn record(uid: u64, room: &str, check_in: &str, check_out: &str) -> Record {
        let parse = |value: &str| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M").ok();
        Record {
            uid,
            source_file: "test.xlsx".into(),
            source_row: uid as usize,
            name: "测试人员".into(),
            id_no: "341024198809128135".into(),
            phone: "13905591234".into(),
            hotel_name: "测试旅馆".into(),
            province: "安徽省".into(),
            city: "黄山市".into(),
            county: "祁门县".into(),
            region: "安徽省黄山市祁门县".into(),
            address: "测试路".into(),
            room_no: room.into(),
            check_in_text: check_in.into(),
            register_time_text: String::new(),
            check_out_text: check_out.into(),
            check_in: parse(check_in),
            register_time: None,
            check_out: parse(check_out),
            person_key: "id:341024198809128135".into(),
            household_province: "安徽省".into(),
            household_city: "黄山市".into(),
            household_county: "祁门县".into(),
            household_region: "安徽省 黄山市 祁门县".into(),
            household_address: String::new(),
            age: Some(37),
            gender: "男".into(),
            issues: vec![],
        }
    }

    #[test]
    fn same_room_overlap_alerts_and_different_room_scores_higher() {
        let first = record(1, "301", "2026-05-01 09:30", "2026-05-01 13:00");
        let same = record(2, "301", "2026-05-01 10:00", "2026-05-01 12:00");
        let same_result = analyze_records(&[first.clone(), same], &AnalysisSettings::default()).0;
        let same_alert = &same_result[0].alerts[0];
        assert_eq!(same_alert.kind, "overlap");
        assert_eq!(same_alert.score, 22);
        assert_eq!(same_alert.severity, "中");

        let other = record(3, "302", "2026-05-01 10:00", "2026-05-01 12:00");
        let other_result = analyze_records(&[first, other], &AnalysisSettings::default()).0;
        assert_eq!(other_result[0].alerts[0].score, 27);
        assert_eq!(other_result[0].alerts[0].severity, "高");
    }

    #[test]
    fn selected_window_frequency_disables_rolling_frequency() {
        let mut records = Vec::new();
        for day in 1..=4 {
            records.push(record(
                day,
                "301",
                &format!("2026-05-{day:02} 09:30"),
                &format!("2026-05-{day:02} 13:00"),
            ));
        }
        let settings = AnalysisSettings {
            frequency_start: NaiveDate::from_ymd_opt(2026, 5, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0),
            ..Default::default()
        };
        let analyses = analyze_records(&records, &settings).0;
        assert_eq!(analyses[0].alerts.len(), 1);
        assert_eq!(analyses[0].alerts[0].kind, "window_frequency");
        assert_eq!(analyses[0].alerts[0].score, 51);
    }

    #[test]
    fn analysis_window_excludes_records_from_counts_and_evidence() {
        let records = vec![
            record(1, "301", "2026-04-30 09:30", "2026-04-30 13:00"),
            record(2, "301", "2026-05-01 09:30", "2026-05-01 13:00"),
            record(3, "302", "2026-05-01 10:00", "2026-05-01 12:00"),
        ];
        let settings = AnalysisSettings {
            frequency_start: NaiveDate::from_ymd_opt(2026, 5, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0),
            frequency_end: NaiveDate::from_ymd_opt(2026, 5, 1)
                .unwrap()
                .and_hms_opt(23, 59, 0),
            ..Default::default()
        };
        let (analyses, stats) = analyze_records(&records, &settings);
        assert_eq!(analyses[0].summary.total_records, 2);
        assert_eq!(stats.records, 2);
        assert_eq!(analyses[0].alerts[0].evidence_ids, vec![2, 3]);
    }
}
