//! 磁盘 ↔ 数据库内容对账：Scan（只读）→ 预览 → Apply（事务 + 备份）。

use crate::{
    chrono_like_timestamp, inspect_mod_jar_path, launcher_data_directory, open_database,
    running_games, unique_timestamp, LauncherError,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub sha256: String,
    pub files: Vec<String>,
    pub keep: String,
    pub removable_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileReport {
    pub instance_id: i64,
    pub db_missing_on_disk: Vec<String>,
    pub disk_missing_in_db: Vec<String>,
    pub duplicate_groups: Vec<DuplicateGroup>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileApplyResult {
    pub added_records: usize,
    pub removed_stale_records: usize,
    pub deduplicated_files: usize,
    pub freed_bytes: u64,
}

fn sha256_file(path: &PathBuf) -> Result<String, LauncherError> {
    let bytes = std::fs::read(path).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn canonical_name(file_name: &str) -> String {
    // 去掉“19 位纳秒时间戳-”前缀；没有则保持原名。
    let bytes = file_name.as_bytes();
    if bytes.len() > 20 && bytes[..19].iter().all(|b| b.is_ascii_digit()) && bytes[19] == b'-' {
        file_name[20..].to_string()
    } else {
        file_name.to_string()
    }
}

fn build_report(
    instance_id: i64,
    root_path: &str,
    connection: &rusqlite::Connection,
) -> Result<ReconcileReport, LauncherError> {
    let mods_dir = PathBuf::from(root_path).join(".minecraft").join("mods");
    let db_files: Vec<(String, String)> = {
        let mut statement = connection
            .prepare(
                "SELECT file_name, hash FROM content_items WHERE instance_id=?1 AND kind='mod'",
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let rows = statement
            .query_map([instance_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        drop(statement);
        rows
    };
    let mut disk_files: Vec<PathBuf> = Vec::new();
    if mods_dir.is_dir() {
        for entry in std::fs::read_dir(&mods_dir)
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .flatten()
        {
            if entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("jar"))
            {
                disk_files.push(entry.path());
            }
        }
    }
    let mut disk_hash_map: BTreeMap<String, String> = BTreeMap::new();
    let mut duplicates: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for path in &disk_files {
        let hash = sha256_file(path)?;
        disk_hash_map.insert(
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default(),
            hash.clone(),
        );
        duplicates.entry(hash).or_default().push(path.clone());
    }
    let db_missing_on_disk = db_files
        .iter()
        .filter(|(file_name, _)| !disk_hash_map.contains_key(file_name))
        .map(|(file_name, _)| file_name.clone())
        .collect::<Vec<_>>();
    let db_hashes = db_files
        .iter()
        .map(|(_, hash)| hash.clone())
        .collect::<std::collections::HashSet<_>>();
    let disk_missing_in_db = disk_files
        .iter()
        .filter(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let hash = disk_hash_map.get(&name).cloned().unwrap_or_default();
            !db_hashes.contains(&hash)
        })
        .map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let duplicate_groups = duplicates
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(hash, files)| {
            let keep_path = files
                .iter()
                .min_by_key(|path| {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default();
                    (canonical_name(&name) != name, name.len())
                })
                .cloned()
                .unwrap_or_else(|| files[0].clone());
            let removable_bytes = files
                .iter()
                .filter(|path| *path != &keep_path)
                .filter_map(|path| std::fs::metadata(path).ok().map(|meta| meta.len()))
                .sum();
            DuplicateGroup {
                keep: keep_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
                files: files
                    .iter()
                    .map(|path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_default()
                    })
                    .collect(),
                removable_bytes,
                sha256: hash,
            }
        })
        .collect::<Vec<_>>();
    let mut fingerprint = Sha256::new();
    for name in &db_missing_on_disk {
        fingerprint.update(name.as_bytes());
    }
    for name in &disk_missing_in_db {
        fingerprint.update(name.as_bytes());
    }
    for group in &duplicate_groups {
        fingerprint.update(group.sha256.as_bytes());
        for file in &group.files {
            fingerprint.update(file.as_bytes());
        }
    }
    Ok(ReconcileReport {
        instance_id,
        db_missing_on_disk,
        disk_missing_in_db,
        duplicate_groups,
        fingerprint: format!("{:x}", fingerprint.finalize()),
    })
}

#[tauri::command]
pub fn reconcile_scan(app: AppHandle, instance_id: i64) -> Result<ReconcileReport, LauncherError> {
    let connection = open_database(&app)?;
    let root_path: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))?;
    build_report(instance_id, &root_path, &connection)
}

#[tauri::command]
pub fn reconcile_apply(
    app: AppHandle,
    instance_id: i64,
    fingerprint: String,
) -> Result<ReconcileApplyResult, LauncherError> {
    if running_games()
        .lock()
        .map_err(|_| LauncherError::storage("无法读取游戏运行状态。"))?
        .contains_key(&instance_id)
    {
        return Err(LauncherError::validation(
            "实例正在运行，不能执行对账写入。",
        ));
    }
    let mut connection = open_database(&app)?;
    let root_path: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))?;
    let fresh = build_report(instance_id, &root_path, &connection)?;
    if fresh.fingerprint != fingerprint {
        return Err(LauncherError::validation(
            "内容已发生变化，请重新扫描后再应用。",
        ));
    }
    // 联机受管理 helper 的 DB 对账：文件缺失 / 哈希变化时修正 DB，绝不假装可信。
    crate::multiplayer::reconcile_managed_helpers(&connection, instance_id)?;
    let backup_base = launcher_data_directory()?.join("backups");
    apply_reconcile_core(
        &mut connection,
        instance_id,
        &root_path,
        &fresh,
        &backup_base,
    )
}

/// 对账应用核心：文件移动走 FsTransaction、DB 增删走 rusqlite 事务，任一失败联动回滚。
/// 独立于 Tauri 上下文，便于在临时实例目录 + 内存 DB 上做往返集成测试。
fn apply_reconcile_core(
    connection: &mut rusqlite::Connection,
    instance_id: i64,
    root_path: &str,
    fresh: &ReconcileReport,
    backup_base: &std::path::Path,
) -> Result<ReconcileApplyResult, LauncherError> {
    let mods_dir = PathBuf::from(&root_path).join(".minecraft").join("mods");
    let backup_root = backup_base.join("removed-content").join(format!(
        "reconcile-{}-{}",
        instance_id,
        unique_timestamp()
    ));
    let mut freed = 0u64;
    let mut deduplicated = 0usize;
    let mut file_transaction =
        crate::fs_safe::FsTransaction::new(format!("reconcile-apply-{instance_id}"));
    for group in &fresh.duplicate_groups {
        let keep = group.keep.clone();
        for file in &group.files {
            if *file == keep {
                continue;
            }
            let path = mods_dir.join(file);
            if path.is_file() {
                std::fs::create_dir_all(&backup_root)
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
                if let Ok(meta) = std::fs::metadata(&path) {
                    freed = freed.saturating_add(meta.len());
                }
                let backup_path = backup_root.join(file);
                if file_transaction.move_with_undo(&path, &backup_path).is_ok() {
                    deduplicated += 1;
                }
            }
        }
    }
    let (added, removed) = {
        let db_transaction = connection
            .transaction()
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let db_result = (|| -> Result<(usize, usize), LauncherError> {
            let mut added = 0usize;
            for file_name in &fresh.disk_missing_in_db {
                let path = mods_dir.join(file_name);
                let Ok(info) = inspect_mod_jar_path(&path) else {
                    continue;
                };
                let metadata = serde_json::to_string(&info).unwrap_or_default();
                let changed = db_transaction
                    .execute(
                        "INSERT OR IGNORE INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at)
                         VALUES(?1,'mod',?2,?3,?4,1,'external',?5)",
                        rusqlite::params![
                            instance_id,
                            file_name,
                            info.sha256,
                            metadata,
                            chrono_like_timestamp()
                        ],
                    )
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
                added += usize::from(changed > 0);
            }
            let mut removed = 0usize;
            for file_name in &fresh.db_missing_on_disk {
                removed += db_transaction
                    .execute(
                        "DELETE FROM content_items WHERE instance_id=?1 AND kind='mod' AND file_name=?2",
                        rusqlite::params![instance_id, file_name],
                    )
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
            }
            Ok((added, removed))
        })();
        match db_result {
            Ok(counts) => {
                db_transaction
                    .commit()
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
                counts
            }
            Err(error) => {
                drop(db_transaction);
                file_transaction.rollback()?;
                return Err(error);
            }
        }
    };
    file_transaction.commit();
    Ok(ReconcileApplyResult {
        added_records: added,
        removed_stale_records: removed,
        deduplicated_files: deduplicated,
        freed_bytes: freed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn write_fabric_jar(path: &std::path::Path, mod_id: &str, payload: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        archive.start_file("fabric.mod.json", options).unwrap();
        archive
            .write_all(
                format!(
                    r#"{{"schemaVersion":1,"id":"{mod_id}","version":"1.0.0","name":"{mod_id}"}}"#
                )
                .as_bytes(),
            )
            .unwrap();
        archive.start_file("payload.bin", options).unwrap();
        archive.write_all(payload).unwrap();
        archive.finish().unwrap();
    }

    #[test]
    fn canonical_name_strips_timestamp_prefix() {
        assert_eq!(
            canonical_name("1786803006838905300-artifacts-forge-9.5.13.jar"),
            "artifacts-forge-9.5.13.jar"
        );
        assert_eq!(
            canonical_name("artifacts-forge-9.5.13.jar"),
            "artifacts-forge-9.5.13.jar"
        );
    }

    #[test]
    fn reconcile_round_trip_on_temp_instance() {
        let directory = std::env::temp_dir().join(format!("sh-reconcile-{}", unique_timestamp()));
        let mods = directory.join(".minecraft").join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        // a.jar 与 b.jar 内容完全一致（重复组）；c.jar 仅在磁盘。
        write_fabric_jar(&mods.join("a.jar"), "shared", b"same-content");
        write_fabric_jar(&mods.join("b.jar"), "shared", b"same-content");
        write_fabric_jar(&mods.join("c.jar"), "gamma", b"other-content");

        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::run_migrations(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO instances(id,name,root_path,game_version,loader_type,status,source,created_at)
                 VALUES(1,'QA',?1,'1.20.1','fabric','ready','qa','1')",
                [directory.to_string_lossy().to_string()],
            )
            .unwrap();
        let alpha_hash = crate::sha256_file_sync(&mods.join("a.jar")).unwrap();
        connection
            .execute(
                "INSERT INTO content_items(instance_id,kind,file_name,hash,enabled,source,installed_at)
                 VALUES(1,'mod','a.jar',?1,1,'modrinth','1')",
                [&alpha_hash],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO content_items(instance_id,kind,file_name,hash,enabled,source,installed_at)
                 VALUES(1,'mod','stale.jar','stale-hash',1,'modrinth','1')",
                [],
            )
            .unwrap();

        let root = directory.to_string_lossy().to_string();
        let before = build_report(1, &root, &connection).unwrap();
        assert_eq!(before.duplicate_groups.len(), 1, "应发现 1 个重复组");
        assert_eq!(
            before.db_missing_on_disk,
            vec!["stale.jar".to_string()],
            "应发现数据库有但磁盘无的过期记录"
        );
        assert_eq!(
            before.disk_missing_in_db,
            vec!["c.jar".to_string()],
            "应发现磁盘有但数据库无的模组"
        );

        let backup_base = directory.join(".backups");
        apply_reconcile_core(&mut connection, 1, &root, &before, &backup_base).unwrap();

        let after = build_report(1, &root, &connection).unwrap();
        assert!(after.duplicate_groups.is_empty(), "应用后不应再有重复组");
        assert!(after.db_missing_on_disk.is_empty(), "应用后过期记录应清理");
        assert!(
            after.disk_missing_in_db.is_empty(),
            "应用后磁盘模组应全部入库"
        );
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM content_items WHERE instance_id=1 AND kind='mod'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "保留 a.jar 与 c.jar 两条记录");
        let backup_files = walkdir::WalkDir::new(&backup_base)
            .into_iter()
            .flatten()
            .filter(|entry| entry.file_type().is_file())
            .count();
        assert_eq!(backup_files, 1, "重复文件应移入备份目录");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
