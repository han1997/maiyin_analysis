use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnalysisSettings {
    pub province: String,
    pub city: String,
    pub county: String,
    pub household_province: String,
    pub household_city: String,
    pub household_county: String,
    pub exclude_household_province: String,
    pub exclude_household_city: String,
    pub exclude_household_county: String,
    pub min_age: Option<u8>,
    pub max_age: Option<u8>,
    pub gender: String,
    pub month_threshold: usize,
    pub year_threshold: usize,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            province: String::new(),
            city: String::new(),
            county: String::new(),
            household_province: String::new(),
            household_city: String::new(),
            household_county: String::new(),
            exclude_household_province: String::new(),
            exclude_household_city: String::new(),
            exclude_household_county: String::new(),
            min_age: None,
            max_age: None,
            gender: String::new(),
            month_threshold: 6,
            year_threshold: 24,
        }
    }
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
    pub max_month_count: usize,
    pub max_year_count: usize,
    pub overlap_days: usize,
    pub sequential_days: usize,
    pub score: u32,
    pub level: String,
    pub alert_count: usize,
    pub alert_titles: Vec<String>,
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
    pub people: Vec<PersonSummary>,
    pub sessions: Vec<SessionSummary>,
    pub settings: AnalysisSettings,
    pub import_stats: ImportStats,
    pub source_session_ids: Vec<String>,
    pub generated_at: String,
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
