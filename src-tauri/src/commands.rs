use crate::analysis::{analyze_records, within_analysis_scope, within_analysis_time_window};
use crate::error::{AppError, CommandError};
use crate::exporter::{export_to, OperationResult};
use crate::importer;
use crate::model::{
    format_datetime, AnalysisSettings, EvidenceRecord, ImportedStayRecord, PersonDetail,
    StoredSession, WorkspaceSnapshot,
};
use crate::storage::SessionStore;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;
use uuid::Uuid;

pub struct AppState {
    inner: Mutex<BackendState>,
}

struct BackendState {
    store: SessionStore,
    current: Option<StoredSession>,
    preference_path: PathBuf,
}

impl AppState {
    pub fn open(app_data_root: PathBuf) -> Result<Self, AppError> {
        fs::create_dir_all(&app_data_root).map_err(|error| AppError::Storage(error.to_string()))?;
        let preference_path = app_data_root.join("storage.json");
        let storage_root = read_storage_preference(&preference_path).unwrap_or(app_data_root);
        let store = SessionStore::open(storage_root)?;
        Ok(Self {
            inner: Mutex::new(BackendState {
                store,
                current: None,
                preference_path,
            }),
        })
    }
}

#[tauri::command]
pub fn bootstrap_workspace(state: State<'_, AppState>) -> Result<WorkspaceSnapshot, CommandError> {
    let backend = lock(&state)?;
    Ok(snapshot(&backend))
}

#[tauri::command]
pub async fn import_paths(
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    let imported = tauri::async_runtime::spawn_blocking(move || importer::import_paths(&paths))
        .await
        .map_err(|error| CommandError {
            code: "task_error",
            message: error.to_string(),
        })??;
    let mut backend = lock(&state)?;
    let settings = backend
        .current
        .as_ref()
        .map(|session| session.settings.clone())
        .unwrap_or_default();
    let (analyses, stats) = analyze_records(&imported.records, &settings);
    let session = StoredSession {
        schema_version: 1,
        session_id: format!(
            "{}-{}",
            Local::now().timestamp_millis(),
            &Uuid::new_v4().simple().to_string()[..8]
        ),
        file_name: imported.title,
        imported_at: Local::now().to_rfc3339(),
        file_count: imported.file_count,
        settings,
        records: imported.records,
        analyses,
        stats,
        import_stats: imported.stats,
        source_session_ids: vec![],
        is_combined: false,
    };
    backend.store.save(&session)?;
    backend.current = Some(session);
    Ok(snapshot(&backend))
}

#[tauri::command]
pub async fn import_folders(
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    let files =
        tauri::async_runtime::spawn_blocking(move || importer::discover_supported_files(&paths))
            .await
            .map_err(|error| CommandError {
                code: "task_error",
                message: error.to_string(),
            })??;
    if files.is_empty() {
        return Err(
            AppError::Empty("所选文件夹及其子目录中没有 .xls、.xlsx 或 .csv 文件".into()).into(),
        );
    }
    import_paths(files, state).await
}

#[tauri::command]
pub fn load_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    let mut backend = lock(&state)?;
    let session = backend.store.load(&session_id, true)?;
    backend.current = Some(session);
    Ok(snapshot(&backend))
}

#[tauri::command]
pub fn merge_sessions(
    session_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    if session_ids.len() < 2 {
        return Err(AppError::Validation("至少选择两条历史数据".into()).into());
    }
    let mut backend = lock(&state)?;
    let settings = backend
        .current
        .as_ref()
        .map(|session| session.settings.clone())
        .unwrap_or_default();
    let mut combined = Vec::new();
    let mut seen = HashSet::new();
    let mut duplicate_count = 0;
    let mut short_stay_count = 0;
    let mut missing_id_count = 0;
    let mut file_count = 0;
    for session_id in &session_ids {
        let session = backend.store.load(session_id, false)?;
        file_count += session.file_count;
        short_stay_count += session.import_stats.short_stay_count;
        missing_id_count += session.import_stats.missing_id_count;
        for mut record in session.records {
            let key = record_key(&record);
            if !seen.insert(key) {
                duplicate_count += 1;
                continue;
            }
            record.uid = combined.len() as u64 + 1;
            combined.push(record);
        }
    }
    let (analyses, stats) = analyze_records(&combined, &settings);
    let imported = combined.len();
    let session = StoredSession {
        schema_version: 1,
        session_id: format!("combined-{}", Local::now().timestamp_millis()),
        file_name: format!("合并分析 · {} 个历史会话", session_ids.len()),
        imported_at: Local::now().to_rfc3339(),
        file_count,
        settings,
        records: combined,
        analyses,
        stats,
        import_stats: crate::model::ImportStats {
            imported,
            duplicate_count,
            short_stay_count,
            missing_id_count,
        },
        source_session_ids: session_ids,
        is_combined: true,
    };
    backend.current = Some(session);
    Ok(snapshot(&backend))
}

#[tauri::command]
pub fn delete_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    let mut backend = lock(&state)?;
    backend.store.delete(&session_id)?;
    let active = backend.store.active_id().map(str::to_string);
    backend.current = active.and_then(|session_id| backend.store.load(&session_id, false).ok());
    Ok(snapshot(&backend))
}

#[tauri::command]
pub fn clear_workspace(state: State<'_, AppState>) -> Result<WorkspaceSnapshot, CommandError> {
    let mut backend = lock(&state)?;
    backend.current = None;
    Ok(snapshot(&backend))
}

#[tauri::command]
pub fn reanalyze(
    settings: AnalysisSettings,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    validate_settings(&settings)?;
    let mut backend = lock(&state)?;
    let should_save = {
        let session = backend.current.as_mut().ok_or(AppError::NoWorkspace)?;
        let (analyses, stats) = analyze_records(&session.records, &settings);
        session.settings = settings;
        session.analyses = analyses;
        session.stats = stats;
        !session.is_combined
    };
    if should_save {
        let session = backend
            .current
            .as_ref()
            .cloned()
            .ok_or(AppError::NoWorkspace)?;
        backend.store.save(&session)?;
    }
    Ok(snapshot(&backend))
}

#[tauri::command]
pub fn get_person_detail(
    person_key: String,
    state: State<'_, AppState>,
) -> Result<PersonDetail, CommandError> {
    let backend = lock(&state)?;
    let session = backend.current.as_ref().ok_or(AppError::NoWorkspace)?;
    let analysis = session
        .analyses
        .iter()
        .find(|item| item.summary.person_key == person_key)
        .ok_or(AppError::Validation("未找到指定人员".into()))?;
    let evidence_ids: HashSet<u64> = analysis
        .alerts
        .iter()
        .flat_map(|alert| alert.evidence_ids.iter().copied())
        .collect();
    let evidence = session
        .records
        .iter()
        .filter(|record| {
            record.person_key == person_key
                && within_analysis_scope(record, &session.settings)
                && within_analysis_time_window(record, &session.settings)
                && (evidence_ids.is_empty() || evidence_ids.contains(&record.uid))
        })
        .map(|record| EvidenceRecord {
            uid: record.uid,
            source_file: record.source_file.clone(),
            source_row: record.source_row,
            hotel_name: record.hotel_name.clone(),
            region: record.region.clone(),
            address: record.address.clone(),
            room_no: record.room_no.clone(),
            check_in: format_datetime(record.check_in),
            check_out: format_datetime(record.check_out),
            issues: record.issues.clone(),
        })
        .collect();
    Ok(PersonDetail {
        person: analysis.summary.clone(),
        alerts: analysis.alerts.clone(),
        evidence,
    })
}

#[tauri::command]
pub fn get_imported_records(
    state: State<'_, AppState>,
) -> Result<Vec<ImportedStayRecord>, CommandError> {
    let backend = lock(&state)?;
    let session = backend.current.as_ref().ok_or(AppError::NoWorkspace)?;
    Ok(session
        .records
        .iter()
        .filter(|record| {
            within_analysis_scope(record, &session.settings)
                && within_analysis_time_window(record, &session.settings)
        })
        .map(imported_stay_record)
        .collect())
}

#[tauri::command]
pub fn export_result(
    kind: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<OperationResult, CommandError> {
    let backend = lock(&state)?;
    let session = backend.current.as_ref().ok_or(AppError::NoWorkspace)?;
    Ok(export_to(&kind, session, &PathBuf::from(path))?)
}

#[tauri::command]
pub fn set_storage_directory(
    path: String,
    state: State<'_, AppState>,
) -> Result<OperationResult, CommandError> {
    let destination = PathBuf::from(path);
    if !destination.exists() {
        return Err(AppError::Validation("所选目录不存在".into()).into());
    }
    let mut backend = lock(&state)?;
    backend.store.move_to(destination.clone())?;
    let preference = StoragePreference {
        storage_root: destination.to_string_lossy().into_owned(),
    };
    fs::write(
        &backend.preference_path,
        serde_json::to_vec_pretty(&preference).map_err(AppError::from)?,
    )
    .map_err(|error| AppError::Storage(error.to_string()))?;
    Ok(OperationResult {
        message: format!("历史数据存放目录已更改为 {}", destination.display()),
        path: Some(destination.to_string_lossy().into_owned()),
    })
}

fn snapshot(backend: &BackendState) -> WorkspaceSnapshot {
    let sessions = backend.store.list();
    if let Some(session) = &backend.current {
        return WorkspaceSnapshot {
            mode: if session.is_combined {
                "combined".into()
            } else {
                "session".into()
            },
            title: session.file_name.clone(),
            subtitle: if session.is_combined {
                "已跨历史去重，并按当前参数重新计算风险".into()
            } else {
                format!(
                    "{} 个文件 · {} 条有效记录",
                    session.file_count, session.stats.records
                )
            },
            stats: session.stats.clone(),
            people: session
                .analyses
                .iter()
                .map(|item| item.summary.clone())
                .collect(),
            sessions,
            settings: session.settings.clone(),
            import_stats: session.import_stats.clone(),
            source_session_ids: if session.is_combined {
                session.source_session_ids.clone()
            } else {
                vec![session.session_id.clone()]
            },
            generated_at: Local::now().to_rfc3339(),
        };
    }
    WorkspaceSnapshot {
        mode: "empty".into(),
        title: "尚未载入数据".into(),
        subtitle: "选择 Excel、CSV 或历史会话开始分析".into(),
        stats: Default::default(),
        people: vec![],
        sessions,
        settings: Default::default(),
        import_stats: Default::default(),
        source_session_ids: vec![],
        generated_at: Local::now().to_rfc3339(),
    }
}

fn validate_settings(settings: &AnalysisSettings) -> Result<(), AppError> {
    for (label, value) in [
        ("时间窗口", settings.frequency_threshold),
        ("7 天", settings.week_threshold),
        ("30 天", settings.month_threshold),
        ("365 天", settings.year_threshold),
    ] {
        if !(1..=99_999).contains(&value) {
            return Err(AppError::Validation(format!(
                "{label}阈值应在 1 到 99999 之间"
            )));
        }
    }
    if settings
        .frequency_start
        .zip(settings.frequency_end)
        .is_some_and(|(start, end)| start > end)
    {
        return Err(AppError::Validation("入住开始时间不能晚于结束时间".into()));
    }
    if settings
        .min_age
        .zip(settings.max_age)
        .is_some_and(|(minimum, maximum)| minimum > maximum)
    {
        return Err(AppError::Validation("最小年龄不能大于最大年龄".into()));
    }
    Ok(())
}

fn imported_stay_record(record: &crate::model::Record) -> ImportedStayRecord {
    ImportedStayRecord {
        uid: record.uid,
        source_file: record.source_file.clone(),
        source_row: record.source_row,
        name: record.name.clone(),
        id_no: record.id_no.clone(),
        phone: record.phone.clone(),
        household_region: record.household_region.clone(),
        hotel_name: record.hotel_name.clone(),
        region: record.region.clone(),
        address: record.address.clone(),
        room_no: record.room_no.clone(),
        check_in: format_datetime(record.check_in),
        register_time: format_datetime(record.register_time),
        check_out: format_datetime(record.check_out),
        issues: record.issues.clone(),
    }
}

fn lock<'a>(
    state: &'a State<'_, AppState>,
) -> Result<std::sync::MutexGuard<'a, BackendState>, CommandError> {
    state.inner.lock().map_err(|_| CommandError {
        code: "state_poisoned",
        message: "本地状态不可用，请重启应用".into(),
    })
}

fn record_key(record: &crate::model::Record) -> String {
    [
        record.id_no.clone(),
        record.hotel_name.clone(),
        record.province.clone(),
        record.city.clone(),
        record.county.clone(),
        record.region.clone(),
        record.address.clone(),
        record.room_no.clone(),
        command_date_key(record.check_in, &record.check_in_text),
        command_date_key(record.check_out, &record.check_out_text),
    ]
    .join("\u{1f}")
}

fn command_date_key(value: Option<chrono::NaiveDateTime>, raw: &str) -> String {
    value
        .map(|item| format!("dt:{}", item.format("%Y-%m-%dT%H:%M:%S")))
        .unwrap_or_else(|| format!("raw:{}", raw.trim()))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoragePreference {
    storage_root: String,
}

fn read_storage_preference(path: &std::path::Path) -> Option<PathBuf> {
    let content = fs::read(path).ok()?;
    let preference: StoragePreference = serde_json::from_slice(&content).ok()?;
    let path = PathBuf::from(preference.storage_root);
    path.exists().then_some(path)
}

#[derive(Serialize)]
#[allow(dead_code)]
struct ProgressEvent<'a> {
    operation_id: &'a str,
    completed: usize,
    total: usize,
    file_name: &'a str,
}
