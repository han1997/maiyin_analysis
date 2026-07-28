use crate::model::{
    AlertSummary, AnalysisSettings, AnalysisStats, FrequencyMode, HotelRegion, PersonAnalysis,
    PersonSummary, Record,
};
use chrono::{Duration, NaiveDate, NaiveDateTime};
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

const PARALLEL_SORT_THRESHOLD: usize = 4_096;
const HASH_DEDUP_THRESHOLD: usize = 8;
const ROLLING_WINDOW_DAYS: [i64; 3] = [7, 30, 365];
const DENSE_OVERLAP_THRESHOLD: usize = 32;

#[derive(Clone, Copy, Default)]
struct WindowRange {
    start: usize,
    end: usize,
}

impl WindowRange {
    fn len(self) -> usize {
        self.end - self.start
    }
}

struct DayAnalysis {
    day: NaiveDate,
    start: usize,
    end: usize,
    overlap: Option<OverlapSummary>,
}

struct OverlapSummary {
    pair_count: usize,
    different_place_count: usize,
    pair_labels: Vec<String>,
    evidence_ids: Vec<u64>,
    evidence_seen: HashSet<u64>,
}

impl OverlapSummary {
    fn new() -> Self {
        Self {
            pair_count: 0,
            different_place_count: 0,
            pair_labels: Vec::with_capacity(4),
            evidence_ids: Vec::new(),
            evidence_seen: HashSet::new(),
        }
    }

    fn add_pair(&mut self, first: &Record, second: &Record, different_place: bool) {
        self.pair_count += 1;
        self.different_place_count += usize::from(different_place);
        if self.pair_labels.len() < 4 {
            self.pair_labels.push(format!(
                "{} {} 与 {} {}",
                fallback(&first.hotel_name, "未填旅馆"),
                fallback(&first.room_no, "未填房间"),
                fallback(&second.hotel_name, "未填旅馆"),
                fallback(&second.room_no, "未填房间"),
            ));
        }
        for uid in [first.uid, second.uid] {
            if self.evidence_seen.insert(uid) {
                self.evidence_ids.push(uid);
            }
        }
    }
}

pub fn analyze_records(
    records: &[Record],
    settings: &AnalysisSettings,
) -> (Vec<PersonAnalysis>, AnalysisStats) {
    let mut grouped: HashMap<&str, Vec<&Record>> = HashMap::new();
    let mut scoped_records = 0;
    let mut issues = 0;
    for record in records {
        if !within_analysis_time_window(record, settings) {
            continue;
        }
        scoped_records += 1;
        issues += usize::from(!record.issues.is_empty());
        grouped.entry(&record.person_key).or_default().push(record);
    }

    let mut analyses: Vec<PersonAnalysis> = grouped
        .into_par_iter()
        .map(|(_, group)| analyze_person(group, settings))
        .collect();
    if analyses.len() >= PARALLEL_SORT_THRESHOLD {
        analyses.par_sort_unstable_by(compare_analyses);
    } else {
        analyses.sort_unstable_by(compare_analyses);
    }

    let (alerted, high) = analyses.iter().fold((0, 0), |(alerted, high), item| {
        (
            alerted + usize::from(!item.alerts.is_empty()),
            high + usize::from(item.summary.level == "高风险"),
        )
    });
    let stats = AnalysisStats {
        records: scoped_records,
        people: analyses.len(),
        alerted,
        high,
        issues,
    };
    (analyses, stats)
}

fn compare_analyses(left: &PersonAnalysis, right: &PersonAnalysis) -> Ordering {
    right
        .summary
        .score
        .cmp(&left.summary.score)
        .then_with(|| right.summary.total_records.cmp(&left.summary.total_records))
        .then_with(|| left.summary.name.cmp(&right.summary.name))
        .then_with(|| left.summary.person_key.cmp(&right.summary.person_key))
}

fn detect_dense_day_overlaps(
    records: &[&Record],
    day_index: usize,
    days: &mut [DayAnalysis],
    location_cache: &mut HashMap<u64, (String, String)>,
) {
    let start = days[day_index].start;
    let end = days[day_index].end;
    let first_idx = (0..end).find(|&i| records[i].check_in.is_some());
    let last_idx = (start..end).rev().find(|&i| records[i].check_in.is_some());
    let (Some(first_idx), Some(last_idx)) = (first_idx, last_idx) else {
        return;
    };
    let last_check_in = records[last_idx].check_in.unwrap();
    let all_overlap = effective_end(records[first_idx]) > last_check_in;

    let mut summary = OverlapSummary::new();
    let mut active_count: usize = 0;
    let mut active_groups: HashMap<(String, String), usize> = HashMap::new();
    let mut active_by_end: BTreeMap<NaiveDateTime, Vec<usize>> = BTreeMap::new();
    let mut involved: HashSet<u64> = HashSet::new();

    for i in 0..end {
        let record = records[i];
        let Some(check_in) = record.check_in else {
            continue;
        };

        let expired_keys: Vec<NaiveDateTime> = active_by_end
            .range(..=check_in)
            .map(|(key, _)| *key)
            .collect();
        for key in expired_keys {
            if let Some(indices) = active_by_end.remove(&key) {
                for &idx in &indices {
                    let group_key = location_cache
                        .get(&records[idx].uid)
                        .cloned()
                        .unwrap_or_default();
                    if let Some(count) = active_groups.get_mut(&group_key) {
                        *count -= 1;
                        if *count == 0 {
                            active_groups.remove(&group_key);
                        }
                    }
                    active_count -= 1;
                }
            }
        }

        let group_key = location_cache
            .entry(record.uid)
            .or_insert_with(|| (compact(&record.hotel_name), compact(&record.room_no)))
            .clone();

        if i >= start {
            summary.pair_count += active_count;
            let same_place = active_groups.get(&group_key).copied().unwrap_or(0);
            summary.different_place_count += active_count - same_place;

            if !all_overlap {
                involved.insert(record.uid);
                for (_, indices) in active_by_end.iter() {
                    for &idx in indices {
                        involved.insert(records[idx].uid);
                    }
                }
            }
        }

        active_count += 1;
        *active_groups.entry(group_key).or_insert(0) += 1;
        active_by_end
            .entry(effective_end(record))
            .or_default()
            .push(i);
    }

    for second_idx in start..end {
        if summary.pair_labels.len() >= 4 {
            break;
        }
        if second_idx - start >= DENSE_OVERLAP_THRESHOLD {
            break;
        }
        let second = records[second_idx];
        let Some(second_check_in) = second.check_in else {
            continue;
        };
        let first_lower = second_idx.saturating_sub(DENSE_OVERLAP_THRESHOLD);
        for first in records[first_lower..second_idx].iter().copied() {
            if summary.pair_labels.len() >= 4 {
                break;
            }
            if first.check_in.is_none() {
                continue;
            }
            if effective_end(first) > second_check_in {
                summary.pair_labels.push(format!(
                    "{} {} 与 {} {}",
                    fallback(&first.hotel_name, "未填旅馆"),
                    fallback(&first.room_no, "未填房间"),
                    fallback(&second.hotel_name, "未填旅馆"),
                    fallback(&second.room_no, "未填房间"),
                ));
            }
        }
    }

    if all_overlap {
        summary.evidence_ids = (0..end)
            .filter_map(|i| records[i].check_in.map(|_| records[i].uid))
            .collect();
        summary.evidence_seen = summary.evidence_ids.iter().copied().collect();
    } else {
        summary.evidence_ids = (0..end)
            .filter_map(|i| {
                let record = records[i];
                if record.check_in.is_some() && involved.contains(&record.uid) {
                    Some(record.uid)
                } else {
                    None
                }
            })
            .collect();
        summary.evidence_seen = involved;
    }

    days[day_index].overlap = Some(summary);
}

fn analyze_person(mut records: Vec<&Record>, settings: &AnalysisSettings) -> PersonAnalysis {
    records.sort_by_key(|record| record.check_in.unwrap_or(NaiveDateTime::MIN));
    if records.len() == 1 {
        return analyze_single_record(records[0], settings);
    }

    let (mut days, record_days) = day_ranges(&records);
    let mut alerts = Vec::new();
    let mut overlap_days = 0;
    let mut sequential_days = 0;
    let mut location_cache = HashMap::new();

    let dense_days: Vec<usize> = days
        .iter()
        .enumerate()
        .filter(|(_, day)| day.end - day.start > DENSE_OVERLAP_THRESHOLD)
        .map(|(index, _)| index)
        .collect();
    for &day_index in &dense_days {
        detect_dense_day_overlaps(&records, day_index, &mut days, &mut location_cache);
    }
    let dense_day_set: HashSet<usize> = dense_days.into_iter().collect();

    for (first_index, first) in records.iter().enumerate() {
        let Some(first_start) = first.check_in else {
            continue;
        };
        let first_end = effective_end(first);
        for second_index in first_index + 1..records.len() {
            let second = records[second_index];
            let Some(second_start) = second.check_in else {
                continue;
            };
            if second_start >= first_end {
                break;
            }
            if dense_day_set.contains(&record_days[second_index]) {
                continue;
            }
            if first_start < effective_end(second) {
                let different_place =
                    different_accommodation_cached(first, second, &mut location_cache);
                days[record_days[second_index]]
                    .overlap
                    .get_or_insert_with(OverlapSummary::new)
                    .add_pair(first, second, different_place);
            }
        }
    }

    for day in days {
        if let Some(overlap) = day.overlap {
            overlap_days += 1;
            alerts.push(AlertSummary {
                kind: "overlap".into(),
                severity: if overlap.different_place_count > 0 {
                    "高"
                } else {
                    "中"
                }
                .into(),
                score: overlap_score(overlap.pair_count, overlap.different_place_count),
                title: format!("{} 入住时间重叠", day.day),
                detail: format!(
                    "{} 对记录存在入住到退房时间交叉；其中 {} 对酒店或房号不同：{}",
                    overlap.pair_count,
                    overlap.different_place_count,
                    overlap.pair_labels.join("；")
                ),
                evidence_count: overlap.evidence_ids.len(),
                evidence_ids: overlap.evidence_ids,
            });
        } else if day.end - day.start > 3 {
            sequential_days += 1;
            let evidence_ids = records[day.start..day.end]
                .iter()
                .map(|record| record.uid)
                .collect::<Vec<_>>();
            alerts.push(AlertSummary {
                kind: "same_day_many".into(),
                severity: "中".into(),
                score: (25 + (((day.end - day.start) - 4) as u32) * 5).min(45),
                title: format!("{} 辖区内非重合入住超过 3 次", day.day),
                detail: format!(
                    "{} 条有效记录未发现入住时间重叠；不足 10 分钟的记录已排除。",
                    day.end - day.start
                ),
                evidence_count: evidence_ids.len(),
                evidence_ids,
            });
        }
    }

    let [week_range, month_range, year_range] = max_window_ranges(&records);
    let use_selected_window = settings.frequency_mode == FrequencyMode::Selected;
    if use_selected_window && records.len() > settings.frequency_threshold {
        alerts.push(frequency_alert(
            "window_frequency",
            "时间窗口",
            &records,
            settings.frequency_threshold,
        ));
    } else if !use_selected_window {
        for (kind, label, window_range, threshold) in [
            (
                "week_frequency",
                "7 天",
                week_range,
                settings.week_threshold,
            ),
            (
                "month_frequency",
                "30 天",
                month_range,
                settings.month_threshold,
            ),
            (
                "year_frequency",
                "365 天",
                year_range,
                settings.year_threshold,
            ),
        ] {
            if window_range.len() > threshold {
                alerts.push(frequency_alert(
                    kind,
                    label,
                    &records[window_range.start..window_range.end],
                    threshold,
                ));
            }
        }
    }

    finish_person_analysis(
        &records,
        alerts,
        [week_range.len(), month_range.len(), year_range.len()],
        overlap_days,
        sequential_days,
    )
}

fn analyze_single_record(record: &Record, settings: &AnalysisSettings) -> PersonAnalysis {
    let records = [record];
    let mut alerts = Vec::new();
    if settings.frequency_mode == FrequencyMode::Selected {
        if settings.frequency_threshold == 0 {
            alerts.push(frequency_alert(
                "window_frequency",
                "时间窗口",
                &records,
                settings.frequency_threshold,
            ));
        }
    } else {
        for (kind, label, threshold) in [
            ("week_frequency", "7 天", settings.week_threshold),
            ("month_frequency", "30 天", settings.month_threshold),
            ("year_frequency", "365 天", settings.year_threshold),
        ] {
            if threshold == 0 {
                alerts.push(frequency_alert(kind, label, &records, threshold));
            }
        }
    }
    finish_person_analysis(&records, alerts, [1, 1, 1], 0, 0)
}

fn finish_person_analysis(
    records: &[&Record],
    alerts: Vec<AlertSummary>,
    window_counts: [usize; 3],
    overlap_days: usize,
    sequential_days: usize,
) -> PersonAnalysis {
    let first = records[0];
    let score = alerts.iter().map(|alert| alert.score).sum::<u32>().min(100);
    let level = level_from_score(score).to_string();
    let summary = PersonSummary {
        person_key: first.person_key.clone(),
        name: fallback(&first.name, "未填").to_string(),
        id_no: first.id_no.clone(),
        phone: first.phone.clone(),
        household_region: first.household_region.clone(),
        household_province: first.household_province.clone(),
        household_city: first.household_city.clone(),
        household_county: first.household_county.clone(),
        age: first.age,
        gender: first.gender.clone(),
        total_records: records.len(),
        max_week_count: window_counts[0],
        max_month_count: window_counts[1],
        max_year_count: window_counts[2],
        overlap_days,
        sequential_days,
        score,
        level,
        alert_count: alerts.len(),
        alert_titles: alerts.iter().map(|alert| alert.title.clone()).collect(),
        hotel_names: unique_hotel_names(records),
        hotel_regions: unique_hotel_regions(records),
    };
    PersonAnalysis { summary, alerts }
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

pub fn within_analysis_time_window(record: &Record, settings: &AnalysisSettings) -> bool {
    let Some(check_in) = record.check_in else {
        return false;
    };
    if settings.frequency_mode != FrequencyMode::Selected {
        return true;
    }
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

fn different_accommodation_cached(
    first: &Record,
    second: &Record,
    cache: &mut HashMap<u64, (String, String)>,
) -> bool {
    cache
        .entry(first.uid)
        .or_insert_with(|| (compact(&first.hotel_name), compact(&first.room_no)));
    cache
        .entry(second.uid)
        .or_insert_with(|| (compact(&second.hotel_name), compact(&second.room_no)));
    // or_insert_with above guarantees both keys are cached; the else branches
    // are defensive only and never execute under the current invariants.
    let Some(first_location) = cache.get(&first.uid) else {
        return false;
    };
    let Some(second_location) = cache.get(&second.uid) else {
        return false;
    };
    (!first_location.0.is_empty()
        && !second_location.0.is_empty()
        && first_location.0 != second_location.0)
        || (!first_location.1.is_empty()
            && !second_location.1.is_empty()
            && first_location.1 != second_location.1)
}

fn day_ranges(records: &[&Record]) -> (Vec<DayAnalysis>, Vec<usize>) {
    let mut days: Vec<DayAnalysis> = Vec::new();
    let mut record_days = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        let Some(check_in) = record.check_in else {
            continue;
        };
        let day = check_in.date();
        if days.last().is_some_and(|current| current.day == day) {
            if let Some(current) = days.last_mut() {
                current.end = index + 1;
            }
        } else {
            days.push(DayAnalysis {
                day,
                start: index,
                end: index + 1,
                overlap: None,
            });
        }
        record_days.push(days.len() - 1);
    }
    (days, record_days)
}

fn max_window_ranges(records: &[&Record]) -> [WindowRange; 3] {
    let mut best = [WindowRange::default(); 3];
    let mut ends = [0; 3];
    for start in 0..records.len() {
        let check_in = records[start].check_in.unwrap_or(NaiveDateTime::MIN);
        for (window_index, days) in ROLLING_WINDOW_DAYS.into_iter().enumerate() {
            let window_end = check_in + Duration::days(days);
            while ends[window_index] < records.len()
                && records[ends[window_index]]
                    .check_in
                    .is_some_and(|value| value <= window_end)
            {
                ends[window_index] += 1;
            }
            if ends[window_index] - start > best[window_index].len() {
                best[window_index] = WindowRange {
                    start,
                    end: ends[window_index],
                };
            }
        }
    }
    best
}

fn unique_hotel_names(records: &[&Record]) -> Vec<String> {
    if records.len() <= HASH_DEDUP_THRESHOLD {
        return records.iter().fold(Vec::new(), |mut names, record| {
            if !record.hotel_name.trim().is_empty() && !names.contains(&record.hotel_name) {
                names.push(record.hotel_name.clone());
            }
            names
        });
    }

    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for record in records {
        if !record.hotel_name.trim().is_empty() && seen.insert(record.hotel_name.as_str()) {
            names.push(record.hotel_name.clone());
        }
    }
    names
}

fn unique_hotel_regions(records: &[&Record]) -> Vec<HotelRegion> {
    if records.len() <= HASH_DEDUP_THRESHOLD {
        return records.iter().fold(Vec::new(), |mut regions, record| {
            let region = hotel_region(record);
            if !empty_hotel_region(&region) && !regions.contains(&region) {
                regions.push(region);
            }
            regions
        });
    }

    let mut regions = Vec::new();
    let mut seen = HashSet::new();
    for record in records {
        if record.province.trim().is_empty()
            && record.city.trim().is_empty()
            && record.county.trim().is_empty()
            && record.region.trim().is_empty()
        {
            continue;
        }
        let key = (
            record.province.as_str(),
            record.city.as_str(),
            record.county.as_str(),
            record.region.as_str(),
        );
        if seen.insert(key) {
            regions.push(hotel_region(record));
        }
    }
    regions
}

fn hotel_region(record: &Record) -> HotelRegion {
    HotelRegion {
        province: record.province.clone(),
        city: record.city.clone(),
        county: record.county.clone(),
        region: record.region.clone(),
    }
}

fn empty_hotel_region(region: &HotelRegion) -> bool {
    region.province.trim().is_empty()
        && region.city.trim().is_empty()
        && region.county.trim().is_empty()
        && region.region.trim().is_empty()
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
    use std::time::Instant;

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
            frequency_mode: FrequencyMode::Selected,
            frequency_start: NaiveDate::from_ymd_opt(2026, 5, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0),
            frequency_end: NaiveDate::from_ymd_opt(2026, 5, 4)
                .unwrap()
                .and_hms_opt(23, 59, 59),
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
            frequency_mode: FrequencyMode::Selected,
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

    #[test]
    fn rolling_frequency_mode_ignores_stale_time_boundaries() {
        let records = vec![
            record(1, "301", "2026-05-01 09:30", "2026-05-01 13:00"),
            record(2, "301", "2026-05-02 09:30", "2026-05-02 13:00"),
            record(3, "301", "2026-05-03 09:30", "2026-05-03 13:00"),
            record(4, "301", "2026-05-04 09:30", "2026-05-04 13:00"),
        ];
        let settings = AnalysisSettings {
            frequency_mode: FrequencyMode::Rolling,
            frequency_start: NaiveDate::from_ymd_opt(2026, 5, 2)
                .unwrap()
                .and_hms_opt(0, 0, 0),
            frequency_end: NaiveDate::from_ymd_opt(2026, 5, 2)
                .unwrap()
                .and_hms_opt(23, 59, 59),
            ..Default::default()
        };
        let analyses = analyze_records(&records, &settings).0;
        assert_eq!(analyses[0].summary.total_records, 4);
        assert!(analyses[0]
            .alerts
            .iter()
            .any(|alert| alert.kind == "week_frequency"));
    }

    #[test]
    fn summary_collects_unique_hotel_regions_and_defaults_missing_regions() {
        let first = record(1, "301", "2026-05-01 09:30", "2026-05-01 13:00");
        let mut second = record(2, "302", "2026-05-02 09:30", "2026-05-02 13:00");
        second.province = "浙江省".into();
        second.city = "杭州市".into();
        second.county = "西湖区".into();
        second.region = "浙江省杭州市西湖区".into();
        let analyses = analyze_records(&[first, second], &AnalysisSettings::default()).0;
        assert_eq!(analyses[0].summary.hotel_regions.len(), 2);

        let mut serialized = serde_json::to_value(&analyses[0].summary).unwrap();
        serialized.as_object_mut().unwrap().remove("hotelRegions");
        let restored: PersonSummary = serde_json::from_value(serialized).unwrap();
        assert!(restored.hotel_regions.is_empty());
    }

    #[test]
    fn large_hotel_collections_preserve_first_seen_order() {
        let base = NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let names = ["甲", "乙", "甲", "丙", "乙", "丁", "戊", "己", "庚", "甲"];
        let records = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let mut item = benchmark_record(
                    index as u64 + 1,
                    1,
                    index,
                    base + Duration::days(index as i64 * 40),
                    base + Duration::days(index as i64 * 40) + Duration::hours(1),
                );
                item.hotel_name = (*name).into();
                item.province = format!("{name}省");
                item.city.clear();
                item.county.clear();
                item.region.clear();
                item
            })
            .collect::<Vec<_>>();
        let summary = &analyze_records(&records, &AnalysisSettings::default()).0[0].summary;
        assert_eq!(
            summary.hotel_names,
            ["甲", "乙", "丙", "丁", "戊", "己", "庚"]
        );
        assert_eq!(
            summary
                .hotel_regions
                .iter()
                .map(|region| region.province.as_str())
                .collect::<Vec<_>>(),
            ["甲省", "乙省", "丙省", "丁省", "戊省", "己省", "庚省"]
        );
    }

    #[test]
    fn parallel_person_sort_remains_deterministic() {
        let base = NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let records = (0..PARALLEL_SORT_THRESHOLD + 4)
            .rev()
            .map(|person_index| {
                let mut item = benchmark_record(
                    person_index as u64 + 1,
                    person_index,
                    0,
                    base,
                    base + Duration::hours(1),
                );
                item.name = "同名".into();
                item
            })
            .collect::<Vec<_>>();
        let analyses = analyze_records(&records, &AnalysisSettings::default()).0;
        assert_eq!(analyses.len(), PARALLEL_SORT_THRESHOLD + 4);
        assert_eq!(analyses[0].summary.person_key, "id:000000000000000000");
        assert_eq!(
            analyses.last().unwrap().summary.person_key,
            format!("id:{:018}", PARALLEL_SORT_THRESHOLD + 3)
        );
    }

    #[test]
    fn analysis_regression_checksum() {
        let records = regression_records();
        let rolling = analyze_records(&records, &AnalysisSettings::default());
        let selected_settings = AnalysisSettings {
            frequency_mode: FrequencyMode::Selected,
            frequency_start: NaiveDate::from_ymd_opt(2026, 5, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0),
            frequency_end: NaiveDate::from_ymd_opt(2026, 5, 31)
                .unwrap()
                .and_hms_opt(23, 59, 59),
            frequency_threshold: 2,
            ..Default::default()
        };
        let selected = analyze_records(&records, &selected_settings);
        let rolling_checksum = checksum_json(&rolling);
        let selected_checksum = checksum_json(&selected);
        println!(
            "analysis_regression rolling_checksum={rolling_checksum} selected_checksum={selected_checksum}"
        );
        assert_eq!(rolling_checksum, 11_531_671_983_614_133_412);
        assert_eq!(selected_checksum, 7_793_499_981_386_381_458);
    }

    #[test]
    #[ignore = "large synthetic analysis performance benchmark"]
    fn benchmark_sparse_analysis_workload() {
        let people_count = benchmark_size("MAIYIN_ANALYSIS_BENCH_PEOPLE", 352_948);
        let records_count =
            benchmark_size("MAIYIN_ANALYSIS_BENCH_RECORDS", 453_506).max(people_count);
        let records = sparse_benchmark_records(people_count, records_count);
        let started = Instant::now();
        let (analyses, stats) = analyze_records(&records, &AnalysisSettings::default());
        let elapsed = started.elapsed();
        println!(
            "analysis_benchmark=sparse records={} people={} analysis_ms={}",
            records.len(),
            analyses.len(),
            elapsed.as_millis()
        );
        assert_eq!(stats.records, records_count);
        assert_eq!(stats.people, people_count);
        assert_eq!(analyses.len(), people_count);
    }

    #[test]
    #[ignore = "dense overlap analysis performance benchmark"]
    fn benchmark_dense_overlap_analysis() {
        let record_count = benchmark_size("MAIYIN_ANALYSIS_BENCH_OVERLAPS", 800);
        let base = NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let records = (0..record_count)
            .map(|index| {
                benchmark_record(
                    index as u64 + 1,
                    0,
                    index,
                    base + Duration::seconds(index as i64),
                    base + Duration::days(2),
                )
            })
            .collect::<Vec<_>>();
        let started = Instant::now();
        let (analyses, stats) = analyze_records(&records, &AnalysisSettings::default());
        let elapsed = started.elapsed();
        println!(
            "analysis_benchmark=dense_overlap records={} pairs={} analysis_ms={}",
            record_count,
            record_count.saturating_mul(record_count.saturating_sub(1)) / 2,
            elapsed.as_millis()
        );
        assert_eq!(stats.people, 1);
        assert_eq!(analyses[0].summary.overlap_days, 1);
        assert_eq!(analyses[0].alerts[0].evidence_count, record_count);
    }

    fn regression_records() -> Vec<Record> {
        let base = NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let mut records = sparse_benchmark_records(12, 18);
        let mut uid = records.len() as u64 + 1;

        for index in 0..5 {
            let mut item = benchmark_record(
                uid,
                100,
                index,
                base + Duration::minutes(index as i64 * 10),
                base + Duration::hours(4),
            );
            item.hotel_name = if index % 2 == 0 {
                "重叠甲旅馆".into()
            } else {
                "重叠乙旅馆".into()
            };
            item.room_no = format!("{}", 301 + index);
            records.push(item);
            uid += 1;
        }

        for index in 0..4 {
            records.push(benchmark_record(
                uid,
                101,
                index,
                base + Duration::hours(index as i64 * 2),
                base + Duration::hours(index as i64 * 2 + 1),
            ));
            uid += 1;
        }

        for index in 0..6 {
            records.push(benchmark_record(
                uid,
                102,
                index,
                base + Duration::days(index as i64 * 6),
                base + Duration::days(index as i64 * 6) + Duration::hours(2),
            ));
            uid += 1;
        }

        let mut missing = benchmark_record(uid, 103, 0, base, base + Duration::hours(1));
        missing.check_in = None;
        missing.check_in_text.clear();
        missing.issues.push("缺少入住时间".into());
        records.push(missing);
        records.reverse();
        records
    }

    fn sparse_benchmark_records(people_count: usize, records_count: usize) -> Vec<Record> {
        let base = NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let mut records = Vec::with_capacity(records_count);
        let mut uid = 1_u64;
        for person_index in 0..people_count {
            let day = (person_index % 365) as i64;
            records.push(benchmark_record(
                uid,
                person_index,
                0,
                base + Duration::days(day),
                base + Duration::days(day) + Duration::hours(2),
            ));
            uid += 1;
        }
        for extra_index in 0..records_count.saturating_sub(people_count) {
            let person_index = extra_index % people_count.max(1);
            let stay_index = extra_index / people_count.max(1) + 1;
            let day = ((person_index + stay_index * 17) % 365) as i64;
            records.push(benchmark_record(
                uid,
                person_index,
                stay_index,
                base + Duration::days(day),
                base + Duration::days(day) + Duration::hours(2),
            ));
            uid += 1;
        }
        records
    }

    fn benchmark_record(
        uid: u64,
        person_index: usize,
        stay_index: usize,
        check_in: NaiveDateTime,
        check_out: NaiveDateTime,
    ) -> Record {
        let id_no = format!("{person_index:018}");
        Record {
            uid,
            source_file: "benchmark.csv".into(),
            source_row: uid as usize,
            name: format!("人员{person_index:09}"),
            id_no: id_no.clone(),
            phone: format!("1{:010}", person_index % 10_000_000_000_usize),
            hotel_name: format!("旅馆{}", stay_index % 5),
            province: if stay_index.is_multiple_of(2) {
                "安徽省".into()
            } else {
                "浙江省".into()
            },
            city: if stay_index.is_multiple_of(2) {
                "黄山市".into()
            } else {
                "杭州市".into()
            },
            county: if stay_index.is_multiple_of(2) {
                "祁门县".into()
            } else {
                "西湖区".into()
            },
            region: if stay_index.is_multiple_of(2) {
                "安徽省黄山市祁门县".into()
            } else {
                "浙江省杭州市西湖区".into()
            },
            address: "测试路 1 号".into(),
            room_no: format!("{}", 300 + stay_index % 20),
            check_in_text: check_in.format("%Y-%m-%d %H:%M:%S").to_string(),
            register_time_text: String::new(),
            check_out_text: check_out.format("%Y-%m-%d %H:%M:%S").to_string(),
            check_in: Some(check_in),
            register_time: None,
            check_out: Some(check_out),
            person_key: format!("id:{id_no}"),
            household_province: "安徽省".into(),
            household_city: "黄山市".into(),
            household_county: "祁门县".into(),
            household_region: "安徽省黄山市祁门县".into(),
            household_address: "户籍地址".into(),
            age: Some((person_index % 80 + 18) as u8),
            gender: if person_index.is_multiple_of(2) {
                "男".into()
            } else {
                "女".into()
            },
            issues: if uid.is_multiple_of(997) {
                vec!["基准问题".into()]
            } else {
                vec![]
            },
        }
    }

    fn checksum_json<T: serde::Serialize>(value: &T) -> u64 {
        serde_json::to_vec(value)
            .unwrap()
            .into_iter()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            })
    }

    fn benchmark_size(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }
}
