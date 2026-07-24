use crate::error::AppError;
use lz4_flex::block::{
    compress_into_with_table, decompress_size_prepended, get_maximum_output_size, CompressTable,
};
use rusqlite::types::Value;
use std::cell::RefCell;
pub(crate) const COMPRESSED_JSON_MAGIC: &[u8; 4] = b"MYL4";

pub(crate) struct JsonCompressor {
    table: CompressTable,
    json: Vec<u8>,
}

thread_local! {
    pub(crate) static JSON_COMPRESSOR: RefCell<JsonCompressor> = RefCell::new(JsonCompressor {
        table: CompressTable::default(),
        json: Vec::new(),
    });
}

pub(crate) fn compressed_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, AppError> {
    JSON_COMPRESSOR.with(|compressor| {
        let mut compressor = compressor.borrow_mut();
        let JsonCompressor { table, json } = &mut *compressor;
        json.clear();
        serde_json::to_writer(&mut *json, value).map_err(AppError::from)?;
        let json_len = u32::try_from(json.len())
            .map_err(|_| AppError::Storage("JSON payload exceeds the LZ4 block limit".into()))?;
        let maximum_compressed_size = get_maximum_output_size(json.len());
        let mut stored =
            Vec::with_capacity(COMPRESSED_JSON_MAGIC.len() + 4 + maximum_compressed_size);
        stored.extend_from_slice(COMPRESSED_JSON_MAGIC);
        stored.extend_from_slice(&json_len.to_le_bytes());
        stored.resize(COMPRESSED_JSON_MAGIC.len() + 4 + maximum_compressed_size, 0);
        let compressed_len = compress_into_with_table(json, &mut stored[8..], table)
            .map_err(|error| AppError::Storage(format!("JSON compression failed: {error}")))?;
        stored.truncate(8 + compressed_len);
        Ok(stored)
    })
}

pub(crate) fn from_stored_json<T: serde::de::DeserializeOwned>(
    value: Value,
) -> Result<T, AppError> {
    match value {
        Value::Text(text) => from_json(&text),
        Value::Blob(blob) if blob.starts_with(COMPRESSED_JSON_MAGIC) => {
            let json = decompress_size_prepended(&blob[COMPRESSED_JSON_MAGIC.len()..]).map_err(
                |error| AppError::Storage(format!("JSON decompression failed: {error}")),
            )?;
            serde_json::from_slice(&json).map_err(AppError::from)
        }
        Value::Blob(blob) => serde_json::from_slice(&blob).map_err(AppError::from),
        _ => Err(AppError::Storage(
            "Stored JSON payload is neither TEXT nor BLOB".into(),
        )),
    }
}

pub(crate) fn json<T: serde::Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(AppError::from)
}

pub(crate) fn from_json<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, AppError> {
    serde_json::from_str(value).map_err(AppError::from)
}
