//! 存储管理：扫描磁盘占用、安全清理、已删除实例的恢复与永久删除。
//! 所有破坏性操作都基于“预览 → 二次校验 → 执行”，并用 fingerprint 防 TOCTOU。

use crate::{
    chrono_like_timestamp, fs_safe, launcher_data_directory, open_database, LauncherError,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageCategory {
    Instance,
    DownloadCache,
    PartialDownload,
    JavaRuntime,
    JavaArchive,
    LoaderInstaller,
    Log,
    CrashReport,
    WorldBackup,
    RemovedContentBackup,
    DeletedInstance,
    Temporary,
    CorruptBackup,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeleteSafety {
    Safe,
    Recoverable,
    Destructive,
    InUse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageItem {
    pub id: String,
    pub category: StorageCategory,
    pub label: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub safety: DeleteSafety,
    pub last_modified_at: Option<i64>,
    pub in_use_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCategorySummary {
    pub category: StorageCategory,
    pub bytes: u64,
    pub item_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageOverview {
    pub total_bytes: u64,
    pub reclaimable_bytes: u64,
    pub categories: Vec<StorageCategorySummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub id: String,
    pub fingerprint: String,
    pub generated_at: i64,
    pub reclaimable_bytes: u64,
    pub destructive_count: usize,
    pub items: Vec<StorageItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub freed_bytes: u64,
    pub removed_items: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedInstance {
    pub id: String,
    pub original_instance_id: i64,
    pub display_name: String,
    pub backup_path: String,
    pub size_bytes: Option<i64>,
    pub deleted_at: String,
    pub game_version: Option<String>,
    pub loader_type: Option<String>,
}

pub(crate) fn directory_size(path: &Path) -> u64 {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_symlink() {
            continue;
        }
        if entry.file_type().is_file() {
            total = total.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
        }
    }
    total
}

fn classify(root: &Path, path: &Path) -> StorageCategory {
    let Ok(relative) = path.strip_prefix(root) else {
        return StorageCategory::Temporary;
    };
    let text = relative.to_string_lossy().replace('\\', "/");
    if text.starts_with("instances/") {
        StorageCategory::Instance
    } else if text.starts_with("runtimes/") {
        StorageCategory::JavaRuntime
    } else if text.starts_with("backups/deleted-instances/") {
        StorageCategory::DeletedInstance
    } else if text.starts_with("backups/") {
        StorageCategory::RemovedContentBackup
    } else if text.starts_with("logs/") {
        StorageCategory::Log
    } else if text.ends_with(".part") || text.contains("/.staging/") || text.ends_with(".tmp") {
        StorageCategory::PartialDownload
    } else if text.starts_with("cache/") {
        StorageCategory::DownloadCache
    } else if text.starts_with("java-archive") || text.contains("jdk") {
        StorageCategory::JavaArchive
    } else {
        StorageCategory::Temporary
    }
}

fn category_safety(category: &StorageCategory) -> DeleteSafety {
    match category {
        StorageCategory::DownloadCache
        | StorageCategory::JavaArchive
        | StorageCategory::LoaderInstaller
        | StorageCategory::Log
        | StorageCategory::Temporary => DeleteSafety::Safe,
        StorageCategory::PartialDownload => DeleteSafety::Safe,
        StorageCategory::CrashReport | StorageCategory::WorldBackup => DeleteSafety::Recoverable,
        StorageCategory::RemovedContentBackup => DeleteSafety::Recoverable,
        StorageCategory::DeletedInstance => DeleteSafety::Destructive,
        StorageCategory::Instance | StorageCategory::JavaRuntime => DeleteSafety::InUse,
        StorageCategory::CorruptBackup => DeleteSafety::Safe,
    }
}

fn scan_items(root: &Path) -> Vec<StorageItem> {
    let mut items = Vec::new();
    let dirs = [
        root.join("instances"),
        root.join("cache"),
        root.join("runtimes"),
        root.join("logs"),
        root.join("backups"),
        root.join("tmp"),
    ];
    let mut seen_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    for directory in dirs {
        if !directory.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&directory)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_symlink() {
                continue;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().to_path_buf();
            let category = classify(root, &path);
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let last_modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            items.push(StorageItem {
                id: md5_bytes(path.to_string_lossy().as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                label: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string()),
                path: path.clone(),
                bytes,
                safety: category_safety(&category),
                category,
                last_modified_at: last_modified,
                in_use_by: Vec::new(),
            });
            if let Some(parent) = path.parent() {
                seen_dirs.insert(parent.to_path_buf());
            }
        }
    }
    let _ = seen_dirs;
    items
}

fn md5_bytes(bytes: &[u8]) -> [u8; 16] {
    use md5::{Digest, Md5};
    let digest = Md5::digest(bytes);
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

fn aggregate(items: &[StorageItem]) -> Vec<StorageCategorySummary> {
    let mut summaries: std::collections::BTreeMap<String, (u64, usize)> = Default::default();
    for item in items {
        let key = format!("{:?}", item.category);
        let entry = summaries.entry(key).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(item.bytes);
        entry.1 += 1;
    }
    summaries
        .into_iter()
        .map(|(key, (bytes, item_count))| StorageCategorySummary {
            category: match key.as_str() {
                "Instance" => StorageCategory::Instance,
                "DownloadCache" => StorageCategory::DownloadCache,
                "PartialDownload" => StorageCategory::PartialDownload,
                "JavaRuntime" => StorageCategory::JavaRuntime,
                "JavaArchive" => StorageCategory::JavaArchive,
                "LoaderInstaller" => StorageCategory::LoaderInstaller,
                "Log" => StorageCategory::Log,
                "CrashReport" => StorageCategory::CrashReport,
                "WorldBackup" => StorageCategory::WorldBackup,
                "RemovedContentBackup" => StorageCategory::RemovedContentBackup,
                "DeletedInstance" => StorageCategory::DeletedInstance,
                "CorruptBackup" => StorageCategory::CorruptBackup,
                _ => StorageCategory::Temporary,
            },
            bytes,
            item_count,
        })
        .collect()
}

fn plan_fingerprint(items: &[StorageItem]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for item in items {
        hasher.update(item.path.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(item.bytes.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn active_download_targets(app: &AppHandle) -> BTreeSet<PathBuf> {
    let Ok(connection) = open_database(app) else {
        return BTreeSet::new();
    };
    let Ok(mut statement) = connection
        .prepare("SELECT target_path FROM download_jobs WHERE status IN ('downloading','queued')")
    else {
        return BTreeSet::new();
    };
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .into_iter()
        .flatten()
        .flatten();
    rows.map(PathBuf::from).collect()
}

#[tauri::command]
pub fn get_storage_overview(_app: AppHandle) -> Result<StorageOverview, LauncherError> {
    let root = launcher_data_directory()?;
    let items = scan_items(&root);
    let total = items.iter().map(|item| item.bytes).sum::<u64>();
    let reclaimable = items
        .iter()
        .filter(|item| item.safety == DeleteSafety::Safe)
        .map(|item| item.bytes)
        .sum::<u64>();
    Ok(StorageOverview {
        total_bytes: total,
        reclaimable_bytes: reclaimable,
        categories: aggregate(&items),
    })
}

#[tauri::command]
pub fn build_safe_cleanup_plan(app: AppHandle) -> Result<CleanupPlan, LauncherError> {
    let root = launcher_data_directory()?;
    let active = active_download_targets(&app);
    let mut items = scan_items(&root);
    items.retain(|item| {
        item.safety == DeleteSafety::Safe
            && !active.contains(&item.path)
            && !item.path.to_string_lossy().contains("\\.staging\\")
    });
    let reclaimable = items.iter().map(|item| item.bytes).sum::<u64>();
    let fingerprint = plan_fingerprint(&items);
    Ok(CleanupPlan {
        id: format!("cleanup-{}", crate::unique_timestamp()),
        fingerprint,
        generated_at: chrono_like_timestamp().parse().unwrap_or(0),
        reclaimable_bytes: reclaimable,
        destructive_count: 0,
        items,
    })
}

#[tauri::command]
pub fn execute_cleanup_plan(
    app: AppHandle,
    fingerprint: String,
) -> Result<CleanupResult, LauncherError> {
    let root = launcher_data_directory()?;
    let active = active_download_targets(&app);
    let mut items = scan_items(&root);
    items.retain(|item| {
        item.safety == DeleteSafety::Safe
            && !active.contains(&item.path)
            && !item.path.to_string_lossy().contains("\\.staging\\")
    });
    if plan_fingerprint(&items) != fingerprint {
        return Err(LauncherError::validation(
            "清理内容已发生变化，请重新预览后再执行。",
        ));
    }
    let mut freed = 0u64;
    let mut removed = 0usize;
    for item in &items {
        if item.path.exists() {
            if item.path.is_dir() {
                if std::fs::remove_dir_all(&item.path).is_ok() {
                    freed = freed.saturating_add(item.bytes);
                    removed += 1;
                }
            } else if std::fs::remove_file(&item.path).is_ok() {
                freed = freed.saturating_add(item.bytes);
                removed += 1;
            }
        }
    }
    Ok(CleanupResult {
        freed_bytes: freed,
        removed_items: removed,
    })
}

#[tauri::command]
pub fn list_deleted_instances(app: AppHandle) -> Result<Vec<DeletedInstance>, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection
        .prepare(
            "SELECT id, original_instance_id, display_name, backup_path, size_bytes, deleted_at, instance_json
             FROM deleted_instances ORDER BY deleted_at DESC",
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            let json: Option<String> = row.get(6)?;
            let parsed = json
                .as_deref()
                .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
            Ok(DeletedInstance {
                id: row.get(0)?,
                original_instance_id: row.get(1)?,
                display_name: row.get(2)?,
                backup_path: row.get(3)?,
                size_bytes: row.get(4)?,
                deleted_at: row.get(5)?,
                game_version: parsed.as_ref().and_then(|value| {
                    value
                        .get("game_version")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                }),
                loader_type: parsed.as_ref().and_then(|value| {
                    value
                        .get("loader_type")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                }),
            })
        })
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| LauncherError::storage(error.to_string()))
}

#[tauri::command]
pub fn restore_deleted_instance(app: AppHandle, id: String) -> Result<i64, LauncherError> {
    let connection = open_database(&app)?;
    let (original_id, json): (i64, Option<String>) = connection
        .query_row(
            "SELECT original_instance_id, instance_json FROM deleted_instances WHERE id=?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| LauncherError::validation("已删除实例记录不存在。"))?;
    let value = json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .ok_or_else(|| LauncherError::validation("实例恢复信息缺失，无法恢复。"))?;
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("已恢复实例");
    let root_path = value
        .get("root_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LauncherError::validation("实例原路径缺失，无法恢复。"))?;
    let game_version = value
        .get("game_version")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let loader_type = value
        .get("loader_type")
        .and_then(|v| v.as_str())
        .unwrap_or("vanilla");
    if connection
        .query_row(
            "SELECT COUNT(*) FROM instances WHERE id=?1",
            [original_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?
        > 0
    {
        return Err(LauncherError::validation("原实例编号已被占用，无法恢复。"));
    }
    connection
        .execute(
            "INSERT INTO instances(id, name, root_path, game_version, loader_type, memory_mb, status, source, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, 4096, 'missing', 'restored', ?6)",
            params![original_id, name, root_path, game_version, loader_type, chrono_like_timestamp()],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    connection
        .execute("DELETE FROM deleted_instances WHERE id=?1", [&id])
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(original_id)
}

#[tauri::command]
pub fn permanently_delete_instance_backup(app: AppHandle, id: String) -> Result<(), LauncherError> {
    let connection = open_database(&app)?;
    let backup_path: String = connection
        .query_row(
            "SELECT backup_path FROM deleted_instances WHERE id=?1",
            [&id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("已删除实例记录不存在。"))?;
    let root = launcher_data_directory()?;
    let backup = PathBuf::from(&backup_path);
    if backup.is_dir() {
        fs_safe::ensure_canonical_child(&root.join("backups"), &backup)?;
        std::fs::remove_dir_all(&backup)
            .map_err(|error| LauncherError::storage(format!("永久删除失败：{error}")))?;
    }
    connection
        .execute("DELETE FROM deleted_instances WHERE id=?1", [&id])
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}

pub(crate) fn record_deleted_instance(
    connection: &rusqlite::Connection,
    original_instance_id: i64,
    display_name: &str,
    backup_path: &str,
    size_bytes: u64,
    instance_json: &str,
) -> Result<(), LauncherError> {
    let id = format!(
        "deleted-{}-{}",
        original_instance_id,
        chrono_like_timestamp()
    );
    connection
        .execute(
            "INSERT INTO deleted_instances(id, original_instance_id, display_name, backup_path, size_bytes, deleted_at, instance_json)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id,
                original_instance_id,
                display_name,
                backup_path,
                size_bytes as i64,
                chrono_like_timestamp(),
                instance_json
            ],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}
