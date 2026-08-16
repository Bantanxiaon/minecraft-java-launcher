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
    pub created_at: String,
    pub updated_at: String,
}

fn staging_root() -> Result<PathBuf, LauncherError> {
    Ok(launcher_data_directory()?
        .join("instances")
        .join(".staging"))
}

pub fn write_operation_metadata(metadata: &OperationMetadata) -> Result<(), LauncherError> {
    let directory = staging_root()?.join(&metadata.id);
    std::fs::create_dir_all(&directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(metadata)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    std::fs::write(directory.join("operation.json"), bytes)
        .map_err(|error| LauncherError::storage(error.to_string()))
}

pub fn read_operation_metadata(id: &str) -> Result<Option<OperationMetadata>, LauncherError> {
    if id.contains("..") || id.contains('\\') || id.contains('/') {
        return Err(LauncherError::validation("操作 ID 不安全。"));
    }
    let path = staging_root()?.join(id).join("operation.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_slice(&bytes).map_err(|error| {
        LauncherError::storage(error.to_string())
    })?))
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
    if id.contains("..") || id.contains('\\') || id.contains('/') {
        return Err(LauncherError::validation("操作 ID 不安全。"));
    }
    let directory = staging_root()?.join(id);
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
