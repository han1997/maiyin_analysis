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
        .filter(|record| within_analysis_scope(record, settings))
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
    records.retain(|record| within_analysis_scope(record, settings));
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

    for (day, day_records) in by_day {
        if day_records.len() < 2 {
            continue;
        }
        let mut pair_count: usize = 0;
        let mut pair_labels = Vec::new();
        for index in 0..day_records.len() {
            for second in day_records.iter().skip(index + 1) {
                let first_record = day_records[index];
                if intervals_overlap(first_record, second)
                    && different_accommodation(first_record, second)
                {
                    pair_count += 1;
                    if pair_labels.len() < 4 {
                        pair_labels.push(format!(
                            "{} {} 与 {} {}",
                            fallback(&first_record.hotel_name, "未填旅馆"),
                            fallback(&first_record.room_no, "未填房间"),
                            fallback(&second.hotel_name, "未填旅馆"),
                            fallback(&second.room_no, "未填房间"),
                        ));
                    }
                }
            }
        }

        let evidence_ids = day_records
            .iter()
            .map(|record| record.uid)
            .collect::<Vec<_>>();
        if pair_count > 0 {
            overlap_days += 1;
            alerts.push(AlertSummary {
                kind: "overlap".into(),
                severity: "高".into(),
                score: (45 + (pair_count.saturating_sub(1) as u32) * 5).min(60),
                title: format!("{} 辖区内入住时间重合", day),
                detail: format!(
                    "{} 条记录，重合关系：{}",
                    day_records.len(),
                    pair_labels.join("；")
                ),
                evidence_count: evidence_ids.len(),
                evidence_ids,
            });
        } else if day_records.len() > 3 {
            sequential_days += 1;
            alerts.push(AlertSummary {
                kind: "daily_frequency".into(),
                severity: "中".into(),
                score: (25 + ((day_records.len() - 4) as u32) * 5).min(45),
                title: format!("{} 辖区内非重合入住超过 3 次", day),
                detail: format!(
                    "{} 条有效记录未发现不同酒店或不同房间的时间重合；不足 10 分钟的记录已排除。",
                    day_records.len()
                ),
                evidence_count: evidence_ids.len(),
                evidence_ids,
            });
        }
    }

    let month_count = max_window_count(&records, 30);
    let year_count = max_window_count(&records, 365);
    if month_count > settings.month_threshold {
        let difference = month_count - settings.month_threshold;
        alerts.push(AlertSummary {
            kind: "month_frequency".into(),
            severity: if difference >= 3 {
                "高".into()
            } else {
                "中".into()
            },
            score: (30 + difference as u32 * 2).min(50),
            title: format!("30 天内入住 {} 次", month_count),
            detail: format!("超过页面设置阈值 {} 次。", settings.month_threshold),
            evidence_count: records.len(),
            evidence_ids: records.iter().map(|record| record.uid).collect(),
        });
    }
    if year_count > settings.year_threshold {
        let difference = year_count - settings.year_threshold;
        alerts.push(AlertSummary {
            kind: "year_frequency".into(),
            severity: if difference >= 6 {
                "高".into()
            } else {
                "中".into()
            },
            score: (35 + difference as u32).min(55),
            title: format!("365 天内入住 {} 次", year_count),
            detail: format!("超过页面设置阈值 {} 次。", settings.year_threshold),
            evidence_count: records.len(),
            evidence_ids: records.iter().map(|record| record.uid).collect(),
        });
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
        max_month_count: month_count,
        max_year_count: year_count,
        overlap_days,
        sequential_days,
        score,
        level,
        alert_count: alerts.len(),
        alert_titles: alerts.iter().map(|alert| alert.title.clone()).collect(),
    };
    Some(PersonAnalysis { summary, alerts })
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

pub fn intervals_overlap(first: &Record, second: &Record) -> bool {
    let (Some(first_start), Some(second_start)) = (first.check_in, second.check_in) else {
        return false;
    };
    let first_end = effective_end(first);
    let second_end = effective_end(second);
    first_start < second_end && second_start < first_end
}

pub fn different_accommodation(first: &Record, second: &Record) -> bool {
    let first_hotel = compact(&first.hotel_name);
    let second_hotel = compact(&second.hotel_name);
    let first_room = compact(&first.room_no);
    let second_room = compact(&second.room_no);
    (!first_hotel.is_empty() && !second_hotel.is_empty() && first_hotel != second_hotel)
        || (!first_room.is_empty() && !second_room.is_empty() && first_room != second_room)
}

pub fn max_window_count(records: &[&Record], days: i64) -> usize {
    let mut timestamps: Vec<NaiveDateTime> = records
        .iter()
        .filter_map(|record| record.check_in)
        .collect();
    timestamps.sort();
    let mut maximum = 0;
    let mut left = 0;
    for right in 0..timestamps.len() {
        while timestamps[right] - timestamps[left] > Duration::days(days) {
            left += 1;
        }
        maximum = maximum.max(right - left + 1);
    }
    maximum
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
    fn overlap_requires_different_room_or_hotel() {
        let first = record(1, "301", "2026-05-01 09:30", "2026-05-01 13:00");
        let same = record(2, "301", "2026-05-01 10:00", "2026-05-01 12:00");
        let other = record(3, "302", "2026-05-01 10:00", "2026-05-01 12:00");
        assert!(intervals_overlap(&first, &same));
        assert!(!different_accommodation(&first, &same));
        assert!(different_accommodation(&first, &other));
    }

    #[test]
    fn scores_overlap_and_window_frequency() {
        let mut records = Vec::new();
        for day in 1..=8 {
            records.push(record(
                day,
                if day == 2 { "302" } else { "301" },
                &format!("2026-05-{day:02} 09:30"),
                &format!("2026-05-{day:02} 13:00"),
            ));
        }
        records[1].check_in = NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(10, 0, 0);
        let (analyses, _) = analyze_records(&records, &AnalysisSettings::default());
        assert_eq!(analyses.len(), 1);
        assert!(analyses[0].summary.score >= 77);
        assert!(analyses[0].alerts.iter().any(|item| item.kind == "overlap"));
        assert!(analyses[0]
            .alerts
            .iter()
            .any(|item| item.kind == "month_frequency"));
    }
}
