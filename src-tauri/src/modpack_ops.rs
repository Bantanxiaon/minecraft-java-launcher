//! 整合包导入操作元数据：崩溃恢复（继续/回滚/安全清理）的可审计基础。

use crate::{launcher_data_directory, LauncherError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationMetadata {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub instance_id: Option<i64>,
    pub file_count: u64,
    pub bytes: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub pack_name: Option<String>,
    #[serde(default)]
    pub pack_version: Option<String>,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub loader_type: Option<String>,
    #[serde(default)]
    pub total_files: Option<u64>,
    #[serde(default)]
    pub completed_files: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

fn staging_root() -> Result<PathBuf, LauncherError> {
    Ok(launcher_data_directory()?
        .join("instances")
        .join(".staging"))
}

pub fn validate_operation_id(id: &str) -> Result<(), LauncherError> {
    if id.is_empty()
        || id.len() > 128
        || id.contains("..")
        || id.contains('\\')
        || id.contains('/')
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err(LauncherError::validation("操作 ID 不安全。"));
    }
    Ok(())
}

/// 每个操作独占的 staging 目录：文件镜像、overrides 解压与 operation.json 元数据。
pub fn operation_staging_directory(id: &str) -> Result<PathBuf, LauncherError> {
    validate_operation_id(id)?;
    Ok(staging_root()?.join(id))
}

pub fn operation_files_directory(id: &str) -> Result<PathBuf, LauncherError> {
    Ok(operation_staging_directory(id)?.join("files"))
}

pub fn operation_overrides_directory(id: &str) -> Result<PathBuf, LauncherError> {
    Ok(operation_staging_directory(id)?.join("overrides"))
}

pub fn write_operation_metadata(metadata: &OperationMetadata) -> Result<(), LauncherError> {
    let directory = operation_staging_directory(&metadata.id)?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(metadata)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    std::fs::write(directory.join("operation.json"), bytes)
        .map_err(|error| LauncherError::storage(error.to_string()))
}

pub fn read_operation_metadata(id: &str) -> Result<Option<OperationMetadata>, LauncherError> {
    validate_operation_id(id)?;
    let path = operation_staging_directory(id)?.join("operation.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_slice(&bytes).map_err(|error| {
        LauncherError::storage(error.to_string())
    })?))
}

/// 更新既有操作的状态（崩溃恢复用），保留其余字段。
pub fn mark_operation_state(
    id: &str,
    state: &str,
    instance_id: Option<i64>,
    error: Option<String>,
) -> Result<(), LauncherError> {
    let mut metadata = read_operation_metadata(id)?
        .ok_or_else(|| LauncherError::validation("找不到这个操作，无法更新状态。"))?;
    metadata.state = state.into();
    if instance_id.is_some() {
        metadata.instance_id = instance_id;
    }
    metadata.error = error;
    metadata.updated_at = crate::chrono_like_timestamp();
    write_operation_metadata(&metadata)
}

#[tauri::command]
pub fn list_operations() -> Result<Vec<OperationMetadata>, LauncherError> {
    let root = staging_root()?;
    let mut operations = Vec::new();
    if !root.is_dir() {
        return Ok(operations);
    }
    for entry in std::fs::read_dir(&root)
        .map_err(|error| LauncherError::storage(error.to_string()))?
        .flatten()
    {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if let Ok(Some(metadata)) = read_operation_metadata(&name) {
            operations.push(metadata);
        }
    }
    operations.sort_by_key(|operation| std::cmp::Reverse(operation.updated_at.clone()));
    Ok(operations)
}

#[tauri::command]
pub fn cleanup_operation(id: String) -> Result<u64, LauncherError> {
    let id = id.as_str();
    validate_operation_id(id)?;
    let directory = operation_staging_directory(id)?;
    let bytes = crate::storage::directory_size(&directory);
    if directory.is_dir() {
        std::fs::remove_dir_all(&directory)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrono_like_timestamp;

    #[test]
    fn operation_metadata_roundtrips_and_cleanup() {
        let id = format!("op-test-{}", crate::unique_timestamp());
        write_operation_metadata(&OperationMetadata {
            id: id.clone(),
            kind: "modrinth".into(),
            state: "running".into(),
            instance_id: Some(1),
            file_count: 3,
            bytes: 42,
            error: None,
            source_path: Some("pack.mrpack".into()),
            pack_name: Some("Test Pack".into()),
            pack_version: None,
            game_version: Some("1.20.1".into()),
            loader_type: Some("forge".into()),
            total_files: Some(3),
            completed_files: Some(1),
            created_at: chrono_like_timestamp(),
            updated_at: chrono_like_timestamp(),
        })
        .unwrap();
        let loaded = read_operation_metadata(&id).unwrap().unwrap();
        assert_eq!(loaded.file_count, 3);
        assert!(list_operations().unwrap().iter().any(|op| op.id == id));
        assert!(cleanup_operation(id.clone()).unwrap() > 0);
        assert!(read_operation_metadata(&id).unwrap().is_none());
    }
}
