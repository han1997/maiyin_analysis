use serde::Serialize;
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("无法读取文件：{0}")]
    Read(String),
    #[error("无法解析数据：{0}")]
    Parse(String),
    #[error("没有可分析的有效记录：{0}")]
    Empty(String),
    #[error("本地历史操作失败：{0}")]
    Storage(String),
    #[error("导出失败：{0}")]
    Export(String),
    #[error("请求参数无效：{0}")]
    Validation(String),
    #[error("当前工作区没有数据")]
    NoWorkspace,
    #[error("未找到指定历史会话")]
    SessionNotFound,
}

impl From<io::Error> for AppError {
    fn from(value: io::Error) -> Self {
        Self::Read(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Storage(value.to_string())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl From<AppError> for CommandError {
    fn from(value: AppError) -> Self {
        let code = match value {
            AppError::Read(_) => "read_error",
            AppError::Parse(_) => "parse_error",
            AppError::Empty(_) => "empty_import",
            AppError::Storage(_) => "storage_error",
            AppError::Export(_) => "export_error",
            AppError::Validation(_) => "validation_error",
            AppError::NoWorkspace => "no_workspace",
            AppError::SessionNotFound => "session_not_found",
        };
        Self {
            code,
            message: value.to_string(),
        }
    }
}
