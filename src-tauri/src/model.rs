use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnalysisSettings {
    pub frequency_start: Option<NaiveDateTime>,
    pub frequency_end: Option<NaiveDateTime>,
    pub frequency_threshold: usize,
    pub week_threshold: usize,
    pub month_threshold: usize,
    pub year_threshold: usize,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            frequency_start: None,
            frequency_end: None,
            frequency_threshold: 3,
            week_threshold: 3,
            month_threshold: 12,
            year_threshold: 144,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotelRegion {
    pub province: String,
    pub city: String,
    pub county: String,
    pub region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub uid: u64,
    pub source_file: String,
    pub source_row: usize,
    pub name: String,
    pub id_no: String,
    pub phone: String,
    pub hotel_name: String,
    pub province: String,
    pub city: String,
    pub county: String,
    pub region: String,
    pub address: String,
    pub room_no: String,
    pub check_in_text: String,
    pub register_time_text: String,
    pub check_out_text: String,
    pub check_in: Option<NaiveDateTime>,
    pub register_time: Option<NaiveDateTime>,
    pub check_out: Option<NaiveDateTime>,
    pub person_key: String,
    pub household_province: String,
    pub household_city: String,
    pub household_county: String,
    pub household_region: String,
    pub household_address: String,
    pub age: Option<u8>,
    pub gender: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertSummary {
    pub kind: String,
    pub severity: String,
    pub score: u32,
    pub title: String,
    pub detail: String,
    pub evidence_count: usize,
    #[serde(default)]
    pub evidence_ids: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonSummary {
    pub person_key: String,
    pub name: String,
    pub id_no: String,
    pub phone: String,
    pub household_region: String,
    pub age: Option<u8>,
    pub gender: String,
    pub total_records: usize,
    #[serde(default)]
    pub max_week_count: usize,
    pub max_month_count: usize,
    pub max_year_count: usize,
    pub overlap_days: usize,
    pub sequential_days: usize,
    pub score: u32,
    pub level: String,
    pub alert_count: usize,
    pub alert_titles: Vec<String>,
    #[serde(default)]
    pub hotel_names: Vec<String>,
    #[serde(default)]
    pub hotel_regions: Vec<HotelRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonAnalysis {
    pub summary: PersonSummary,
    pub alerts: Vec<AlertSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRecord {
    pub uid: u64,
    pub source_file: String,
    pub source_row: usize,
    pub hotel_name: String,
    pub region: String,
    pub address: String,
    pub room_no: String,
    pub check_in: String,
    pub check_out: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedStayRecord {
    pub uid: u64,
    pub source_file: String,
    pub source_row: usize,
    pub name: String,
    pub id_no: String,
    pub phone: String,
    pub household_region: String,
    pub hotel_name: String,
    pub region: String,
    pub address: String,
    pub room_no: String,
    pub check_in: String,
    pub register_time: String,
    pub check_out: String,
    pub issues: Vec<String>,
}

impl From<Record> for ImportedStayRecord {
    fn from(record: Record) -> Self {
        Self {
            uid: record.uid,
            source_file: record.source_file,
            source_row: record.source_row,
            name: record.name,
            id_no: record.id_no,
            phone: record.phone,
            household_region: record.household_region,
            hotel_name: record.hotel_name,
            region: record.region,
            address: record.address,
            room_no: record.room_no,
            check_in: format_datetime(record.check_in),
            register_time: format_datetime(record.register_time),
            check_out: format_datetime(record.check_out),
            issues: record.issues,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportedRecordsQuery {
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedRecordsPage {
    pub items: Vec<ImportedStayRecord>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonDetail {
    pub person: PersonSummary,
    pub alerts: Vec<AlertSummary>,
    pub evidence: Vec<EvidenceRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisStats {
    pub records: usize,
    pub people: usize,
    pub alerted: usize,
    pub high: usize,
    pub issues: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStats {
    pub imported: usize,
    pub duplicate_count: usize,
    pub short_stay_count: usize,
    pub missing_id_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub session_id: String,
    pub file_name: String,
    pub imported_at: String,
    pub file_count: usize,
    pub records: usize,
    pub people: usize,
    pub duplicate_count: usize,
    pub short_stay_count: usize,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub mode: String,
    pub title: String,
    pub subtitle: String,
    pub stats: AnalysisStats,
    pub sessions: Vec<SessionSummary>,
    pub settings: AnalysisSettings,
    pub import_stats: ImportStats,
    pub source_session_ids: Vec<String>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersonQuery {
    pub search: String,
    pub hotel_search: String,
    pub hotel_province: String,
    pub hotel_city: String,
    pub hotel_county: String,
    pub household_province: String,
    pub household_city: String,
    pub household_county: String,
    pub exclude_household_province: String,
    pub exclude_household_city: String,
    pub exclude_household_county: String,
    pub min_age: Option<usize>,
    pub max_age: Option<usize>,
    pub gender: String,
    pub level: String,
    pub alert_state: String,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonPage {
    pub items: Vec<PersonSummary>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSession {
    pub schema_version: u32,
    pub session_id: String,
    pub file_name: String,
    pub imported_at: String,
    pub file_count: usize,
    pub settings: AnalysisSettings,
    pub records: Vec<Record>,
    pub analyses: Vec<PersonAnalysis>,
    pub stats: AnalysisStats,
    pub import_stats: ImportStats,
    pub source_session_ids: Vec<String>,
    pub is_combined: bool,
}

pub fn format_datetime(value: Option<NaiveDateTime>) -> String {
    value
        .map(|item| item.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

pub fn calculate_age(birth: Option<NaiveDate>, today: NaiveDate) -> Option<u8> {
    let birth = birth?;
    let mut age = today.year() - birth.year();
    if (today.month(), today.day()) < (birth.month(), birth.day()) {
        age -= 1;
    }
    (0..=130).contains(&age).then_some(age as u8)
}

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_analysis_filter_fields_are_ignored_during_deserialization() {
        let settings: AnalysisSettings = serde_json::from_value(serde_json::json!({
            "province": "安徽省",
            "householdCounty": "祁门县",
            "minAge": 18,
            "gender": "男",
            "weekThreshold": 5
        }))
        .unwrap();
        assert_eq!(settings.week_threshold, 5);
        assert_eq!(settings.month_threshold, 12);
        assert!(settings.frequency_start.is_none());
    }
}
