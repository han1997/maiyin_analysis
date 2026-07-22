use crate::analysis::analyze_records;
use crate::error::{AppError, CommandError};
use crate::exporter::{export_to, OperationResult};
use crate::importer;
use crate::model::{
    AnalysisSettings, FrequencyMode, ImportedRecordsPage, ImportedRecordsQuery, PersonDetail,
    PersonPage, PersonQuery, StoredSession, WorkspaceSnapshot, CURRENT_SCHEMA_VERSION,
};
use crate::storage::{SessionMetadata, SessionStore};
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
    current: Option<SessionMetadata>,
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
    Ok(snapshot(&backend)?)
}

#[tauri::command]
pub async fn import_paths(
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    let (store, settings) = {
        let backend = lock(&state)?;
        (
            backend.store.clone(),
            backend
                .current
                .as_ref()
                .map(|session| session.settings.clone())
                .unwrap_or_default(),
        )
    };
    let metadata = tauri::async_runtime::spawn_blocking(move || {
        let imported = importer::import_paths(&paths)?;
        let (analyses, stats) = analyze_records(&imported.records, &settings);
        let session = StoredSession {
            schema_version: CURRENT_SCHEMA_VERSION,
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
        store.save(&session)
    })
    .await
    .map_err(task_error)??;
    let mut backend = lock(&state)?;
    backend.current = Some(metadata);
    Ok(snapshot(&backend)?)
}

#[tauri::command]
pub async fn import_folders(
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    let files =
        tauri::async_runtime::spawn_blocking(move || importer::discover_supported_files(&paths))
            .await
            .map_err(task_error)??;
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
    let store = { lock(&state)?.store.clone() };
    let metadata = store.activate(&session_id)?;
    let mut backend = lock(&state)?;
    backend.current = Some(metadata);
    Ok(snapshot(&backend)?)
}

#[tauri::command]
pub async fn merge_sessions(
    session_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    if session_ids.len() < 2 {
        return Err(AppError::Validation("至少选择两条历史数据".into()).into());
    }
    let (store, settings) = {
        let backend = lock(&state)?;
        (
            backend.store.clone(),
            backend
                .current
                .as_ref()
                .map(|session| session.settings.clone())
                .unwrap_or_default(),
        )
    };
    let metadata = tauri::async_runtime::spawn_blocking(move || {
        let mut combined = Vec::new();
        let mut seen = HashSet::new();
        let mut duplicate_count = 0;
        let mut short_stay_count = 0;
        let mut missing_id_count = 0;
        let mut file_count = 0;
        for session_id in &session_ids {
            let session = store.load(session_id)?;
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
            schema_version: CURRENT_SCHEMA_VERSION,
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
        store.save(&session)
    })
    .await
    .map_err(task_error)??;
    let mut backend = lock(&state)?;
    backend.current = Some(metadata);
    Ok(snapshot(&backend)?)
}

#[tauri::command]
pub fn delete_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    let store = { lock(&state)?.store.clone() };
    let current = store.delete(&session_id)?;
    let mut backend = lock(&state)?;
    backend.current = current;
    Ok(snapshot(&backend)?)
}

#[tauri::command]
pub fn clear_workspace(state: State<'_, AppState>) -> Result<WorkspaceSnapshot, CommandError> {
    let mut backend = lock(&state)?;
    backend.current = None;
    Ok(snapshot(&backend)?)
}

#[tauri::command]
pub async fn reanalyze(
    settings: AnalysisSettings,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, CommandError> {
    validate_settings(&settings)?;
    let (store, session_id) = current_store(&state)?;
    let metadata = tauri::async_runtime::spawn_blocking(move || {
        let mut session = store.load(&session_id)?;
        let (analyses, stats) = analyze_records(&session.records, &settings);
        session.settings = settings;
        session.analyses = analyses;
        session.stats = stats;
        session.schema_version = CURRENT_SCHEMA_VERSION;
        store.save(&session)
    })
    .await
    .map_err(task_error)??;
    let mut backend = lock(&state)?;
    backend.current = Some(metadata);
    Ok(snapshot(&backend)?)
}

#[tauri::command]
pub async fn query_people(
    query: PersonQuery,
    state: State<'_, AppState>,
) -> Result<PersonPage, CommandError> {
    let (store, session_id) = current_store(&state)?;
    let page =
        tauri::async_runtime::spawn_blocking(move || store.query_people(&session_id, &query))
            .await
            .map_err(task_error)??;
    Ok(page)
}

#[tauri::command]
pub async fn get_person_detail(
    person_key: String,
    state: State<'_, AppState>,
) -> Result<PersonDetail, CommandError> {
    let (store, session_id) = current_store(&state)?;
    let detail =
        tauri::async_runtime::spawn_blocking(move || store.person_detail(&session_id, &person_key))
            .await
            .map_err(task_error)??;
    Ok(detail)
}

#[tauri::command]
pub async fn get_imported_records(
    query: ImportedRecordsQuery,
    state: State<'_, AppState>,
) -> Result<ImportedRecordsPage, CommandError> {
    let (store, session_id) = current_store(&state)?;
    let page = tauri::async_runtime::spawn_blocking(move || {
        store.query_imported_records(&session_id, &query)
    })
    .await
    .map_err(task_error)??;
    Ok(page)
}

#[tauri::command]
pub async fn export_result(
    kind: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<OperationResult, CommandError> {
    let (store, session_id) = current_store(&state)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let session = store.load(&session_id)?;
        export_to(&kind, &session, &PathBuf::from(path))
    })
    .await
    .map_err(task_error)??;
    Ok(result)
}

#[tauri::command]
pub async fn set_storage_directory(
    path: String,
    state: State<'_, AppState>,
) -> Result<OperationResult, CommandError> {
    let destination = PathBuf::from(path);
    if !destination.exists() {
        return Err(AppError::Validation("所选目录不存在".into()).into());
    }
    let (store, preference_path) = {
        let backend = lock(&state)?;
        (backend.store.clone(), backend.preference_path.clone())
    };
    let result_path = destination.clone();
    let next_store = tauri::async_runtime::spawn_blocking(move || {
        let next_store = store.move_to(destination.clone())?;
        let preference = StoragePreference {
            storage_root: destination.to_string_lossy().into_owned(),
        };
        fs::write(
            preference_path,
            serde_json::to_vec_pretty(&preference).map_err(AppError::from)?,
        )
        .map_err(|error| AppError::Storage(error.to_string()))?;
        Ok::<_, AppError>(next_store)
    })
    .await
    .map_err(task_error)??;
    let mut backend = lock(&state)?;
    backend.store = next_store;
    Ok(OperationResult {
        message: format!("历史数据存放目录已更改为 {}", result_path.display()),
        path: Some(result_path.to_string_lossy().into_owned()),
    })
}

fn snapshot(backend: &BackendState) -> Result<WorkspaceSnapshot, AppError> {
    let sessions = backend.store.list()?;
    if let Some(session) = &backend.current {
        return Ok(WorkspaceSnapshot {
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
            sessions,
            settings: session.settings.clone(),
            import_stats: session.import_stats.clone(),
            source_session_ids: if session.is_combined {
                session.source_session_ids.clone()
            } else {
                vec![session.session_id.clone()]
            },
            generated_at: Local::now().to_rfc3339(),
        });
    }
    Ok(WorkspaceSnapshot {
        mode: "empty".into(),
        title: "尚未载入数据".into(),
        subtitle: "选择 Excel、CSV 或历史会话开始分析".into(),
        stats: Default::default(),
        sessions,
        settings: Default::default(),
        import_stats: Default::default(),
        source_session_ids: vec![],
        generated_at: Local::now().to_rfc3339(),
    })
}

fn current_store(state: &State<'_, AppState>) -> Result<(SessionStore, String), CommandError> {
    let backend = lock(state)?;
    let session_id = backend
        .current
        .as_ref()
        .map(|session| session.session_id.clone())
        .ok_or(AppError::NoWorkspace)?;
    Ok((backend.store.clone(), session_id))
}

fn validate_settings(settings: &AnalysisSettings) -> Result<(), AppError> {
    let thresholds = if settings.frequency_mode == FrequencyMode::Selected {
        vec![("时间窗口", settings.frequency_threshold)]
    } else {
        vec![
            ("7 天", settings.week_threshold),
            ("30 天", settings.month_threshold),
            ("365 天", settings.year_threshold),
        ]
    };
    for (label, value) in thresholds {
        if !(1..=99_999).contains(&value) {
            return Err(AppError::Validation(format!(
                "{label}阈值应在 1 到 99999 之间"
            )));
        }
    }
    if settings.frequency_mode == FrequencyMode::Selected
        && (settings.frequency_start.is_none() || settings.frequency_end.is_none())
    {
        return Err(AppError::Validation(
            "选定入住时间范围时，开始时间和结束时间均为必填".into(),
        ));
    }
    if settings.frequency_mode == FrequencyMode::Selected
        && settings
            .frequency_start
            .zip(settings.frequency_end)
            .is_some_and(|(start, end)| start > end)
    {
        return Err(AppError::Validation("入住开始时间不能晚于结束时间".into()));
    }
    Ok(())
}

fn lock<'a>(
    state: &'a State<'_, AppState>,
) -> Result<std::sync::MutexGuard<'a, BackendState>, CommandError> {
    state.inner.lock().map_err(|_| CommandError {
        code: "state_poisoned",
        message: "本地状态不可用，请重启应用".into(),
    })
}

fn task_error(error: impl std::fmt::Display) -> CommandError {
    CommandError {
        code: "task_error",
        message: error.to_string(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_analysis_thresholds_and_time_order() {
        let mut inactive_threshold = AnalysisSettings {
            frequency_threshold: 0,
            ..Default::default()
        };
        assert!(validate_settings(&inactive_threshold).is_ok());

        inactive_threshold.frequency_start = chrono::NaiveDate::from_ymd_opt(2026, 7, 2)
            .unwrap()
            .and_hms_opt(0, 0, 0);
        inactive_threshold.frequency_end = chrono::NaiveDate::from_ymd_opt(2026, 7, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0);
        assert!(validate_settings(&inactive_threshold).is_ok());

        let mut settings = AnalysisSettings {
            week_threshold: 0,
            ..Default::default()
        };
        assert!(validate_settings(&settings).is_err());

        settings.week_threshold = 3;
        settings.frequency_mode = FrequencyMode::Selected;
        assert!(validate_settings(&settings).is_err());

        settings.frequency_start = chrono::NaiveDate::from_ymd_opt(2026, 7, 2)
            .unwrap()
            .and_hms_opt(0, 0, 0);
        settings.frequency_end = chrono::NaiveDate::from_ymd_opt(2026, 7, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0);
        assert!(validate_settings(&settings).is_err());

        settings.frequency_start = chrono::NaiveDate::from_ymd_opt(2026, 7, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0);
        assert!(validate_settings(&settings).is_ok());

        inactive_threshold.frequency_mode = FrequencyMode::Selected;
        inactive_threshold.frequency_start = settings.frequency_start;
        inactive_threshold.frequency_end = settings.frequency_end;
        inactive_threshold.frequency_threshold = 3;
        inactive_threshold.week_threshold = 0;
        assert!(validate_settings(&inactive_threshold).is_ok());
    }
}
