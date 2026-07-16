use crate::error::AppError;
use crate::model::{SessionSummary, StoredSession};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const DATA_FOLDER: &str = "MaiyinAnalysisData";

#[derive(Debug)]
pub struct SessionStore {
    sessions_dir: PathBuf,
    index_path: PathBuf,
    index: SessionIndex,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionIndex {
    active_session_id: String,
    sessions: Vec<SessionSummary>,
}

impl SessionStore {
    pub fn open(storage_root: PathBuf) -> Result<Self, AppError> {
        let data_dir = storage_root.join(DATA_FOLDER);
        let sessions_dir = data_dir.join("sessions");
        let index_path = data_dir.join("index.json");
        fs::create_dir_all(&sessions_dir).map_err(|error| AppError::Storage(error.to_string()))?;
        let index = if index_path.exists() {
            serde_json::from_slice(
                &fs::read(&index_path).map_err(|error| AppError::Storage(error.to_string()))?,
            )
            .unwrap_or_default()
        } else {
            SessionIndex::default()
        };
        let mut store = Self {
            sessions_dir,
            index_path,
            index,
        };
        store.cleanup_missing()?;
        Ok(store)
    }

    pub fn list(&self) -> Vec<SessionSummary> {
        let mut sessions = self.index.sessions.clone();
        for session in &mut sessions {
            session.active = session.session_id == self.index.active_session_id;
        }
        sessions.sort_by(|left, right| right.imported_at.cmp(&left.imported_at));
        sessions
    }

    pub fn active_id(&self) -> Option<&str> {
        (!self.index.active_session_id.is_empty()).then_some(self.index.active_session_id.as_str())
    }

    pub fn save(&mut self, session: &StoredSession) -> Result<(), AppError> {
        let content = serde_json::to_vec(session)?;
        let temporary = self
            .sessions_dir
            .join(format!("{}.json.tmp", session.session_id));
        let destination = self.session_path(&session.session_id);
        fs::write(&temporary, content).map_err(|error| AppError::Storage(error.to_string()))?;
        if destination.exists() {
            fs::remove_file(&destination).map_err(|error| AppError::Storage(error.to_string()))?;
        }
        fs::rename(&temporary, destination)
            .map_err(|error| AppError::Storage(error.to_string()))?;
        let summary = summary(session, true);
        self.index
            .sessions
            .retain(|item| item.session_id != session.session_id);
        self.index.sessions.insert(0, summary);
        self.index.active_session_id = session.session_id.clone();
        self.write_index()
    }

    pub fn load(&mut self, session_id: &str, activate: bool) -> Result<StoredSession, AppError> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return Err(AppError::SessionNotFound);
        }
        let session: StoredSession = serde_json::from_slice(
            &fs::read(path).map_err(|error| AppError::Storage(error.to_string()))?,
        )?;
        if activate {
            self.index.active_session_id = session_id.to_string();
            self.write_index()?;
        }
        Ok(session)
    }

    pub fn delete(&mut self, session_id: &str) -> Result<(), AppError> {
        let path = self.session_path(session_id);
        if path.exists() {
            fs::remove_file(path).map_err(|error| AppError::Storage(error.to_string()))?;
        }
        self.index
            .sessions
            .retain(|item| item.session_id != session_id);
        if self.index.active_session_id == session_id {
            self.index.active_session_id = self
                .index
                .sessions
                .first()
                .map(|item| item.session_id.clone())
                .unwrap_or_default();
        }
        self.write_index()
    }

    pub fn move_to(&mut self, destination_root: PathBuf) -> Result<(), AppError> {
        let destination_data = destination_root.join(DATA_FOLDER);
        let destination_sessions = destination_data.join("sessions");
        fs::create_dir_all(&destination_sessions)
            .map_err(|error| AppError::Storage(error.to_string()))?;
        for session in &self.index.sessions {
            let source = self.session_path(&session.session_id);
            let destination = destination_sessions.join(format!("{}.json", session.session_id));
            if source.exists() && !destination.exists() {
                fs::copy(source, destination)
                    .map_err(|error| AppError::Storage(error.to_string()))?;
            }
        }
        fs::write(
            destination_data.join("index.json"),
            serde_json::to_vec_pretty(&self.index)?,
        )
        .map_err(|error| AppError::Storage(error.to_string()))?;
        *self = Self::open(destination_root)?;
        Ok(())
    }

    fn cleanup_missing(&mut self) -> Result<(), AppError> {
        let sessions_dir = self.sessions_dir.clone();
        self.index.sessions.retain(|session| {
            sessions_dir
                .join(format!("{}.json", session.session_id))
                .exists()
        });
        if !self
            .index
            .sessions
            .iter()
            .any(|session| session.session_id == self.index.active_session_id)
        {
            self.index.active_session_id = self
                .index
                .sessions
                .first()
                .map(|session| session.session_id.clone())
                .unwrap_or_default();
        }
        self.write_index()
    }

    fn write_index(&self) -> Result<(), AppError> {
        fs::write(&self.index_path, serde_json::to_vec_pretty(&self.index)?)
            .map_err(|error| AppError::Storage(error.to_string()))
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{session_id}.json"))
    }
}

pub fn summary(session: &StoredSession, active: bool) -> SessionSummary {
    SessionSummary {
        session_id: session.session_id.clone(),
        file_name: session.file_name.clone(),
        imported_at: session.imported_at.clone(),
        file_count: session.file_count,
        records: session.stats.records,
        people: session.stats.people,
        duplicate_count: session.import_stats.duplicate_count,
        short_stay_count: session.import_stats.short_stay_count,
        active,
    }
}
