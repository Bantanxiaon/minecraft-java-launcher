#![allow(
    clippy::needless_lifetimes,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::useless_conversion
)]

use dashmap::DashMap;
use futures_util::StreamExt;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::time::Duration;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Instant,
};
use tauri::Manager;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const AUTHLIB_INJECTOR_VERSION: &str = "1.2.8";
const AUTHLIB_INJECTOR_MIN_BYTES: u64 = 300_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildInfo {
    version: &'static str,
    git_commit: &'static str,
    channel: &'static str,
    build_timestamp: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstanceHealth {
    instance_id: i64,
    name: String,
    game_version: String,
    loader_type: String,
    loader_version: Option<String>,
    memory_mb: i64,
    status: String,
    game_files_ok: bool,
    mod_count: usize,
    missing_dependencies: Vec<String>,
    incompatible_mods: Vec<String>,
}

#[tauri::command]
fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        git_commit: env!("SH_GIT_COMMIT"),
        channel: if env!("CARGO_PKG_VERSION").starts_with("0.") {
            "beta"
        } else {
            "stable"
        },
        build_timestamp: env!("SH_BUILD_TIMESTAMP"),
    }
}

#[tauri::command]
fn instance_health(app: AppHandle, instance_id: i64) -> Result<InstanceHealth, LauncherError> {
    let connection = open_database(&app)?;
    let (name, root_path, game_version, loader_type, loader_version, memory_mb, status): (
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        String,
    ) = connection
        .query_row(
            "SELECT name, root_path, game_version, loader_type, loader_version, memory_mb, status FROM instances WHERE id=?1",
            [instance_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))?;
    drop(connection);
    let game = PathBuf::from(&root_path).join(".minecraft");
    let game_files_ok = game
        .join("versions")
        .join(&game_version)
        .join(format!("{game_version}.jar"))
        .is_file();
    let mods = game.join("mods");
    let mut mod_count = 0usize;
    let mut missing_ids = Vec::new();
    let mut incompatible_mods = Vec::new();
    if loader_type != "vanilla" && mods.is_dir() {
        for entry in fs::read_dir(&mods)
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .flatten()
        {
            if entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_none_or(|value| !value.eq_ignore_ascii_case("jar"))
            {
                continue;
            }
            let Ok(info) = inspect_mod_jar_path(&entry.path()) else {
                continue;
            };
            if info.loader_type == "unknown" {
                continue;
            }
            mod_count += 1;
            if !info.game_version_requirements.is_empty()
                && !info
                    .game_version_requirements
                    .iter()
                    .any(|requirement| game_version_matches(requirement, &game_version))
            {
                incompatible_mods.push(info.file_name.clone());
            }
            if !loaders_compatible(&loader_type, &info.loader_type) {
                incompatible_mods.push(info.file_name);
            }
        }
        let installed = installed_mod_ids(
            fs::read_dir(&mods)
                .map_err(|error| LauncherError::storage(error.to_string()))?
                .flatten()
                .filter_map(|entry| inspect_mod_jar_path(&entry.path()).ok())
                .collect::<Vec<_>>()
                .iter(),
        );
        let mut seen = BTreeSet::new();
        for entry in fs::read_dir(&mods)
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .flatten()
        {
            let Ok(info) = inspect_mod_jar_path(&entry.path()) else {
                continue;
            };
            for dependency in missing_dependencies(
                info.dependencies.iter().map(|id| id.as_str()),
                &installed,
                has_kotlinforforge_file(&mods),
            ) {
                seen.insert(dependency);
            }
        }
        missing_ids = seen.into_iter().collect();
    }
    Ok(InstanceHealth {
        instance_id,
        name,
        game_version,
        loader_type,
        loader_version,
        memory_mb,
        status,
        game_files_ok,
        mod_count,
        missing_dependencies: missing_ids,
        incompatible_mods,
    })
}

/// 与 Java `UUID.nameUUIDFromBytes("OfflinePlayer:<name>")` 完全一致的离线 UUID：
/// 对输入 bytes 做 MD5，再按 Java 语义设置 version=3 与 IETF variant。
pub(crate) fn minecraft_offline_uuid(player_name: &str) -> Uuid {
    use md5::{Digest, Md5};
    let input = format!("OfflinePlayer:{player_name}");
    let digest = Md5::digest(input.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// 旧版启动器曾使用 SHA-256 前 32 hex 作为离线 UUID，保留用于历史兼容。
pub(crate) fn legacy_offline_uuid(player_name: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("OfflinePlayer:{player_name}").as_bytes())
    )[..32]
        .to_string()
}

#[cfg(debug_assertions)]
mod acceptance;
mod auth;
mod content_reconcile;
mod diagnostics;
mod download_perf;
mod exports;
mod fs_safe;
mod multiplayer;
mod storage;
mod system;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LauncherError {
    code: String,
    message: String,
    recoverable: bool,
}

impl LauncherError {
    pub(crate) fn storage(message: impl Into<String>) -> Self {
        Self {
            code: "storage_error".into(),
            message: message.into(),
            recoverable: true,
        }
    }
    pub(crate) fn validation(message: impl Into<String>) -> Self {
        Self {
            code: "validation_error".into(),
            message: message.into(),
            recoverable: true,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    id: i64,
    account_type: String,
    display_name: String,
    created_at: String,
    last_used_at: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LauncherSettings {
    game_directory: Option<String>,
    download_concurrency: u32,
    close_launcher_after_game_start: bool,
    language: String,
    default_memory_mb: u32,
    #[serde(default)]
    microsoft_client_id: Option<String>,
    #[serde(default)]
    backup_worlds_before_launch: bool,
    #[serde(default = "default_ui_theme")]
    ui_theme: String,
}

fn default_ui_theme() -> String {
    "modern".into()
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            game_directory: None,
            download_concurrency: 8,
            close_launcher_after_game_start: false,
            language: "zh-CN".into(),
            default_memory_mb: 4096,
            microsoft_client_id: None,
            backup_worlds_before_launch: false,
            ui_theme: default_ui_theme(),
        }
    }
}

fn validate_settings(settings: &LauncherSettings) -> Result<(), LauncherError> {
    if !(1..=64).contains(&settings.download_concurrency) {
        return Err(LauncherError::validation("下载并发数须为 1–64。"));
    }
    if !(2048..=65536).contains(&settings.default_memory_mb) {
        return Err(LauncherError::validation("默认内存须为 2048–65536 MB。"));
    }
    if !matches!(settings.language.as_str(), "zh-CN" | "en-US") {
        return Err(LauncherError::validation("不支持的界面语言。"));
    }
    if !matches!(settings.ui_theme.as_str(), "modern" | "classic") {
        return Err(LauncherError::validation("不支持的界面主题。"));
    }
    Ok(())
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<LauncherSettings, LauncherError> {
    let connection = open_database(&app)?;
    let value = connection.query_row(
        "SELECT value_json FROM settings WHERE key='launcher'",
        [],
        |row| row.get::<_, String>(0),
    );
    match value {
        Ok(json) => serde_json::from_str(&json)
            .map_err(|error| LauncherError::storage(format!("设置数据无效：{error}"))),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(LauncherSettings::default()),
        Err(error) => Err(LauncherError::storage(error.to_string())),
    }
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    settings: LauncherSettings,
) -> Result<LauncherSettings, LauncherError> {
    validate_settings(&settings)?;
    let json = serde_json::to_string(&settings)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let connection = open_database(&app)?;
    connection.execute("INSERT INTO settings(key, value_json) VALUES('launcher', ?1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json", [json]).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(settings)
}

pub(crate) fn launcher_data_directory() -> Result<PathBuf, LauncherError> {
    if let Some(explicit) = std::env::var_os("MINECRAFT_LAUNCHER_DATA").map(PathBuf::from) {
        fs::create_dir_all(&explicit).map_err(|error| LauncherError::storage(error.to_string()))?;
        return Ok(explicit);
    }
    // 旧版 D 盘数据目录若存在，继续沿用，避免升级丢数据。
    let legacy = PathBuf::from(r"D:\MinecraftLauncherData");
    if legacy.is_dir() {
        return Ok(legacy);
    }
    // 没有旧数据时使用系统本地应用数据目录，不再要求必须有 D 盘。
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("SHLauncher"))
        .join("SHLauncher");
    fs::create_dir_all(&local).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(local)
}

fn database_path(_app: &AppHandle) -> Result<PathBuf, LauncherError> {
    let directory = launcher_data_directory()?;
    Ok(directory.join("launcher.sqlite3"))
}

fn run_migrations(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.execute_batch("PRAGMA foreign_keys = ON;
    CREATE TABLE IF NOT EXISTS migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS accounts (id INTEGER PRIMARY KEY, account_type TEXT NOT NULL CHECK(account_type IN ('OFFLINE','MICROSOFT','EXTERNAL')), display_name TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL, last_used_at TEXT, safe_secret_ref TEXT, auth_server TEXT);
    CREATE TABLE IF NOT EXISTS servers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, address TEXT NOT NULL, port INTEGER NOT NULL DEFAULT 25565, description TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, last_connected_at TEXT);
    CREATE TABLE IF NOT EXISTS modpack_archives (id INTEGER PRIMARY KEY, source_kind TEXT NOT NULL, file_path TEXT, project_id TEXT, file_name TEXT NOT NULL, name TEXT, version TEXT, game_version TEXT, loader_type TEXT, format TEXT NOT NULL, size_bytes INTEGER, instance_id INTEGER, imported_at TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS instances (id INTEGER PRIMARY KEY, name TEXT NOT NULL, icon TEXT, root_path TEXT NOT NULL UNIQUE, game_version TEXT NOT NULL, loader_type TEXT NOT NULL, loader_version TEXT, java_profile TEXT, memory_mb INTEGER NOT NULL DEFAULT 4096, resolution TEXT, last_played TEXT, status TEXT NOT NULL, source TEXT NOT NULL, created_at TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS installation_states (id INTEGER PRIMARY KEY, instance_id INTEGER NOT NULL REFERENCES instances(id) ON DELETE CASCADE, component_kind TEXT NOT NULL, component_key TEXT NOT NULL, hash TEXT, size_bytes INTEGER, status TEXT NOT NULL, UNIQUE(instance_id, component_kind, component_key));
    CREATE TABLE IF NOT EXISTS download_jobs (id INTEGER PRIMARY KEY, source_url TEXT NOT NULL, target_path TEXT NOT NULL, progress_bytes INTEGER NOT NULL DEFAULT 0, total_bytes INTEGER, retry_count INTEGER NOT NULL DEFAULT 0, expected_hash TEXT, status TEXT NOT NULL, error TEXT, recovery_action TEXT, created_at TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS play_history (id INTEGER PRIMARY KEY, instance_id INTEGER NOT NULL REFERENCES instances(id) ON DELETE CASCADE, started_at TEXT NOT NULL, ended_at TEXT, exit_code INTEGER);
    CREATE TABLE IF NOT EXISTS content_items (id INTEGER PRIMARY KEY, instance_id INTEGER NOT NULL REFERENCES instances(id) ON DELETE CASCADE, kind TEXT NOT NULL, file_name TEXT NOT NULL, hash TEXT NOT NULL, metadata_json TEXT, enabled INTEGER NOT NULL DEFAULT 1, dependencies_json TEXT, conflicts_json TEXT, source TEXT NOT NULL, installed_at TEXT NOT NULL, UNIQUE(instance_id, kind, hash));
    CREATE TABLE IF NOT EXISTS crash_reports (id INTEGER PRIMARY KEY, instance_id INTEGER NOT NULL REFERENCES instances(id) ON DELETE CASCADE, occurred_at TEXT NOT NULL, exit_code INTEGER, log_path TEXT NOT NULL, suspected_cause TEXT NOT NULL, confidence TEXT NOT NULL, suggestion TEXT NOT NULL);
    INSERT OR IGNORE INTO migrations(version, applied_at) VALUES(1, strftime('%s','now'));
    INSERT OR IGNORE INTO migrations(version, applied_at) VALUES(2, strftime('%s','now'));
    INSERT OR IGNORE INTO migrations(version, applied_at) VALUES(3, strftime('%s','now'));")?;
    let mut statement = connection.prepare("PRAGMA table_info(download_jobs)")?;
    let mut columns = statement.query([])?;
    let mut existing = Vec::new();
    while let Some(row) = columns.next()? {
        existing.push(row.get::<_, String>(1)?);
    }
    drop(columns);
    drop(statement);
    if !existing.iter().any(|name| name == "started_at") {
        connection.execute("ALTER TABLE download_jobs ADD COLUMN started_at TEXT", [])?;
    }
    if !existing.iter().any(|name| name == "updated_at") {
        connection.execute("ALTER TABLE download_jobs ADD COLUMN updated_at TEXT", [])?;
    }
    if !existing.iter().any(|name| name == "bytes_per_second") {
        connection.execute(
            "ALTER TABLE download_jobs ADD COLUMN bytes_per_second INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !existing.iter().any(|name| name == "eta_seconds") {
        connection.execute(
            "ALTER TABLE download_jobs ADD COLUMN eta_seconds INTEGER",
            [],
        )?;
    }
    connection.execute(
        "INSERT OR IGNORE INTO migrations(version, applied_at) VALUES(4, strftime('%s','now'))",
        [],
    )?;
    let has_v5: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM migrations WHERE version=5)",
        [],
        |row| row.get(0),
    )?;
    if !has_v5 {
        let tx = connection.transaction()?;
        tx.execute_batch(
            "ALTER TABLE accounts RENAME TO accounts_old;
            CREATE TABLE accounts (id INTEGER PRIMARY KEY, account_type TEXT NOT NULL CHECK(account_type IN ('OFFLINE','MICROSOFT','EXTERNAL')), display_name TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL, last_used_at TEXT, safe_secret_ref TEXT, auth_server TEXT);
            INSERT INTO accounts (id, account_type, display_name, created_at, last_used_at, safe_secret_ref, auth_server)
            SELECT id, account_type, display_name, created_at, last_used_at, safe_secret_ref, NULL FROM accounts_old;
            DROP TABLE accounts_old;",
        )?;
        tx.execute(
            "INSERT INTO migrations(version, applied_at) VALUES(5, strftime('%s','now'))",
            [],
        )?;
        tx.commit()?;
    }
    let has_v6: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM migrations WHERE version=6)",
        [],
        |row| row.get(0),
    )?;
    if !has_v6 {
        let tx = connection.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS content_provenance (
                content_id INTEGER PRIMARY KEY,
                provider TEXT NOT NULL,
                project_id TEXT,
                version_id TEXT,
                file_id TEXT,
                source_url TEXT,
                sha1 TEXT,
                sha256 TEXT,
                installed_at TEXT NOT NULL,
                FOREIGN KEY(content_id) REFERENCES content_items(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS content_identity_cache (
                local_mod_id TEXT NOT NULL,
                game_version TEXT NOT NULL,
                loader TEXT NOT NULL,
                provider TEXT NOT NULL,
                project_id TEXT NOT NULL,
                confidence TEXT NOT NULL,
                source TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(local_mod_id, game_version, loader, provider)
            );
            CREATE TABLE IF NOT EXISTS storage_retention_policy (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                successful_download_days INTEGER NOT NULL DEFAULT 14,
                failed_download_days INTEGER NOT NULL DEFAULT 60,
                log_days INTEGER NOT NULL DEFAULT 30,
                deleted_instance_days INTEGER NOT NULL DEFAULT 30,
                removed_content_days INTEGER NOT NULL DEFAULT 30,
                world_backup_count INTEGER NOT NULL DEFAULT 5,
                mod_backup_versions INTEGER NOT NULL DEFAULT 2
            );
            INSERT OR IGNORE INTO storage_retention_policy(id) VALUES (1);
            CREATE TABLE IF NOT EXISTS deleted_instances (
                id TEXT PRIMARY KEY,
                original_instance_id INTEGER NOT NULL,
                display_name TEXT NOT NULL,
                backup_path TEXT NOT NULL,
                size_bytes INTEGER,
                deleted_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS instance_launch_settings (
                instance_id INTEGER PRIMARY KEY,
                memory_min_mb INTEGER,
                memory_max_mb INTEGER,
                java_mode TEXT NOT NULL DEFAULT 'AUTO',
                java_path TEXT,
                jvm_args_json TEXT NOT NULL DEFAULT '[]',
                game_args_json TEXT NOT NULL DEFAULT '[]',
                width INTEGER,
                height INTEGER,
                account_id INTEGER,
                FOREIGN KEY(instance_id) REFERENCES instances(id) ON DELETE CASCADE,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE SET NULL
            );
            CREATE TABLE IF NOT EXISTS managed_content (
                id TEXT PRIMARY KEY,
                instance_id INTEGER NOT NULL,
                kind TEXT NOT NULL,
                provider TEXT NOT NULL,
                project_id TEXT NOT NULL,
                version_id TEXT NOT NULL,
                file_sha1 TEXT,
                file_sha256 TEXT NOT NULL,
                installed_path TEXT NOT NULL,
                installed_by_launcher INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                FOREIGN KEY(instance_id) REFERENCES instances(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS instance_pack_source (
                instance_id INTEGER PRIMARY KEY,
                provider TEXT NOT NULL,
                project_id TEXT,
                version_id TEXT,
                pack_version TEXT,
                source_url TEXT,
                installed_at TEXT NOT NULL,
                FOREIGN KEY(instance_id) REFERENCES instances(id) ON DELETE CASCADE
            );",
        )?;
        tx.execute(
            "INSERT INTO migrations(version, applied_at) VALUES(6, strftime('%s','now'))",
            [],
        )?;
        tx.commit()?;
    }
    let has_v7: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM migrations WHERE version=7)",
        [],
        |row| row.get(0),
    )?;
    if !has_v7 {
        let tx = connection.transaction()?;
        tx.execute_batch(
            "ALTER TABLE accounts RENAME TO accounts_identity_old;
            CREATE TABLE accounts (
                id INTEGER PRIMARY KEY,
                account_type TEXT NOT NULL CHECK(account_type IN ('OFFLINE','MICROSOFT','EXTERNAL')),
                minecraft_uuid TEXT,
                display_name TEXT NOT NULL,
                microsoft_subject TEXT,
                xuid TEXT,
                credential_ref TEXT,
                legacy_offline_uuid TEXT,
                created_at TEXT NOT NULL,
                last_used_at TEXT,
                auth_server TEXT
            );
            CREATE UNIQUE INDEX idx_accounts_minecraft_uuid
                ON accounts(minecraft_uuid) WHERE minecraft_uuid IS NOT NULL;
            INSERT INTO accounts (id, account_type, display_name, created_at, last_used_at, credential_ref, auth_server)
            SELECT id, account_type, display_name, created_at, last_used_at, safe_secret_ref, auth_server FROM accounts_identity_old;
            DROP TABLE accounts_identity_old;",
        )?;
        tx.execute(
            "INSERT INTO migrations(version, applied_at) VALUES(7, strftime('%s','now'))",
            [],
        )?;
        tx.commit()?;
    }
    let has_v8: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM migrations WHERE version=8)",
        [],
        |row| row.get(0),
    )?;
    if !has_v8 {
        connection.execute_batch("ALTER TABLE deleted_instances ADD COLUMN instance_json TEXT;")?;
        connection.execute(
            "INSERT INTO migrations(version, applied_at) VALUES(8, strftime('%s','now'))",
            [],
        )?;
    }
    // 补齐离线账户的标准 UUID 与旧 SHA-256 UUID，仅处理缺失的旧数据。
    {
        let mut statement = connection.prepare(
            "SELECT id, display_name FROM accounts
             WHERE account_type='OFFLINE' AND (minecraft_uuid IS NULL OR legacy_offline_uuid IS NULL)",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        drop(statement);
        for (account_id, display_name) in rows {
            let standard = minecraft_offline_uuid(&display_name).to_string();
            let legacy = legacy_offline_uuid(&display_name);
            connection.execute(
                "UPDATE accounts SET minecraft_uuid=?1, legacy_offline_uuid=?2 WHERE id=?3",
                params![standard, legacy, account_id],
            )?;
        }
    }
    Ok(())
}

pub(crate) fn open_database(app: &AppHandle) -> Result<Connection, LauncherError> {
    let db_path = database_path(app)?;
    backup_database_before_migration(&db_path);
    let mut connection = Connection::open(database_path(app)?)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    run_migrations(&mut connection).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(connection)
}

fn backup_database_before_migration(db_path: &Path) {
    let Ok(metadata) = fs::metadata(db_path) else {
        return;
    };
    if metadata.len() == 0 {
        return;
    }
    let Some(parent) = db_path.parent() else {
        return;
    };
    let backup_dir = parent.join("db-backups");
    if fs::create_dir_all(&backup_dir).is_err() {
        return;
    }
    let stamp = chrono_like_timestamp();
    let backup_path = backup_dir.join(format!(
        "launcher.db.pre-{}-{stamp}.bak",
        env!("CARGO_PKG_VERSION")
    ));
    if backup_path.exists() {
        return;
    }
    let _ = fs::copy(db_path, &backup_path);
    // 保留最近 5 份迁移前备份。
    if let Ok(entries) = fs::read_dir(&backup_dir) {
        let mut backups = entries
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("launcher.db.pre-")
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        backups.sort();
        while backups.len() > 5 {
            if let Some(oldest) = backups.first() {
                let _ = fs::remove_file(oldest);
                backups.remove(0);
            } else {
                break;
            }
        }
    }
}

fn recover_interrupted_download_jobs(connection: &Connection) -> rusqlite::Result<usize> {
    connection.execute(
        "UPDATE download_jobs
         SET status='failed',
             error=COALESCE(error, '上次下载被关闭或意外中断，请重新下载；已完成的文件会自动校验并继续使用。'),
             recovery_action='重新下载'
         WHERE status='downloading'",
        [],
    )
}

#[tauri::command]
fn list_accounts(app: AppHandle) -> Result<Vec<Account>, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection.prepare("SELECT id, account_type, display_name, created_at, last_used_at FROM accounts ORDER BY COALESCE(last_used_at, created_at) DESC").map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok(Account {
                id: row.get(0)?,
                account_type: row.get(1)?,
                display_name: row.get(2)?,
                created_at: row.get(3)?,
                last_used_at: row.get(4)?,
            })
        })
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let accounts = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(accounts)
}

#[tauri::command]
fn create_offline_account(app: AppHandle, display_name: String) -> Result<Account, LauncherError> {
    validate_profile_name(&display_name)?;
    let connection = open_database(&app)?;
    let created_at = chrono_like_timestamp();
    connection
        .execute(
            "INSERT INTO accounts (account_type, minecraft_uuid, display_name, legacy_offline_uuid, created_at, last_used_at)
             VALUES ('OFFLINE', ?1, ?2, ?3, ?4, ?4)",
            params![
                minecraft_offline_uuid(&display_name).to_string(),
                display_name,
                legacy_offline_uuid(&display_name),
                created_at
            ],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let id = connection.last_insert_rowid();
    Ok(Account {
        id,
        account_type: "OFFLINE".into(),
        display_name,
        created_at: created_at.clone(),
        last_used_at: Some(created_at),
    })
}

#[tauri::command]
async fn login_microsoft(app: AppHandle, client_id: String) -> Result<Account, LauncherError> {
    let normalized_client_id = if client_id.trim().is_empty() {
        option_env!("SH_MICROSOFT_CLIENT_ID")
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        client_id.trim().to_string()
    };
    let result = auth::login(&normalized_client_id)
        .await
        .map_err(LauncherError::validation)?;
    let profile_name = result.profile.name.clone();
    let profile_uuid = result.profile.uuid.clone();
    let profile_xuid = result.profile.xuid.clone();
    let access_token = result.access_token.clone();
    let refresh_token = result.refresh_token.clone();
    let connection = open_database(&app)?;
    let created_at = chrono_like_timestamp();
    connection
        .execute(
            "INSERT INTO accounts (account_type, minecraft_uuid, display_name, xuid, created_at, last_used_at)
             VALUES ('MICROSOFT', ?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(minecraft_uuid) DO UPDATE SET account_type='MICROSOFT', last_used_at=excluded.last_used_at",
            params![profile_uuid, profile_name, profile_xuid, created_at],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let account_id: i64 = connection
        .query_row(
            "SELECT id FROM accounts WHERE display_name=?1",
            [&profile_name],
            |row| row.get(0),
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let secret_ref = format!("microsoft-account-{account_id}");
    let entry = keyring::Entry::new("SH启动器", &secret_ref)
        .map_err(|error| LauncherError::storage(format!("无法打开 Windows 凭据存储：{error}")))?;
    let secret = serde_json::json!({
        "refreshToken": refresh_token,
        "accessToken": access_token,
        "uuid": profile_uuid,
        "xuid": profile_xuid,
    });
    entry
        .set_password(&secret.to_string())
        .map_err(|error| LauncherError::storage(format!("无法保存 Microsoft 凭据：{error}")))?;
    connection
        .execute(
            "UPDATE accounts SET safe_secret_ref=?1 WHERE id=?2",
            params![secret_ref, account_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut settings = get_settings(app.clone())?;
    settings.microsoft_client_id = Some(normalized_client_id);
    let _ = save_settings(app, settings)?;
    Ok(Account {
        id: account_id,
        account_type: "MICROSOFT".into(),
        display_name: profile_name,
        created_at,
        last_used_at: Some(chrono_like_timestamp()),
    })
}

#[tauri::command]
fn microsoft_login_available(app: AppHandle) -> Result<bool, LauncherError> {
    let embedded = option_env!("SH_MICROSOFT_CLIENT_ID")
        .unwrap_or_default()
        .trim();
    if !embedded.is_empty() {
        return Ok(true);
    }
    Ok(get_settings(app)?
        .microsoft_client_id
        .is_some_and(|value| !value.trim().is_empty()))
}

fn authlib_injector_cache_path() -> Result<PathBuf, LauncherError> {
    Ok(launcher_data_directory()?
        .join("cache")
        .join(format!("authlib-injector-{AUTHLIB_INJECTOR_VERSION}.jar")))
}

async fn download_authlib_injector_bytes(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<u8>, LauncherError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| LauncherError::validation(format!("下载外置登录组件失败：{error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(LauncherError::validation(format!(
            "下载外置登录组件失败（HTTP {status}）。"
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::validation(format!("读取外置登录组件失败：{error}")))?;
    if (bytes.len() as u64) < AUTHLIB_INJECTOR_MIN_BYTES {
        return Err(LauncherError::validation(
            "下载到的外置登录组件不完整，请稍后重试。",
        ));
    }
    Ok(bytes.to_vec())
}

async fn ensure_authlib_injector() -> Result<PathBuf, LauncherError> {
    let path = authlib_injector_cache_path()?;
    if let Ok(metadata) = fs::metadata(&path) {
        if metadata.len() >= AUTHLIB_INJECTOR_MIN_BYTES {
            return Ok(path);
        }
    }
    let client = shared_download_client()?;
    let mut bytes: Option<Vec<u8>> = None;
    let mut expected_sha256: Option<String> = None;
    if let Ok(response) = client
        .get("https://authlib-injector.yushi.moe/artifact/latest.json")
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(value) = response.json::<serde_json::Value>().await {
                if let Some(url) = value.get("download_url").and_then(|value| value.as_str()) {
                    expected_sha256 = value
                        .pointer("/checksums/sha256")
                        .and_then(|value| value.as_str())
                        .map(str::to_string);
                    if let Ok(downloaded) = download_authlib_injector_bytes(&client, url).await {
                        bytes = Some(downloaded);
                    }
                }
            }
        }
    }
    if bytes.is_none() {
        let github_url = format!(
            "https://github.com/yushijinhun/authlib-injector/releases/download/v{AUTHLIB_INJECTOR_VERSION}/authlib-injector-{AUTHLIB_INJECTOR_VERSION}.jar"
        );
        bytes = Some(download_authlib_injector_bytes(&client, &github_url).await?);
        expected_sha256 = None;
    }
    let bytes = bytes
        .ok_or_else(|| LauncherError::validation("无法下载外置登录组件，请检查网络后重试。"))?;
    if let Some(checksum) = expected_sha256 {
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if !digest.eq_ignore_ascii_case(&checksum) {
            return Err(LauncherError::validation(
                "外置登录组件校验失败，已停止安装以保护安全。",
            ));
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let temp = path.with_extension(format!("tmp-{}", unique_timestamp()));
    fs::write(&temp, &bytes).map_err(|error| LauncherError::storage(error.to_string()))?;
    fs::rename(&temp, &path).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(path)
}

fn normalize_authlib_api_root(value: &str) -> Result<String, LauncherError> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() || value.chars().count() > 200 {
        return Err(LauncherError::validation("外置登录地址无效。"));
    }
    let url = reqwest::Url::parse(value)
        .map_err(|_| LauncherError::validation("外置登录地址不是有效的网址。"))?;
    let https = url.scheme() == "https";
    let localhost = url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
    if !https && !localhost {
        return Err(LauncherError::validation(
            "外置登录地址必须是 https://；仅本机调试允许 http://localhost。",
        ));
    }
    Ok(value.to_string())
}

async fn refresh_external_token(
    api_root: &str,
    access_token: &str,
    client_token: &str,
) -> Result<Option<(String, String)>, LauncherError> {
    if client_token.is_empty() {
        return Ok(None);
    }
    let client = shared_download_client()?;
    let response = client
        .post(format!("{api_root}/authserver/refresh"))
        .json(&serde_json::json!({
            "accessToken": access_token,
            "clientToken": client_token,
        }))
        .send()
        .await
        .map_err(|error| LauncherError::validation(format!("外置登录服务器连接失败：{error}")))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|error| LauncherError::validation(format!("外置登录刷新响应无效：{error}")))?;
    let new_token = body
        .get("accessToken")
        .and_then(|value| value.as_str())
        .unwrap_or(access_token)
        .to_string();
    let new_client = body
        .get("clientToken")
        .and_then(|value| value.as_str())
        .unwrap_or(client_token)
        .to_string();
    Ok(Some((new_token, new_client)))
}

#[tauri::command]
async fn login_external(
    app: AppHandle,
    api_root: String,
    username: String,
    password: String,
) -> Result<Account, LauncherError> {
    let api_root = normalize_authlib_api_root(&api_root)?;
    let username = username.trim().to_string();
    if username.is_empty()
        || username.chars().count() > 64
        || username.chars().any(|character| character.is_control())
    {
        return Err(LauncherError::validation("请输入有效的外置登录用户名。"));
    }
    if password.is_empty() {
        return Err(LauncherError::validation("请输入外置登录密码。"));
    }
    let password = password.to_string();
    let client = shared_download_client()?;
    let response = client
        .post(format!("{api_root}/authserver/authenticate"))
        .json(&serde_json::json!({
            "agent": {"name": "Minecraft", "version": 1},
            "username": username,
            "password": password,
            "requestUser": true,
        }))
        .send()
        .await
        .map_err(|error| LauncherError::validation(format!("无法连接外置登录服务器：{error}")))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|error| LauncherError::validation(format!("外置登录返回内容无效：{error}")))?;
    if !status.is_success() {
        let message = body
            .get("errorMessage")
            .and_then(|value| value.as_str())
            .or_else(|| body.get("error").and_then(|value| value.as_str()))
            .unwrap_or("用户名或密码错误");
        return Err(LauncherError::validation(format!(
            "外置登录失败：{message}"
        )));
    }
    let access_token = body
        .get("accessToken")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::validation("外置登录返回缺少 accessToken。"))?
        .to_string();
    let client_token = body
        .get("clientToken")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("sh-{}", unique_timestamp()));
    let profile = body
        .get("selectedProfile")
        .ok_or_else(|| LauncherError::validation("该外置登录服务器没有返回游戏档案。"))?;
    let profile_name = profile
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| LauncherError::validation("外置登录返回的档案缺少名称。"))?;
    let profile_uuid = profile
        .get("id")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| LauncherError::validation("外置登录返回的档案缺少 UUID。"))?;
    if profile_name.is_empty()
        || profile_name.chars().count() > 16
        || profile_name.chars().any(|character| character.is_control())
    {
        return Err(LauncherError::validation("外置登录返回的玩家名称无效。"));
    }
    ensure_authlib_injector().await?;
    let connection = open_database(&app)?;
    let existing: Option<(i64, String)> = connection
        .query_row(
            "SELECT id, account_type FROM accounts
             WHERE minecraft_uuid=?1 OR (minecraft_uuid IS NULL AND display_name=?2)
             ORDER BY id LIMIT 1",
            params![profile_uuid, profile_name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    if let Some((_, existing_type)) = existing.as_ref() {
        if existing_type != "EXTERNAL" {
            return Err(LauncherError::validation(
                "已有同名的正版或离线账户，请先移除该账户，或更换外置登录用户名。",
            ));
        }
    }
    let created_at = chrono_like_timestamp();
    let account_id: i64 = if let Some((existing_id, _)) = existing {
        connection
            .execute(
                "UPDATE accounts SET account_type='EXTERNAL', last_used_at=?1, auth_server=?2 WHERE id=?3",
                params![created_at, api_root, existing_id],
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        existing_id
    } else {
        connection
            .execute(
                "INSERT INTO accounts (account_type, minecraft_uuid, display_name, created_at, last_used_at, auth_server)
                 VALUES ('EXTERNAL', ?1, ?2, ?3, ?3, ?4)",
                params![profile_uuid, profile_name, created_at, api_root],
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        connection.last_insert_rowid()
    };
    let secret_ref = format!("external-account-{account_id}");
    let entry = keyring::Entry::new("SH启动器", &secret_ref)
        .map_err(|error| LauncherError::storage(format!("无法打开 Windows 凭据存储：{error}")))?;
    let secret = serde_json::json!({
        "accessToken": access_token,
        "clientToken": client_token,
        "uuid": profile_uuid,
        "apiRoot": api_root,
    });
    entry
        .set_password(&secret.to_string())
        .map_err(|error| LauncherError::storage(format!("无法保存外置登录凭据：{error}")))?;
    connection
        .execute(
            "UPDATE accounts SET credential_ref=?1 WHERE id=?2",
            params![secret_ref, account_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(Account {
        id: account_id,
        account_type: "EXTERNAL".into(),
        display_name: profile_name,
        created_at: created_at.clone(),
        last_used_at: Some(created_at),
    })
}

#[tauri::command]
fn remove_account(app: AppHandle, account_id: i64) -> Result<(), LauncherError> {
    let connection = open_database(&app)?;
    let secret_ref: Option<String> = connection
        .query_row(
            "SELECT credential_ref FROM accounts WHERE id=?1",
            [account_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("账户不存在。"))?;
    if let Some(secret_ref) = secret_ref {
        if let Ok(entry) = keyring::Entry::new("SH启动器", &secret_ref) {
            let _ = entry.delete_credential();
        }
    }
    connection
        .execute("DELETE FROM accounts WHERE id=?1", [account_id])
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerEntry {
    id: i64,
    name: String,
    address: String,
    port: u16,
    description: String,
    created_at: String,
    last_connected_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerPing {
    reachable: bool,
    latency_ms: Option<u128>,
    error: Option<String>,
}

fn validate_server_name(value: &str) -> Result<String, LauncherError> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > 64
        || value
            .chars()
            .any(|character| character.is_control() || matches!(character, '\n' | '\r'))
    {
        return Err(LauncherError::validation(
            "服务器名称须为 1–64 个字符，且不能包含换行。",
        ));
    }
    Ok(value.to_string())
}

fn validate_server_address(value: &str) -> Result<String, LauncherError> {
    let value = value.trim().trim_end_matches(['/', '\\']);
    if value.is_empty() || value.chars().count() > 253 {
        return Err(LauncherError::validation(
            "服务器地址不能为空，且不能超过 253 个字符。",
        ));
    }
    if value.chars().any(|character| {
        character.is_whitespace() || character.is_control() || matches!(character, '/' | '\\')
    }) {
        return Err(LauncherError::validation(
            "服务器地址不能包含空格、控制字符或路径符号。",
        ));
    }
    // 端口单独填写；IPv6 允许使用 [::1] 或裸 IPv6，但不接受 host:port 混写
    if value.contains(':') && !value.starts_with('[') && value.matches(':').count() == 1 {
        return Err(LauncherError::validation(
            "请在“端口”栏单独填写端口，地址只填主机名或 IP。",
        ));
    }
    Ok(value.to_string())
}

fn validate_server_port(port: u16) -> Result<u16, LauncherError> {
    if port == 0 {
        return Err(LauncherError::validation("服务器端口须为 1–65535。"));
    }
    Ok(port)
}

fn read_server_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ServerEntry> {
    Ok(ServerEntry {
        id: row.get(0)?,
        name: row.get(1)?,
        address: row.get(2)?,
        port: row.get(3)?,
        description: row.get(4)?,
        created_at: row.get(5)?,
        last_connected_at: row.get(6)?,
    })
}

#[tauri::command]
fn list_servers(app: AppHandle) -> Result<Vec<ServerEntry>, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection
        .prepare(
            "SELECT id, name, address, port, description, created_at, last_connected_at
             FROM servers ORDER BY COALESCE(last_connected_at, created_at) DESC, id DESC",
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = statement
        .query_map([], read_server_row)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| LauncherError::storage(error.to_string()))
}

#[tauri::command]
fn add_server(
    app: AppHandle,
    name: String,
    address: String,
    port: Option<u16>,
    description: Option<String>,
) -> Result<ServerEntry, LauncherError> {
    let name = validate_server_name(&name)?;
    let address = validate_server_address(&address)?;
    let port = validate_server_port(port.unwrap_or(25565))?;
    let description = description
        .unwrap_or_default()
        .trim()
        .chars()
        .take(500)
        .collect::<String>();
    let connection = open_database(&app)?;
    let created_at = chrono_like_timestamp();
    connection
        .execute(
            "INSERT INTO servers (name, address, port, description, created_at, last_connected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![name, address, port, description, created_at],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let id = connection.last_insert_rowid();
    let mut statement = connection
        .prepare(
            "SELECT id, name, address, port, description, created_at, last_connected_at
             FROM servers WHERE id=?1",
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    statement
        .query_row([id], read_server_row)
        .map_err(|error| LauncherError::storage(error.to_string()))
}

#[tauri::command]
fn update_server(
    app: AppHandle,
    server_id: i64,
    name: String,
    address: String,
    port: Option<u16>,
    description: Option<String>,
) -> Result<ServerEntry, LauncherError> {
    let name = validate_server_name(&name)?;
    let address = validate_server_address(&address)?;
    let port = validate_server_port(port.unwrap_or(25565))?;
    let description = description
        .unwrap_or_default()
        .trim()
        .chars()
        .take(500)
        .collect::<String>();
    let connection = open_database(&app)?;
    let changed = connection
        .execute(
            "UPDATE servers SET name=?1, address=?2, port=?3, description=?4 WHERE id=?5",
            params![name, address, port, description, server_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if changed == 0 {
        return Err(LauncherError::validation("服务器不存在。"));
    }
    let mut statement = connection
        .prepare(
            "SELECT id, name, address, port, description, created_at, last_connected_at
             FROM servers WHERE id=?1",
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    statement
        .query_row([server_id], read_server_row)
        .map_err(|error| LauncherError::storage(error.to_string()))
}

#[tauri::command]
fn remove_server(app: AppHandle, server_id: i64) -> Result<(), LauncherError> {
    let connection = open_database(&app)?;
    connection
        .execute("DELETE FROM servers WHERE id=?1", [server_id])
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}

#[tauri::command]
async fn ping_server(address: String, port: u16) -> Result<ServerPing, LauncherError> {
    let address = validate_server_address(&address)?;
    let started = Instant::now();
    let lookup_address = address.trim_start_matches('[').trim_end_matches(']');
    let Ok(mut addrs) = tokio::net::lookup_host((lookup_address, port)).await else {
        return Ok(ServerPing {
            reachable: false,
            latency_ms: None,
            error: Some("无法解析服务器地址。".into()),
        });
    };
    let Some(addr) = addrs.next() else {
        return Ok(ServerPing {
            reachable: false,
            latency_ms: None,
            error: Some("服务器地址没有可连接的目标。".into()),
        });
    };
    match tokio::time::timeout(Duration::from_secs(4), tokio::net::TcpStream::connect(addr)).await {
        Ok(Ok(_)) => Ok(ServerPing {
            reachable: true,
            latency_ms: Some(started.elapsed().as_millis()),
            error: None,
        }),
        Ok(Err(error)) => Ok(ServerPing {
            reachable: false,
            latency_ms: None,
            error: Some(format!("无法连接：{error}")),
        }),
        Err(_) => Ok(ServerPing {
            reachable: false,
            latency_ms: None,
            error: Some("连接超时（4 秒）。".into()),
        }),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModpackArchive {
    id: i64,
    source_kind: String,
    file_path: Option<String>,
    project_id: Option<String>,
    file_name: String,
    name: Option<String>,
    version: Option<String>,
    game_version: Option<String>,
    loader_type: Option<String>,
    format: String,
    size_bytes: Option<i64>,
    instance_id: Option<i64>,
    instance_name: Option<String>,
    instance_status: Option<String>,
    imported_at: String,
}

fn read_modpack_archive_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModpackArchive> {
    Ok(ModpackArchive {
        id: row.get(0)?,
        source_kind: row.get(1)?,
        file_path: row.get(2)?,
        project_id: row.get(3)?,
        file_name: row.get(4)?,
        name: row.get(5)?,
        version: row.get(6)?,
        game_version: row.get(7)?,
        loader_type: row.get(8)?,
        format: row.get(9)?,
        size_bytes: row.get(10)?,
        instance_id: row.get(11)?,
        instance_name: row.get(12)?,
        instance_status: row.get(13)?,
        imported_at: row.get(14)?,
    })
}

#[tauri::command]
fn list_modpack_archives(app: AppHandle) -> Result<Vec<ModpackArchive>, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection
        .prepare(
            "SELECT a.id, a.source_kind, a.file_path, a.project_id, a.file_name, a.name,
                    a.version, a.game_version, a.loader_type, a.format, a.size_bytes,
                    a.instance_id, i.name AS instance_name, i.status AS instance_status,
                    a.imported_at
             FROM modpack_archives a
             LEFT JOIN instances i ON i.id = a.instance_id
             ORDER BY a.imported_at DESC, a.id DESC",
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = statement
        .query_map([], read_modpack_archive_row)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| LauncherError::storage(error.to_string()))
}

#[tauri::command]
fn record_modpack_archive(
    app: AppHandle,
    source_kind: String,
    file_path: Option<String>,
    project_id: Option<String>,
    file_name: String,
    name: Option<String>,
    version: Option<String>,
    game_version: Option<String>,
    loader_type: Option<String>,
    format: String,
    size_bytes: Option<i64>,
    instance_id: Option<i64>,
) -> Result<ModpackArchive, LauncherError> {
    let file_name = file_name.trim();
    if file_name.is_empty()
        || file_name.chars().count() > 300
        || file_name.chars().any(|character| character.is_control())
    {
        return Err(LauncherError::validation("整合包文件名无效。"));
    }
    let source_kind = match source_kind.as_str() {
        "local" | "modrinth" => source_kind,
        _ => return Err(LauncherError::validation("整合包来源类型无效。")),
    };
    let format = if format.trim().is_empty() {
        "zip".to_string()
    } else {
        format.trim().to_string()
    };
    let connection = open_database(&app)?;
    let existing: Option<i64> = if project_id.as_deref().is_some_and(|value| !value.is_empty()) {
        connection
            .query_row(
                "SELECT id FROM modpack_archives WHERE project_id=?1 AND instance_id IS ?2",
                params![project_id, instance_id],
                |row| row.get(0),
            )
            .ok()
    } else if let Some(path) = file_path.as_deref() {
        connection
            .query_row(
                "SELECT id FROM modpack_archives WHERE file_path=?1 AND instance_id IS ?2",
                params![path, instance_id],
                |row| row.get(0),
            )
            .ok()
    } else {
        None
    };
    let imported_at = chrono_like_timestamp();
    let archive_id = if let Some(existing_id) = existing {
        connection
            .execute(
                "UPDATE modpack_archives
                 SET source_kind=?1, file_path=?2, project_id=?3, file_name=?4, name=?5,
                     version=?6, game_version=?7, loader_type=?8, format=?9, size_bytes=?10,
                     instance_id=?11, imported_at=?12
                 WHERE id=?13",
                params![
                    source_kind,
                    file_path,
                    project_id,
                    file_name,
                    name,
                    version,
                    game_version,
                    loader_type,
                    format,
                    size_bytes,
                    instance_id,
                    imported_at,
                    existing_id
                ],
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        existing_id
    } else {
        connection
            .execute(
                "INSERT INTO modpack_archives (source_kind, file_path, project_id, file_name, name, version, game_version, loader_type, format, size_bytes, instance_id, imported_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    source_kind,
                    file_path,
                    project_id,
                    file_name,
                    name,
                    version,
                    game_version,
                    loader_type,
                    format,
                    size_bytes,
                    instance_id,
                    imported_at
                ],
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        connection.last_insert_rowid()
    };
    let mut statement = connection
        .prepare(
            "SELECT a.id, a.source_kind, a.file_path, a.project_id, a.file_name, a.name,
                    a.version, a.game_version, a.loader_type, a.format, a.size_bytes,
                    a.instance_id, i.name AS instance_name, i.status AS instance_status,
                    a.imported_at
             FROM modpack_archives a
             LEFT JOIN instances i ON i.id = a.instance_id
             WHERE a.id=?1",
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    statement
        .query_row([archive_id], read_modpack_archive_row)
        .map_err(|error| LauncherError::storage(error.to_string()))
}

#[tauri::command]
fn remove_modpack_archive(app: AppHandle, archive_id: i64) -> Result<(), LauncherError> {
    let connection = open_database(&app)?;
    connection
        .execute("DELETE FROM modpack_archives WHERE id=?1", [archive_id])
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}

fn validate_profile_name(value: &str) -> Result<(), LauncherError> {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && (3..=16).contains(&value.len())
    {
        Ok(())
    } else {
        Err(LauncherError::validation(
            "名称须为 3–16 位 ASCII 字母、数字或下划线。",
        ))
    }
}

fn validate_instance_field(value: &str, max: usize) -> Result<(), LauncherError> {
    fs_safe::validate_windows_filename(value)?;
    let forbidden = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    if value.is_empty()
        || value.len() > max
        || value
            .chars()
            .any(|character| forbidden.contains(&character))
    {
        Err(LauncherError::validation("实例名称或版本无效。"))
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Instance {
    id: i64,
    name: String,
    root_path: String,
    game_version: String,
    loader_type: String,
    memory_mb: i64,
    status: String,
    source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JavaRuntime {
    path: String,
    vendor: String,
    version: String,
    major_version: Option<u32>,
    architecture: String,
    is_64_bit: bool,
}

fn property_from_java_output(output: &str, key: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (candidate, value) = line.trim().split_once('=')?;
        (candidate.trim() == key).then(|| value.trim().to_string())
    })
}

fn java_major_version(version: &str) -> Option<u32> {
    let first = version.split(['.', '_', '-']).next()?.parse::<u32>().ok()?;
    if first == 1 {
        version.split('.').nth(1)?.parse().ok()
    } else {
        Some(first)
    }
}

fn collect_java_candidates() -> Vec<PathBuf> {
    let mut candidates = HashSet::new();
    if let Some(java_home) = std::env::var_os("JAVA_HOME") {
        candidates.insert(PathBuf::from(java_home).join("bin").join("java.exe"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            candidates.insert(directory.join("java.exe"));
        }
    }
    for root in [
        r"C:\Program Files\Java",
        r"C:\Program Files\Eclipse Adoptium",
        r"C:\Program Files\Microsoft",
    ] {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                candidates.insert(entry.path().join("bin").join("java.exe"));
            }
        }
    }
    if let Ok(runtime_root) = launcher_data_directory().map(|path| path.join("runtimes")) {
        if runtime_root.is_dir() {
            if let Ok(managed) = collect_files_named(&runtime_root, "java.exe", 8) {
                candidates.extend(managed);
            }
        }
    }
    let mut result: Vec<_> = candidates
        .into_iter()
        .filter(|path| path.is_file())
        .collect();
    result.sort();
    result
}

#[tauri::command]
fn detect_java_runtimes() -> Vec<JavaRuntime> {
    collect_java_candidates()
        .into_iter()
        .filter_map(|path| {
            let output = Command::new(&path)
                .args(["-XshowSettings:properties", "-version"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let version = property_from_java_output(&text, "java.version")
                .unwrap_or_else(|| "unknown".into());
            let architecture = property_from_java_output(&text, "sun.arch.data.model")
                .or_else(|| property_from_java_output(&text, "os.arch"))
                .unwrap_or_else(|| "unknown".into());
            Some(JavaRuntime {
                path: path.to_string_lossy().to_string(),
                vendor: property_from_java_output(&text, "java.vendor")
                    .unwrap_or_else(|| "unknown".into()),
                major_version: java_major_version(&version),
                version,
                is_64_bit: architecture == "64" || architecture.contains("64"),
                architecture,
            })
        })
        .collect()
}

fn inspect_java_runtime(path: &Path) -> Result<JavaRuntime, LauncherError> {
    let output = Command::new(path)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .map_err(|error| LauncherError::storage(format!("无法执行 Java：{error}")))?;
    if !output.status.success() {
        return Err(LauncherError::validation("Java 运行时自检失败。"));
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let version = property_from_java_output(&text, "java.version")
        .ok_or_else(|| LauncherError::validation("无法读取 Java 版本。"))?;
    let architecture = property_from_java_output(&text, "sun.arch.data.model")
        .or_else(|| property_from_java_output(&text, "os.arch"))
        .unwrap_or_else(|| "unknown".into());
    Ok(JavaRuntime {
        path: path.to_string_lossy().to_string(),
        vendor: property_from_java_output(&text, "java.vendor").unwrap_or_else(|| "unknown".into()),
        major_version: java_major_version(&version),
        version,
        is_64_bit: architecture == "64" || architecture.contains("64"),
        architecture,
    })
}

async fn fetch_managed_java_package(
    major: u32,
) -> Result<(String, String, u64, String), LauncherError> {
    if !matches!(major, 8 | 17 | 21 | 25) {
        return Err(LauncherError::validation(
            "仅支持安装 Minecraft 常用的 Java 8、17、21 或 25。",
        ));
    }
    if matches!(major, 17 | 21 | 25) {
        let link = format!("https://aka.ms/download-jdk/microsoft-jdk-{major}-windows-x64.zip");
        let checksum_link = format!("{link}.sha256sum.txt");
        let client = shared_download_client()?;
        let checksum_response = client
            .get(&checksum_link)
            .send()
            .await
            .map_err(|error| {
                LauncherError::storage(format!("读取 Microsoft OpenJDK 校验值失败：{error}"))
            })?
            .error_for_status()
            .map_err(|error| {
                LauncherError::storage(format!("Microsoft OpenJDK 校验服务返回错误：{error}"))
            })?;
        if checksum_response.url().host_str() != Some("download.visualstudio.microsoft.com") {
            return Err(LauncherError::validation(
                "Microsoft OpenJDK 校验地址不受信任。",
            ));
        }
        let checksum_text = checksum_response
            .text()
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let checksum = checksum_text
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if checksum.len() != 64 || !checksum.chars().all(|value| value.is_ascii_hexdigit()) {
            return Err(LauncherError::validation(
                "Microsoft OpenJDK SHA-256 校验值无效。",
            ));
        }
        let package_response = client
            .head(&link)
            .send()
            .await
            .map_err(|error| {
                LauncherError::storage(format!("读取 Microsoft OpenJDK 文件大小失败：{error}"))
            })?
            .error_for_status()
            .map_err(|error| {
                LauncherError::storage(format!("Microsoft OpenJDK 下载服务返回错误：{error}"))
            })?;
        if package_response.url().host_str() != Some("download.visualstudio.microsoft.com") {
            return Err(LauncherError::validation(
                "Microsoft OpenJDK 下载地址不受信任。",
            ));
        }
        let size = package_response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| LauncherError::storage("Microsoft OpenJDK 未提供文件大小。"))?;
        if size > 512 * 1024 * 1024 {
            return Err(LauncherError::validation(
                "Microsoft OpenJDK 压缩包超过安全限制。",
            ));
        }
        return Ok((
            link,
            checksum,
            size,
            format!("microsoft-jdk-{major}-windows-x64.zip"),
        ));
    }
    let url = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse"
    );
    let response = shared_download_client()?
        .get(&url)
        .send()
        .await
        .map_err(|error| LauncherError::storage(format!("读取 Temurin 元数据失败：{error}")))?
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("Temurin 元数据服务返回错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|size| size > 2 * 1024 * 1024)
    {
        return Err(LauncherError::validation("Temurin 元数据超过安全限制。"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(LauncherError::validation("Temurin 元数据超过安全限制。"));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(format!("Temurin 元数据无效：{error}")))?;
    let package = value
        .pointer("/0/binary/package")
        .ok_or_else(|| LauncherError::validation("未找到该 Java 版本的 Windows x64 JRE。"))?;
    let link = package
        .get("link")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("Temurin 元数据缺少下载地址。"))?;
    let parsed = reqwest::Url::parse(link)
        .map_err(|_| LauncherError::validation("Temurin 下载地址无效。"))?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || !parsed.path().starts_with("/adoptium/temurin")
    {
        return Err(LauncherError::validation(
            "Temurin 下载地址不是受信任的 Adoptium 发布地址。",
        ));
    }
    let checksum = package
        .get("checksum")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if checksum.len() != 64 || !checksum.chars().all(|value| value.is_ascii_hexdigit()) {
        return Err(LauncherError::validation("Temurin SHA-256 校验值无效。"));
    }
    let size = package
        .get("size")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| LauncherError::storage("Temurin 元数据缺少文件大小。"))?;
    if size > 512 * 1024 * 1024 {
        return Err(LauncherError::validation("Temurin 压缩包超过安全限制。"));
    }
    let name = package
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("temurin.zip");
    validate_instance_field(name, 240)?;
    Ok((link.to_string(), checksum, size, name.to_string()))
}

async fn download_sha256_file(
    url: &str,
    expected_sha256: &str,
    expected_size: u64,
    target: &Path,
) -> Result<(), LauncherError> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| LauncherError::validation("Java 下载地址无效。"))?;
    if target.is_file()
        && target
            .metadata()
            .map(|metadata| metadata.len() == expected_size)
            .unwrap_or(false)
        && sha256_file_sync(target)?.eq_ignore_ascii_case(expected_sha256)
    {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let part = target.with_extension("part");
    let mut resume_from = tokio::fs::metadata(&part)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if resume_from >= expected_size {
        let _ = tokio::fs::remove_file(&part).await;
        resume_from = 0;
    }
    let client = shared_download_client()?;
    let mut response = send_download_request(&client, &parsed, Some(resume_from)).await?;
    if resume_from > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        let _ = tokio::fs::remove_file(&part).await;
        resume_from = 0;
        response = send_download_request(&client, &parsed, None).await?;
    }
    let response = response
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("Java 下载服务返回错误：{error}")))?;
    let final_url = response.url();
    if final_url.scheme() != "https"
        || !matches!(
            final_url.host_str(),
            Some("github.com")
                | Some("objects.githubusercontent.com")
                | Some("release-assets.githubusercontent.com")
                | Some("download.visualstudio.microsoft.com")
        )
    {
        return Err(LauncherError::validation(
            "Java 下载重定向到了不受信任的地址。",
        ));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resume_from > 0)
        .truncate(resume_from == 0)
        .open(&part)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    if resume_from > 0 {
        let mut existing = tokio::fs::File::open(&part)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let count = existing
                .read(&mut buffer)
                .await
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
    }
    let mut downloaded = resume_from;
    loop {
        let next = tokio::time::timeout(Duration::from_secs(180), stream.next())
            .await
            .map_err(|_| {
                LauncherError::storage(
                    "Java 下载连续 180 秒没有收到数据。请检查网络后重试；已下载的部分会保留。",
                )
            })?;
        let Some(chunk) = next else { break };
        if download_cancel_flag().load(Ordering::Acquire) {
            return Err(LauncherError::storage("Java 下载已取消。"));
        }
        let chunk = chunk.map_err(|error| LauncherError::storage(error.to_string()))?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > expected_size || downloaded > 512 * 1024 * 1024 {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(LauncherError::validation(format!(
                "Java 下载大小超过清单值（已收到 {downloaded} 字节，清单为 {expected_size} 字节）。"
            )));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    file.flush()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if downloaded != expected_size
        || format!("{:x}", hasher.finalize()) != expected_sha256.to_ascii_lowercase()
    {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(LauncherError::validation(
            "Java 压缩包大小或 SHA-256 校验失败。",
        ));
    }
    tokio::fs::rename(part, target)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}

fn extract_managed_java(archive_path: &Path, destination: &Path) -> Result<PathBuf, LauncherError> {
    let limits = fs_safe::ArchiveLimits {
        max_entries: 100_000,
        max_total_uncompressed: 2 * 1024 * 1024 * 1024,
        max_single_file: 512 * 1024 * 1024,
        max_compression_ratio: 200.0,
        reject_symlinks: true,
    };
    let report = fs_safe::extract_zip_securely(archive_path, destination, &limits)?;
    log::info!(
        "Java 解压完成：entries={} files={} bytes={}",
        report.entries,
        report.files,
        report.bytes
    );
    collect_files_named(destination, "java.exe", 3)?
        .into_iter()
        .find(|path| {
            path.parent()
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("bin"))
        })
        .ok_or_else(|| LauncherError::validation("Java ZIP 中未找到 bin/java.exe。"))
}

fn collect_files_named(
    root: &Path,
    expected_name: &str,
    max_depth: usize,
) -> Result<Vec<PathBuf>, LauncherError> {
    let mut found = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        if depth > max_depth {
            continue;
        }
        for entry in
            fs::read_dir(&directory).map_err(|error| LauncherError::storage(error.to_string()))?
        {
            let entry = entry.map_err(|error| LauncherError::storage(error.to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if file_type.is_file()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(expected_name))
            {
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}

#[tauri::command]
async fn install_managed_java(major: u32) -> Result<JavaRuntime, LauncherError> {
    download_cancel_flag().store(false, Ordering::Release);
    let runtimes = launcher_data_directory()?.join("runtimes");
    let destination = runtimes.join(format!("java-{major}"));
    if destination.is_dir() {
        for candidate in collect_files_named(&destination, "java.exe", 8)? {
            if let Ok(runtime) = inspect_java_runtime(&candidate) {
                if runtime.major_version == Some(major) && runtime.is_64_bit {
                    return Ok(runtime);
                }
            }
        }
    }
    let (url, checksum, size, name) = fetch_managed_java_package(major).await?;
    let cache = runtimes.join(".downloads").join(name);
    download_sha256_file(&url, &checksum, size, &cache).await?;
    let staging = runtimes.join(format!(".java-{major}-{}-part", unique_timestamp()));
    let extracted_java = extract_managed_java(&cache, &staging)?;
    let relative_java = extracted_java
        .strip_prefix(&staging)
        .map_err(|_| LauncherError::storage("Java 解压路径越界。"))?
        .to_path_buf();
    if destination.exists() {
        fs::rename(
            &destination,
            runtimes.join(format!("java-{major}-backup-{}", unique_timestamp())),
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    fs::rename(&staging, &destination)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let runtime = inspect_java_runtime(&destination.join(relative_java))?;
    if runtime.major_version != Some(major) || !runtime.is_64_bit {
        return Err(LauncherError::validation(
            "安装后的 Java 版本或架构与请求不一致。",
        ));
    }
    Ok(runtime)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModInspection {
    file_name: String,
    loader_type: String,
    mod_id: Option<String>,
    provides: Vec<String>,
    name: Option<String>,
    version: Option<String>,
    sha256: String,
    file_size: u64,
    warnings: Vec<String>,
    game_version_requirements: Vec<String>,
    dependencies: Vec<String>,
    conflicts: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContentItem {
    id: i64,
    instance_id: i64,
    kind: String,
    file_name: String,
    hash: String,
    metadata_json: Option<String>,
    enabled: bool,
    source: String,
    installed_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModUpdateInfo {
    content_id: i64,
    project_id: String,
    installed_version: String,
    latest_version: String,
    update_available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RemovedContent {
    id: i64,
    backup_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupItem {
    kind: String,
    backup_name: String,
    original_name: String,
    size: u64,
}

#[tauri::command]
fn exit_launcher(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn hide_launcher_window(app: AppHandle) -> Result<(), LauncherError> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| LauncherError::storage("主窗口不存在。"))?;
    window
        .hide()
        .map_err(|error| LauncherError::storage(error.to_string()))
}

pub(crate) fn running_games() -> &'static Mutex<HashMap<i64, u32>> {
    static RUNNING_GAMES: OnceLock<Mutex<HashMap<i64, u32>>> = OnceLock::new();
    RUNNING_GAMES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[tauri::command]
fn terminate_game(instance_id: i64) -> Result<(), LauncherError> {
    let process_id = running_games()
        .lock()
        .map_err(|_| LauncherError::storage("无法读取游戏运行状态。"))?
        .get(&instance_id)
        .copied()
        .ok_or_else(|| LauncherError::validation("这套游戏当前没有正在运行的进程。"))?;
    #[cfg(target_os = "windows")]
    let status = Command::new("taskkill")
        .args(["/PID", &process_id.to_string(), "/T", "/F"])
        .status()
        .map_err(|error| LauncherError::storage(format!("无法结束游戏：{error}")))?;
    #[cfg(not(target_os = "windows"))]
    let status = Command::new("kill")
        .args(["-TERM", &process_id.to_string()])
        .status()
        .map_err(|error| LauncherError::storage(format!("无法结束游戏：{error}")))?;
    if !status.success() {
        return Err(LauncherError::storage(
            "系统没有成功结束游戏，请打开任务管理器后重试。",
        ));
    }
    Ok(())
}

pub(crate) fn stop_game(instance_id: i64) -> Result<(), LauncherError> {
    terminate_game(instance_id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModpackInspection {
    file_name: String,
    format: String,
    name: Option<String>,
    version: Option<String>,
    game_version: Option<String>,
    loader_type: Option<String>,
    mod_count: usize,
    override_count: usize,
    warnings: Vec<String>,
}

fn validate_loader_type(loader_type: &str) -> Result<(), LauncherError> {
    if matches!(
        loader_type,
        "vanilla" | "fabric" | "forge" | "neoforge" | "quilt"
    ) {
        Ok(())
    } else {
        Err(LauncherError::validation("不支持的模组加载器。"))
    }
}

fn safe_jar_file_name(path: &Path) -> Result<String, LauncherError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| LauncherError::validation("模组文件名无效。"))?;
    if name.is_empty()
        || !name.to_ascii_lowercase().ends_with(".jar")
        || name.contains(['/', '\\'])
        || name == "."
        || name == ".."
    {
        return Err(LauncherError::validation("模组文件名无效。"));
    }
    Ok(name.to_string())
}

fn sha256_file_sync(path: &Path) -> Result<String, LauncherError> {
    let mut file =
        fs::File::open(path).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn content_kind_directory(kind: &str) -> Result<&'static str, LauncherError> {
    match kind {
        "resourcepack" => Ok("resourcepacks"),
        "shaderpack" => Ok("shaderpacks"),
        _ => Err(LauncherError::validation("不支持的内容类型。")),
    }
}

fn inspect_content_archive(path: &Path, kind: &str) -> Result<(String, u64), LauncherError> {
    content_kind_directory(kind)?;
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("zip"))
    {
        return Err(LauncherError::validation("资源包和光影仅支持 .zip 文件。"));
    }
    let metadata = fs::metadata(path).map_err(|error| LauncherError::storage(error.to_string()))?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 * 1024 {
        return Err(LauncherError::validation("内容文件无效或超过 1 GB。"));
    }
    let file = fs::File::open(path).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| LauncherError::validation(format!("ZIP 无效：{error}")))?;
    if archive.len() > 100_000 {
        return Err(LauncherError::validation("ZIP 条目数超过安全限制。"));
    }
    let mut expanded = 0u64;
    let mut has_pack_metadata = false;
    let mut has_shader_directory = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        let normalized = entry.name().replace('\\', "/");
        safe_relative_download_path(&normalized)?;
        has_pack_metadata |= normalized.eq_ignore_ascii_case("pack.mcmeta");
        has_shader_directory |= normalized.to_ascii_lowercase().starts_with("shaders/");
        expanded = expanded.saturating_add(entry.size());
        if expanded > 4 * 1024 * 1024 * 1024 {
            return Err(LauncherError::validation("ZIP 解压后超过安全限制。"));
        }
        if entry.compressed_size() > 0 && entry.size() / entry.compressed_size().max(1) > 200 {
            return Err(LauncherError::validation("ZIP 压缩比异常。"));
        }
    }
    if kind == "resourcepack" && !has_pack_metadata {
        return Err(LauncherError::validation(
            "资源包 ZIP 根目录缺少 pack.mcmeta。",
        ));
    }
    if kind == "shaderpack" && !has_shader_directory {
        return Err(LauncherError::validation(
            "光影包 ZIP 根目录缺少 shaders/ 目录。",
        ));
    }
    Ok((sha256_file_sync(path)?, metadata.len()))
}

fn ensure_loader_compatible(instance_loader: &str, mod_loader: &str) -> Result<(), LauncherError> {
    validate_loader_type(instance_loader)?;
    if mod_loader == "unknown" {
        return Err(LauncherError::validation(
            "无法识别此 JAR 的加载器，不允许自动安装。",
        ));
    }
    if instance_loader == "vanilla" {
        return Err(LauncherError::validation(
            "Vanilla 实例不支持加载器模组，请创建 Fabric、Forge、NeoForge 或 Quilt 实例。",
        ));
    }
    if instance_loader != mod_loader {
        return Err(LauncherError::validation(format!(
            "加载器不兼容：此模组需要 {mod_loader}，目标实例使用 {instance_loader}。"
        )));
    }
    Ok(())
}

fn numeric_game_version(value: &str) -> Option<Vec<u64>> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for character in value.trim().chars() {
        if character.is_ascii_digit() {
            current.push(character);
        } else if !current.is_empty() {
            numbers.push(current.parse().ok()?);
            current.clear();
        } else if !matches!(character, '.' | '-' | '_' | '+') {
            break;
        }
    }
    if !current.is_empty() {
        numbers.push(current.parse().ok()?);
    }
    (!numbers.is_empty()).then_some(numbers)
}

fn compare_game_versions(left: &[u64], right: &[u64]) -> std::cmp::Ordering {
    let length = left.len().max(right.len());
    for index in 0..length {
        match left
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right.get(index).copied().unwrap_or(0))
        {
            std::cmp::Ordering::Equal => continue,
            ordering => return ordering,
        }
    }
    std::cmp::Ordering::Equal
}

fn game_version_matches(requirement: &str, actual: &str) -> bool {
    use std::cmp::Ordering;
    let requirement = requirement.trim();
    if requirement.is_empty()
        || matches!(
            requirement,
            "*" | "${minecraft_version}" | "${minecraft_version_range}"
        )
    {
        return true;
    }
    let Some(actual_numbers) = numeric_game_version(actual) else {
        return false;
    };
    if requirement.len() >= 2
        && matches!(requirement.as_bytes()[0], b'[' | b'(')
        && matches!(requirement.as_bytes()[requirement.len() - 1], b']' | b')')
    {
        let lower_inclusive = requirement.starts_with('[');
        let upper_inclusive = requirement.ends_with(']');
        let inner = &requirement[1..requirement.len() - 1];
        if !inner.contains(',') {
            return numeric_game_version(inner).is_some_and(|target| {
                compare_game_versions(&actual_numbers, &target) == Ordering::Equal
            });
        }
        let mut bounds = inner.splitn(2, ',');
        let lower = bounds.next().unwrap_or_default().trim();
        let upper = bounds.next().unwrap_or_default().trim();
        let lower_ok = lower.is_empty()
            || numeric_game_version(lower).is_some_and(|target| {
                let ordering = compare_game_versions(&actual_numbers, &target);
                ordering == Ordering::Greater || (lower_inclusive && ordering == Ordering::Equal)
            });
        let upper_ok = upper.is_empty()
            || numeric_game_version(upper).is_some_and(|target| {
                let ordering = compare_game_versions(&actual_numbers, &target);
                ordering == Ordering::Less || (upper_inclusive && ordering == Ordering::Equal)
            });
        return lower_ok && upper_ok;
    }
    if requirement.contains(char::is_whitespace) {
        return requirement
            .split_whitespace()
            .all(|part| game_version_matches(part, actual));
    }
    for (operator, accepted) in [
        (">=", &[Ordering::Greater, Ordering::Equal][..]),
        ("<=", &[Ordering::Less, Ordering::Equal][..]),
        (">", &[Ordering::Greater][..]),
        ("<", &[Ordering::Less][..]),
        ("=", &[Ordering::Equal][..]),
    ] {
        if let Some(target) = requirement
            .strip_prefix(operator)
            .and_then(numeric_game_version)
        {
            return accepted.contains(&compare_game_versions(&actual_numbers, &target));
        }
    }
    if requirement.contains('*') || requirement.to_ascii_lowercase().contains('x') {
        let prefix = requirement
            .split(['.', '-', '_'])
            .take_while(|part| *part != "*" && !part.eq_ignore_ascii_case("x"))
            .filter_map(|part| part.parse::<u64>().ok())
            .collect::<Vec<_>>();
        return !prefix.is_empty() && actual_numbers.starts_with(&prefix);
    }
    if let Some(base) = requirement.strip_prefix('~').and_then(numeric_game_version) {
        let mut upper = base.clone();
        let index = if upper.len() > 1 { 1 } else { 0 };
        upper[index] += 1;
        upper.truncate(index + 1);
        return compare_game_versions(&actual_numbers, &base) != Ordering::Less
            && compare_game_versions(&actual_numbers, &upper) == Ordering::Less;
    }
    numeric_game_version(requirement)
        .is_some_and(|target| compare_game_versions(&actual_numbers, &target) == Ordering::Equal)
}

fn ensure_game_version_compatible(
    game_version: &str,
    inspection: &ModInspection,
) -> Result<(), LauncherError> {
    if inspection.game_version_requirements.is_empty()
        || inspection
            .game_version_requirements
            .iter()
            .any(|requirement| game_version_matches(requirement, game_version))
    {
        return Ok(());
    }
    Err(LauncherError::validation(format!(
        "模组“{}”不支持 Minecraft {}；它要求的版本是 {}。请换到匹配的游戏配置。",
        inspection.name.as_deref().unwrap_or(&inspection.file_name),
        game_version,
        inspection.game_version_requirements.join(" 或 ")
    )))
}

fn json_requirement_strings(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(value) => vec![value.clone()],
        serde_json::Value::Array(values) => {
            values.iter().flat_map(json_requirement_strings).collect()
        }
        _ => Vec::new(),
    }
}

fn read_descriptor(
    archive: &mut zip::ZipArchive<fs::File>,
    name: &str,
) -> Result<Option<Vec<u8>>, LauncherError> {
    let Ok(mut entry) = archive.by_name(name) else {
        return Ok(None);
    };
    if entry.size() > 2 * 1024 * 1024 {
        return Err(LauncherError::validation(
            "模组 descriptor 超过安全大小限制。",
        ));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(Some(bytes))
}

fn inspect_mod_jar_path(path: &Path) -> Result<ModInspection, LauncherError> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("jar"))
    {
        return Err(LauncherError::validation("仅支持 .jar 模组文件。"));
    }
    let metadata = fs::metadata(path).map_err(|error| LauncherError::storage(error.to_string()))?;
    if !metadata.is_file() || metadata.len() > 512 * 1024 * 1024 {
        return Err(LauncherError::validation("模组文件无效或超过 512 MB。"));
    }
    let mut hash_file =
        fs::File::open(path).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = hash_file
            .read(&mut buffer)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let file = fs::File::open(path).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| LauncherError::validation(format!("JAR 结构无效：{error}")))?;
    if archive.len() > 20_000 {
        return Err(LauncherError::validation("JAR 条目数超过安全限制。"));
    }
    let mut total_uncompressed = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        if Path::new(entry.name()).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(LauncherError::validation("JAR 包含路径穿越条目。"));
        }
        total_uncompressed = total_uncompressed.saturating_add(entry.size());
        if total_uncompressed > 2 * 1024 * 1024 * 1024 {
            return Err(LauncherError::validation("JAR 解压后大小超过安全限制。"));
        }
        if entry.compressed_size() > 0 && entry.size() / entry.compressed_size().max(1) > 200 {
            return Err(LauncherError::validation(
                "JAR 压缩比异常，可能是 Zip Bomb。",
            ));
        }
    }
    let mut loader_type = "unknown".to_string();
    let mut mod_id = None;
    let mut provides = Vec::new();
    let mut display_name = None;
    let mut version = None;
    let mut dependencies = Vec::new();
    let mut conflicts = Vec::new();
    let mut game_version_requirements = Vec::new();
    if let Some(bytes) = read_descriptor(&mut archive, "fabric.mod.json")? {
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            LauncherError::validation(format!("Fabric descriptor 无效：{error}"))
        })?;
        loader_type = "fabric".into();
        mod_id = value
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        display_name = value
            .get("name")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        version = value
            .get("version")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        if let Some(required) = value.get("depends").and_then(|entry| entry.as_object()) {
            dependencies.extend(required.keys().cloned());
            if let Some(requirement) = required.get("minecraft") {
                game_version_requirements.extend(json_requirement_strings(requirement));
            }
        }
        if let Some(provided) = value.get("provides").and_then(|entry| entry.as_array()) {
            provides.extend(
                provided
                    .iter()
                    .filter_map(|entry| entry.as_str().map(str::to_string)),
            );
        }
        if let Some(blocked) = value.get("breaks").and_then(|entry| entry.as_object()) {
            conflicts.extend(blocked.keys().cloned());
        }
    } else if let Some(bytes) = read_descriptor(&mut archive, "quilt.mod.json")? {
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            LauncherError::validation(format!("Quilt descriptor 无效：{error}"))
        })?;
        loader_type = "quilt".into();
        mod_id = value
            .pointer("/quilt_loader/id")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        display_name = value
            .pointer("/quilt_loader/metadata/name")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        version = value
            .pointer("/quilt_loader/version")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        if let Some(required) = value
            .pointer("/quilt_loader/depends")
            .and_then(|entry| entry.as_array())
        {
            for entry in required {
                let dependency_id = entry
                    .as_str()
                    .or_else(|| entry.get("id").and_then(|value| value.as_str()));
                if let Some(dependency_id) = dependency_id {
                    dependencies.push(dependency_id.to_string());
                    if dependency_id == "minecraft" {
                        if let Some(versions) = entry.get("versions") {
                            game_version_requirements.extend(json_requirement_strings(versions));
                        }
                    }
                }
            }
        }
        if let Some(provided) = value
            .pointer("/quilt_loader/provides")
            .and_then(|entry| entry.as_array())
        {
            provides.extend(provided.iter().filter_map(|entry| {
                entry
                    .get("id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            }));
        }
    } else {
        for (descriptor, loader) in [
            ("META-INF/neoforge.mods.toml", "neoforge"),
            ("META-INF/mods.toml", "forge"),
        ] {
            if let Some(bytes) = read_descriptor(&mut archive, descriptor)? {
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| LauncherError::validation("Mod descriptor 不是有效 UTF-8。"))?;
                let value: toml::Value = toml::from_str(text).map_err(|error| {
                    LauncherError::validation(format!("Mod descriptor 无效：{error}"))
                })?;
                let mod_entries = value
                    .get("mods")
                    .and_then(|value| value.as_array())
                    .cloned()
                    .unwrap_or_default();
                let first = mod_entries.first();
                loader_type = loader.into();
                mod_id = first
                    .and_then(|value| value.get("modId"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                if let Some(primary) = mod_id.as_deref() {
                    for entry in mod_entries.iter().skip(1) {
                        if let Some(extra) = entry
                            .get("modId")
                            .and_then(|value| value.as_str())
                            .filter(|extra| !extra.eq_ignore_ascii_case(primary))
                        {
                            provides.push(extra.to_string());
                        }
                    }
                }
                display_name = first
                    .and_then(|value| value.get("displayName"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                version = first
                    .and_then(|value| value.get("version"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                for entry in &mod_entries {
                    let Some(id) = entry.get("modId").and_then(|value| value.as_str()) else {
                        continue;
                    };
                    if let Some(items) = value
                        .get("dependencies")
                        .and_then(|deps| deps.get(id))
                        .and_then(|entry| entry.as_array())
                    {
                        for dependency in items {
                            let Some(dependency_id) =
                                dependency.get("modId").and_then(|entry| entry.as_str())
                            else {
                                continue;
                            };
                            if dependency
                                .get("mandatory")
                                .and_then(|entry| entry.as_bool())
                                .unwrap_or(true)
                            {
                                dependencies.push(dependency_id.to_string());
                                if dependency_id == "minecraft" {
                                    if let Some(requirement) = dependency
                                        .get("versionRange")
                                        .and_then(|entry| entry.as_str())
                                    {
                                        game_version_requirements.push(requirement.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
        }
    }
    let warnings = if loader_type == "unknown" {
        vec!["未识别加载器 descriptor，不会自动安装。".into()]
    } else {
        Vec::new()
    };
    Ok(ModInspection {
        file_name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown.jar")
            .into(),
        loader_type,
        mod_id,
        provides,
        name: display_name,
        version,
        sha256: format!("{:x}", hasher.finalize()),
        file_size: metadata.len(),
        warnings,
        game_version_requirements,
        dependencies,
        conflicts,
    })
}

fn has_kotlinforforge_file(mods_directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(mods_directory) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        name.contains("kotlinforforge") || name.contains("kotlin-for-forge")
    })
}

fn missing_dependencies<'a, S: AsRef<str> + std::cmp::Eq + std::hash::Hash>(
    dependencies: impl IntoIterator<Item = &'a str>,
    installed_ids: &HashSet<S>,
    kotlin_forge_present: bool,
) -> BTreeSet<String> {
    let provided = [
        "minecraft",
        "java",
        "fabricloader",
        "fabric-loader",
        "quilt_loader",
        "quilt-loader",
        "forge",
        "neoforge",
    ];
    dependencies
        .into_iter()
        .map(|id| id.to_ascii_lowercase())
        .filter(|id| {
            !provided.contains(&id.as_str())
                && !(id == "kotlinforforge" && kotlin_forge_present)
                && !installed_ids
                    .iter()
                    .any(|installed| installed.as_ref() == id)
        })
        .collect()
}

fn installed_mod_ids<'a>(
    inspections: impl IntoIterator<Item = &'a ModInspection>,
) -> HashSet<String> {
    let mut ids = HashSet::new();
    for inspection in inspections {
        if let Some(id) = inspection.mod_id.as_deref() {
            ids.insert(id.to_ascii_lowercase());
        }
        for provided in &inspection.provides {
            ids.insert(provided.to_ascii_lowercase());
        }
    }
    ids
}

fn validate_instance_mods(
    root_path: &str,
    game_version: &str,
    loader_type: &str,
) -> Result<(), LauncherError> {
    if loader_type == "vanilla" {
        return Ok(());
    }
    let mods = PathBuf::from(root_path).join(".minecraft").join("mods");
    if !mods.is_dir() {
        return Ok(());
    }
    let kotlin_forge_present = has_kotlinforforge_file(&mods);
    let mut inspections = Vec::new();
    let mut problems = Vec::new();
    let entries = fs::read_dir(&mods)
        .map_err(|error| LauncherError::storage(format!("无法读取模组文件夹：{error}")))?;
    for entry in entries {
        let path = entry
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .path();
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|value| !value.eq_ignore_ascii_case("jar"))
        {
            continue;
        }
        match inspect_mod_jar_path(&path) {
            Ok(inspection) => {
                if inspection.loader_type == "unknown" {
                    // 没有模组元数据的 jar（纯库文件等）Forge 会忽略，不参与启动校验
                    continue;
                }
                if let Err(error) = ensure_loader_compatible(loader_type, &inspection.loader_type) {
                    problems.push(format!("{}：{}", inspection.file_name, error.message));
                } else if let Err(error) = ensure_game_version_compatible(game_version, &inspection)
                {
                    problems.push(error.message);
                }
                inspections.push(inspection);
            }
            Err(error) => problems.push(format!(
                "{}：{}",
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("未知文件"),
                error.message
            )),
        }
    }
    let installed_ids = installed_mod_ids(&inspections);
    let missing = missing_dependencies(
        inspections
            .iter()
            .flat_map(|inspection| inspection.dependencies.iter().map(|id| id.as_str())),
        &installed_ids,
        kotlin_forge_present,
    );
    if !missing.is_empty() {
        problems.push(format!(
            "缺少前置模组：{}",
            missing.into_iter().collect::<Vec<_>>().join("、")
        ));
    }
    if problems.is_empty() {
        return Ok(());
    }
    let hidden = problems.len().saturating_sub(10);
    problems.truncate(10);
    if hidden > 0 {
        problems.push(format!("另外还有 {hidden} 项不兼容"));
    }
    Err(LauncherError::validation(format!(
        "启动前的模组检查没有通过：\n- {}\n请换用匹配的游戏版本，或补齐前置模组后再启动。",
        problems.join("\n- ")
    )))
}

fn inspect_modpack_path(path: &Path) -> Result<ModpackInspection, LauncherError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !matches!(extension.to_ascii_lowercase().as_str(), "zip" | "mrpack") {
        return Err(LauncherError::validation("整合包仅支持 .mrpack 或 .zip。"));
    }
    let metadata = fs::metadata(path).map_err(|error| LauncherError::storage(error.to_string()))?;
    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 * 1024 {
        return Err(LauncherError::validation("整合包无效或超过 2 GB。"));
    }
    let file = fs::File::open(path).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| LauncherError::validation(format!("整合包 ZIP 无效：{error}")))?;
    if archive.len() > 100_000 {
        return Err(LauncherError::validation("整合包条目数超过安全限制。"));
    }
    let mut uncompressed = 0u64;
    let mut override_count = 0usize;
    let mut bundled_mods = 0usize;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        safe_relative_download_path(entry.name())?;
        uncompressed = uncompressed.saturating_add(entry.size());
        if uncompressed > 10 * 1024 * 1024 * 1024 {
            return Err(LauncherError::validation(
                "整合包解压后超过 10 GB 安全限制。",
            ));
        }
        if entry.compressed_size() > 0 && entry.size() / entry.compressed_size().max(1) > 200 {
            return Err(LauncherError::validation("整合包包含异常压缩比条目。"));
        }
        let normalized = entry.name().replace('\\', "/").to_ascii_lowercase();
        if normalized.starts_with("overrides/") || normalized.starts_with("client-overrides/") {
            override_count += 1;
        }
        if normalized.starts_with("mods/") && normalized.ends_with(".jar") {
            bundled_mods += 1;
        }
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| LauncherError::validation("整合包文件名无效。"))?
        .to_string();
    if let Some(bytes) = read_descriptor(&mut archive, "modrinth.index.json")? {
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| LauncherError::validation(format!("Modrinth index 无效：{error}")))?;
        let dependencies = value
            .get("dependencies")
            .and_then(|entry| entry.as_object())
            .ok_or_else(|| LauncherError::validation("Modrinth index 缺少 dependencies。"))?;
        let loader_type = ["fabric-loader", "quilt-loader", "neoforge", "forge"]
            .into_iter()
            .find(|key| dependencies.contains_key(*key))
            .map(|key| key.trim_end_matches("-loader").to_string());
        let files = value
            .get("files")
            .and_then(|entry| entry.as_array())
            .ok_or_else(|| LauncherError::validation("Modrinth index 缺少 files。"))?;
        for entry in files {
            let candidate = entry
                .get("path")
                .and_then(|item| item.as_str())
                .ok_or_else(|| LauncherError::validation("Modrinth 文件缺少路径。"))?;
            safe_relative_download_path(candidate)?;
        }
        return Ok(ModpackInspection {
            file_name,
            format: "modrinth".into(),
            name: value
                .get("name")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            version: value
                .get("versionId")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            game_version: dependencies
                .get("minecraft")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            loader_type,
            mod_count: files.len(),
            override_count,
            warnings: Vec::new(),
        });
    }
    if let Some(bytes) = read_descriptor(&mut archive, "mmc-pack.json")? {
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            LauncherError::validation(format!("MultiMC mmc-pack.json 无效：{error}"))
        })?;
        let components = value
            .get("components")
            .and_then(|entry| entry.as_array())
            .cloned()
            .unwrap_or_default();
        let mut game_version = None;
        let mut loader_type = None;
        for component in &components {
            let uid = component
                .get("uid")
                .and_then(|entry| entry.as_str())
                .unwrap_or_default();
            let version = component
                .get("version")
                .and_then(|entry| entry.as_str())
                .map(str::to_string);
            match uid {
                "net.minecraft" => game_version = version,
                "net.minecraftforge" => loader_type = Some("forge".to_string()),
                "net.neoforged" => loader_type = Some("neoforge".to_string()),
                "net.fabricmc.fabric-loader" => loader_type = Some("fabric".to_string()),
                "org.quiltmc.quilt-loader" => loader_type = Some("quilt".to_string()),
                _ => {}
            }
        }
        let mut mmc_mods = 0usize;
        let mut mmc_files = 0usize;
        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|error| LauncherError::validation(error.to_string()))?;
            if entry.is_dir() {
                continue;
            }
            let normalized = entry.name().replace('\\', "/").to_ascii_lowercase();
            if normalized.starts_with(".minecraft/") {
                mmc_files += 1;
                if normalized.starts_with(".minecraft/mods/") && normalized.ends_with(".jar") {
                    mmc_mods += 1;
                }
            }
        }
        return Ok(ModpackInspection {
            file_name,
            format: "mmc".into(),
            name: value
                .get("name")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            version: None,
            game_version,
            loader_type,
            mod_count: mmc_mods,
            override_count: mmc_files,
            warnings: vec![
                "MultiMC 整合包：导入后会创建对应实例，仍需安装基础游戏和加载器。".into(),
            ],
        });
    }
    if let Some(bytes) = read_descriptor(&mut archive, "modpack.json")? {
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            LauncherError::validation(format!("HMCL modpack.json 无效：{error}"))
        })?;
        let loader_type = value
            .get("addons")
            .and_then(|entry| entry.as_array())
            .and_then(|addons| {
                addons.iter().find_map(|addon| {
                    let id = addon
                        .get("id")
                        .and_then(|entry| entry.as_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    match id.as_str() {
                        "forge" | "minecraftforge" => Some("forge".to_string()),
                        "neoforge" | "net.neoforge" => Some("neoforge".to_string()),
                        "fabric" | "fabric-loader" => Some("fabric".to_string()),
                        "quilt" | "quilt-loader" => Some("quilt".to_string()),
                        _ => None,
                    }
                })
            });
        let mut hmcl_mods = 0usize;
        let mut hmcl_files = 0usize;
        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|error| LauncherError::validation(error.to_string()))?;
            if entry.is_dir() {
                continue;
            }
            let normalized = entry.name().replace('\\', "/").to_ascii_lowercase();
            if normalized.starts_with("minecraft/") {
                hmcl_files += 1;
                if normalized.starts_with("minecraft/mods/") && normalized.ends_with(".jar") {
                    hmcl_mods += 1;
                }
            }
        }
        return Ok(ModpackInspection {
            file_name,
            format: "hmcl".into(),
            name: value
                .get("name")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            version: value
                .get("version")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            game_version: value
                .get("gameVersion")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            loader_type,
            mod_count: hmcl_mods,
            override_count: hmcl_files,
            warnings: vec!["HMCL 整合包：导入后会创建对应实例并自动安装游戏与加载器。".into()],
        });
    }
    if read_descriptor(&mut archive, "mcbbs.packmeta")?.is_some() {
        let bytes = read_descriptor(&mut archive, "mcbbs.packmeta")?
            .or_else(|| {
                read_descriptor(&mut archive, "manifest.json")
                    .ok()
                    .flatten()
            })
            .ok_or_else(|| LauncherError::validation("MCBBS 整合包元数据缺失。"))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| LauncherError::validation(format!("MCBBS 元数据无效：{error}")))?;
        let game_version = value
            .pointer("/minecraft/version")
            .and_then(|entry| entry.as_str())
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("addons")
                    .and_then(|entry| entry.as_array())
                    .and_then(|addons| {
                        addons
                            .iter()
                            .find(|addon| addon.get("id").and_then(|v| v.as_str()) == Some("game"))
                            .and_then(|addon| addon.get("version").and_then(|v| v.as_str()))
                    })
                    .map(str::to_string)
            });
        let loader_id = value
            .pointer("/minecraft/modLoaders/0/id")
            .and_then(|entry| entry.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let loader_type = ["neoforge", "fabric", "quilt", "forge"]
            .into_iter()
            .find(|loader| loader_id.starts_with(loader))
            .map(str::to_string);
        let mut mcbbs_mods = 0usize;
        let mut mcbbs_files = 0usize;
        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|error| LauncherError::validation(error.to_string()))?;
            if entry.is_dir() {
                continue;
            }
            let normalized = entry.name().replace('\\', "/").to_ascii_lowercase();
            if normalized.starts_with("overrides/") {
                mcbbs_files += 1;
                if normalized.starts_with("overrides/mods/") && normalized.ends_with(".jar") {
                    mcbbs_mods += 1;
                }
            }
        }
        return Ok(ModpackInspection {
            file_name,
            format: "mcbbs".into(),
            name: value
                .get("name")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            version: None,
            game_version,
            loader_type,
            mod_count: mcbbs_mods,
            override_count: mcbbs_files,
            warnings: vec!["MCBBS 整合包：导入后会创建对应实例并自动安装游戏与加载器。".into()],
        });
    }
    if let Some(bytes) = read_descriptor(&mut archive, "manifest.json")? {
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            LauncherError::validation(format!("CurseForge manifest 无效：{error}"))
        })?;
        let loader_id = value
            .pointer("/minecraft/modLoaders/0/id")
            .and_then(|entry| entry.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let loader_type = ["neoforge", "fabric", "quilt", "forge"]
            .into_iter()
            .find(|loader| loader_id.starts_with(loader))
            .map(str::to_string);
        let count = value
            .get("files")
            .and_then(|entry| entry.as_array())
            .map_or(0, Vec::len);
        return Ok(ModpackInspection {
            file_name,
            format: "curseforge".into(),
            name: value
                .get("name")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            version: value
                .get("version")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            game_version: value
                .pointer("/minecraft/version")
                .and_then(|entry| entry.as_str())
                .map(str::to_string),
            loader_type,
            mod_count: count + bundled_mods,
            override_count,
            warnings: vec!["CurseForge 清单中的远程项目需要可用下载源；导入前会再次检查。".into()],
        });
    }
    Ok(ModpackInspection {
        file_name,
        format: "generic".into(),
        name: path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_string),
        version: None,
        game_version: None,
        loader_type: None,
        mod_count: bundled_mods,
        override_count,
        warnings: vec!["通用 ZIP 缺少标准整合包清单，只能按受控目录导入。".into()],
    })
}

#[tauri::command]
fn inspect_modpack(path: String) -> Result<ModpackInspection, LauncherError> {
    inspect_modpack_path(Path::new(&path))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportedModpack {
    instance: Instance,
    downloaded_files: usize,
    override_files: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportedLocalPack {
    instance_id: i64,
    imported_files: usize,
    imported_mods: usize,
    downloaded_remote_files: usize,
    unresolved_remote_files: usize,
    skipped_mods: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnlineProject {
    source: String,
    title_zh: Option<String>,
    description_zh: Option<String>,
    slug: Option<String>,
    loader_type: Option<String>,
    project_id: String,
    title: String,
    description: String,
    author: String,
    project_type: String,
    downloads: u64,
    icon_url: Option<String>,
    versions: Vec<String>,
    categories: Vec<String>,
}

fn validate_modrinth_project_id(value: &str) -> Result<(), LauncherError> {
    if (3..=64).contains(&value.len()) && value.chars().all(|value| value.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(LauncherError::validation("Modrinth 项目标识无效。"))
    }
}

async fn fetch_modrinth_json(url: reqwest::Url) -> Result<serde_json::Value, LauncherError> {
    if url.scheme() != "https"
        || url.host_str() != Some("api.modrinth.com")
        || !url.path().starts_with("/v2/")
    {
        return Err(LauncherError::validation("仅允许 Modrinth 官方 API。"));
    }
    let _permit = download_perf::download_concurrency()
        .metadata
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| LauncherError::storage("元数据并发控制异常。"))?;
    let response = send_download_request(&quick_http_client()?, &url, None)
        .await?
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("Modrinth API 返回错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|size| size > 4 * 1024 * 1024)
    {
        return Err(LauncherError::validation("Modrinth API 响应超过安全限制。"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(LauncherError::validation("Modrinth API 响应超过安全限制。"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(format!("Modrinth API 数据无效：{error}")))
}

#[tauri::command]
async fn search_modrinth_projects(
    query: String,
    project_type: String,
    game_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<OnlineProject>, LauncherError> {
    log::info!("搜索 Modrinth：query={query} type={project_type}");
    if query.chars().count() > 80 || !matches!(project_type.as_str(), "mod" | "modpack") {
        return Err(LauncherError::validation("搜索条件无效。"));
    }
    let mut facets = vec![vec![format!("project_type:{project_type}")]];
    if let Some(version) = game_version.filter(|value| !value.is_empty()) {
        validate_instance_field(&version, 64)?;
        facets.push(vec![format!("versions:{version}")]);
    }
    if project_type == "mod" {
        if let Some(loader) = loader.filter(|value| !value.is_empty()) {
            validate_loader_type(&loader)?;
            if loader == "vanilla" {
                return Ok(Vec::new());
            }
            facets.push(vec![format!("categories:{loader}")]);
        }
    }
    let mut url = reqwest::Url::parse("https://api.modrinth.com/v2/search")
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    url.query_pairs_mut()
        .append_pair("query", query.trim())
        .append_pair(
            "facets",
            &serde_json::to_string(&facets).unwrap_or_default(),
        )
        .append_pair("index", "downloads")
        .append_pair("limit", "20");
    let value = fetch_modrinth_json(url).await?;
    let hits = value
        .get("hits")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LauncherError::storage("Modrinth 搜索结果缺少 hits。"))?;
    let mut projects = hits
        .iter()
        .filter_map(|hit| {
            Some(OnlineProject {
                source: "modrinth".into(),
                title_zh: None,
                description_zh: None,
                slug: None,
                loader_type: None,
                project_id: hit.get("project_id")?.as_str()?.to_string(),
                title: hit.get("title")?.as_str()?.to_string(),
                description: hit.get("description")?.as_str()?.to_string(),
                author: hit.get("author")?.as_str()?.to_string(),
                project_type: hit.get("project_type")?.as_str()?.to_string(),
                downloads: hit
                    .get("downloads")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
                icon_url: hit
                    .get("icon_url")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                versions: hit
                    .get("versions")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                categories: hit
                    .get("display_categories")
                    .or_else(|| hit.get("categories"))
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    localize_titles(&mut projects);
    Ok(projects)
}

const CHINESE_DICTIONARY: &[(&str, &str)] = &[
    ("just enough items", "JEI 物品管理"),
    ("applied energistics", "应用能源 (Applied Energistics)"),
    ("immersive engineering", "沉浸工程 (Immersive Engineering)"),
    ("tinkers construct", "匠魂 (Tinkers Construct)"),
    ("twilight forest", "暮色森林 (Twilight Forest)"),
    ("industrial foregoing", "工业先锋 (Industrial Foregoing)"),
    ("draconic evolution", "龙之研究 (Draconic Evolution)"),
    ("refined storage", "精致存储 (Refined Storage)"),
    ("thermal expansion", "热力膨胀 (Thermal Expansion)"),
    ("alex's caves", "亚历克斯的洞穴 (Alex's Caves)"),
    ("alex's mobs", "亚历克斯的怪物 (Alex's Mobs)"),
    ("farmer's delight", "农夫乐事 (Farmer's Delight)"),
    ("minecolonies", "我的殖民地 (MineColonies)"),
    (
        "sophisticated backpacks",
        "精致背包 (Sophisticated Backpacks)",
    ),
    (
        "iron's spells 'n spellbooks",
        "铁咒书 (Iron's Spells 'n Spellbooks)",
    ),
    ("storage drawers", "存储抽屉 (Storage Drawers)"),
    ("mr crayfish's gun mod", "枪械工艺 (MrCrayfish's Gun Mod)"),
    ("create above and beyond", "机械动力：以上与以外"),
    (
        "enchantment descriptions",
        "附魔描述 (Enchantment Descriptions)",
    ),
    ("legendary tooltips", "传说品质提示 (Legendary Tooltips)"),
    ("simple voice chat", "简单语音聊天 (Simple Voice Chat)"),
    ("journeymap", "旅行地图 (JourneyMap)"),
    ("traveler's backpack", "旅者背包 (Traveler's Backpack)"),
    ("xaero's world map", "Xaero 大地图"),
    ("xaero's minimap", "Xaero 小地图"),
    ("inventory sorter", "背包整理 (Inventory Sorter)"),
    ("mouse tweaks", "鼠标手势 (Mouse Tweaks)"),
    ("dungeons enhanced", "增强地牢 (Dungeons Enhanced)"),
    ("deeper and darker", "更深更暗 (Deeper and Darker)"),
    ("born in chaos", "混沌重生 (Born in Chaos)"),
    ("vault hunters", "宝库猎人 (Vault Hunters)"),
    ("better minecraft", "Better MC"),
    ("all the mods", "All the Mods (ATM)"),
    ("skyfactory", "空岛工厂 (SkyFactory)"),
    ("stoneblock", "石头世界 (StoneBlock)"),
    ("enigmatica", "谜团 (Enigmatica)"),
    ("dawncraft", "黎明工艺 (DawnCraft)"),
    ("lucky block", "幸运方块 (Lucky Block)"),
    ("gravestone", "墓碑 (Gravestone)"),
    ("death chest", "死亡箱子 (Death Chest)"),
    ("iron chests", "铁箱子 (Iron Chests)"),
    ("ice and fire", "冰与火之歌 (Ice and Fire)"),
    ("blood magic", "血魔法 (Blood Magic)"),
    ("thaumcraft", "神秘时代 (Thaumcraft)"),
    ("galacticraft", "星系 (Galacticraft)"),
    ("mekanism", "通用机械 (Mekanism)"),
    ("botania", "植物魔法 (Botania)"),
    ("patchouli", "帕秋莉手册 (Patchouli)"),
    ("waystones", "路石 (Waystones)"),
    ("epic fight", "史诗战斗 (Epic Fight)"),
    ("better combat", "更好的战斗 (Better Combat)"),
    ("litematica", "投影 (Litematica)"),
    ("worldedit", "创世神 (WorldEdit)"),
    ("replay mod", "回放模组 (Replay Mod)"),
    ("architectury api", "Architectury API"),
    ("kotlin for forge", "Kotlin for Forge"),
    ("ferritecore", "FerriteCore 内存优化"),
    ("modernfix", "ModernFix 性能优化"),
    ("entity culling", "实体剔除 (Entity Culling)"),
    ("connectivity", "Connectivity 联机优化"),
    ("gecko lib", "GeckoLib 动画库"),
    ("item borders", "物品边框 (Item Borders)"),
    ("curios", "饰品栏 (Curios)"),
    ("optifine", "高清修复 (OptiFine)"),
    ("oculus", "眼窗光影 (Oculus)"),
    ("iris shaders", "鸢尾花光影 (Iris)"),
    ("embeddium", "Embeddium"),
    ("rubidium", "铷 (Rubidium)"),
    ("phosphor", "磷 (Phosphor)"),
    ("lithium", "锂 (Lithium)"),
    ("sodium", "钠 (Sodium)"),
    ("apple skin", "苹果皮 (Apple Skin)"),
    ("fabric api", "Fabric API"),
    ("clumps", "Clumps 经验球合并"),
    ("spark", "Spark 性能监测"),
    ("balm", "Balm 前置库"),
    ("the graveyard", "墓地 (The Graveyard)"),
    ("mowzie's mobs", "Mowzie 的怪物"),
    ("corpse", "死亡尸体 (Corpse)"),
    ("backpack", "背包 (Backpack)"),
    ("shaders", "光影 (Shaders)"),
    ("resource pack", "资源包"),
    ("texture pack", "材质包"),
    ("modpack", "整合包"),
    ("jei", "JEI 物品管理"),
    ("rei", "REI 物品管理"),
    ("emi", "EMI 物品管理"),
    ("tacz", "现代战争枪械 (TaCZ)"),
    ("create", "机械动力 (Create)"),
    ("essential", "Essential 联机模组"),
    ("cloth config", "Cloth Config 配置库"),
    ("forge config api", "Forge Config API"),
    ("fabric language kotlin", "Fabric 语言 Kotlin"),
    ("architectury", "Architectury API"),
    ("geckolib", "GeckoLib 动画库"),
    ("citadel", "城堡核心 (Citadel)"),
    ("mantle", "地幔前置 (Mantle)"),
    ("quark", "夸克 (Quark)"),
    ("supplementaries", "补充品 (Supplementaries)"),
    ("chipped", "Chipped 装饰方块"),
    ("handcrafted", "Handcrafted 手工家具"),
    ("macaw's", "Macaw 系列家具"),
    ("decorative blocks", "装饰方块"),
    ("charm", "魅力 (Charm)"),
    ("terralith", "Terralith 地形"),
    ("biomes o' plenty", "更多生物群系 (Biomes O' Plenty)"),
    ("oh the biomes we've gone", "我们踏足过的生物群系"),
    ("betterend", "更好的末地 (BetterEnd)"),
    ("betternether", "更好的下界 (BetterNether)"),
    ("blue skies", "蓝天 (Blue Skies)"),
    ("the bumblezone", "蜂巢维度 (The Bumblezone)"),
    ("tectonic", "Tectonic 地形"),
    ("caves & cliffs backport", "洞穴与山崖回溯"),
    ("when dungeons arise", "当遗迹浮现 (When Dungeons Arise)"),
    ("dungeons and taverns", "地牢与酒馆"),
    ("dungeon crawl", "地牢爬行"),
    ("cataclysm", "灾厄 (Cataclysm)"),
    ("goety", "巫术 (Goety)"),
    ("vampirism", "吸血鬼 (Vampirism)"),
    ("bewitchment", "巫术技艺 (Bewitchment)"),
    ("malum", "Malum 黑暗魔法"),
    ("ars nouveau", "新生魔艺 (Ars Nouveau)"),
    ("ars elemental", "元素魔艺"),
    ("occultism", "神秘学 (Occultism)"),
    ("spell engine", "咒语引擎"),
    ("mana and artifice", "魔导工艺"),
    ("pneumaticcraft", "气动工艺 (PneumaticCraft)"),
    ("immersive petroleum", "沉浸石油"),
    ("railcraft", "铁路工艺 (Railcraft)"),
    ("computercraft", "电脑 (ComputerCraft)"),
    ("opencomputers", "开放电脑 (OpenComputers)"),
    ("integrated dynamics", "集成动力"),
    ("laserio", "LaserIO 激光物流"),
    ("pipez", "Pipez 管道"),
    ("sophisticated storage", "精致存储 (Sophisticated Storage)"),
    ("functional storage", "功能存储"),
    ("tom's simple storage", "Tom 的简易存储"),
    ("expanded storage", "扩展存储"),
    ("ender storage", "末影存储"),
    ("explorer's compass", "探险家指南针"),
    ("nature's compass", "自然指南针"),
    ("antique atlas", "古地图册"),
    ("map atlas", "地图册"),
    ("spyglass improvements", "望远镜增强"),
    ("neat", "NEAT 血条"),
    ("torohealth", "Toro 血条"),
    ("shoulder surfing reloaded", "第三人称越肩视角"),
    ("falling leaves", "飘落树叶"),
    ("particle rain", "粒子雨"),
    ("ambient sounds", "环境音效"),
    ("sound physics remastered", "声音物理重制"),
    ("presence footstep", "脚步声"),
    ("first person model", "第一人称模型"),
    ("not enough animations", "更多动画"),
    ("3d skin layers", "3D 皮肤层"),
    ("zoomify", "缩放"),
    ("ok zoomer", "缩放"),
    ("accessories", "饰品 (Accessories)"),
    ("trinkets", "饰品 (Trinkets)"),
    ("elytra slot", "鞘翅栏"),
    ("back slot", "背部栏"),
    ("cosmetic armor", "时装盔甲"),
    ("tetra", "四艺 (Tetra)"),
    ("silent gear", "寂静装备"),
    ("silent gems", "寂静宝石"),
    ("construct's armory", "匠魂军械库"),
    ("materialis", "匠魂材料扩展"),
    ("jade", "玉 (Jade)"),
    ("wthit", "WTHIT 信息提示"),
    ("hwyla", "HWYLA 信息提示"),
    ("the one probe", "TOP 信息提示"),
    ("waila", "WAILA 信息提示"),
    ("just enough resources", "JE 资源信息"),
    ("inventory profiles next", "背包配置 Next"),
    ("inventory tweaks", "背包整理"),
    ("carry on", "搬起来 (Carry On)"),
    ("pick up notifier", "拾取提示"),
    ("starlight", "星光 (Starlight)"),
    ("canary", "金丝雀 (Canary)"),
    ("krypton", "氪 (Krypton)"),
    ("lazy language loader", "懒加载语言文件"),
    ("smooth boot", "平滑启动"),
    ("fastload", "快速加载"),
    ("chunky", "Chunky 预生成区块"),
    ("distanthorizons", "遥远地平线 (Distant Horizons)"),
    ("nvidium", "Nvidium 渲染"),
    ("sodium extra", "钠扩展"),
    ("optifabric", "OptiFabric 兼容"),
    ("betterfps", "BetterFPS"),
    ("friends and foes", "朋友与敌人"),
    ("naturalist", "博物学家 (Naturalist)"),
    ("ecologics", "生态学"),
    ("gardens of the dead", "死亡花园"),
    ("biomemakeover", "生物群系改造"),
    ("mutant beasts", "变异野兽"),
    ("savage and ravage", "野蛮与掠夺"),
    ("illagers wear armor", "灾厄村民穿盔甲"),
    ("enderman overhaul", "末影人重做"),
    ("legendary monsters", "传说怪物"),
    ("dragonmounts", "龙坐骑"),
    ("moredragoneggs", "更多龙蛋"),
    ("flywheel", "飞轮 (Flywheel)"),
    ("create addition", "机械动力扩展"),
    ("create crafts & additions", "机械动力：工艺与扩展"),
    ("steam 'n' rails", "机械动力：蒸汽与铁路"),
    ("copycats+", "机械动力：仿制方块"),
    ("ponder", "Ponder 教程机制"),
    ("serene seasons", "宁静四季"),
    ("seasonhud", "季节 HUD"),
    ("packing tape", "打包胶带"),
    ("little logistics", "小型物流"),
];

fn translation_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_translation_cache() {
    let Ok(directory) = launcher_data_directory() else {
        return;
    };
    let path = directory.join("cache").join("translations.json");
    let Ok(bytes) = fs::read(&path) else {
        return;
    };
    if bytes.len() > 2 * 1024 * 1024 {
        return;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    let Ok(mut cache) = translation_cache().lock() else {
        return;
    };
    if let Some(object) = value.as_object() {
        for (key, translated) in object {
            if let Some(translated) = translated.as_str() {
                cache.insert(key.clone(), translated.to_string());
            }
        }
    }
}

fn save_translation_cache() {
    let Ok(cache) = translation_cache().lock() else {
        return;
    };
    if cache.len() > 5000 {
        return;
    }
    let Ok(directory) = launcher_data_directory() else {
        return;
    };
    let path = directory.join("cache").join("translations.json");
    let _ = fs::create_dir_all(path.parent().unwrap_or(&directory));
    if let Ok(bytes) = serde_json::to_vec(&*cache) {
        if bytes.len() <= 2 * 1024 * 1024 {
            let _ = fs::write(path, bytes);
        }
    }
}

fn contains_cjk(text: &str) -> bool {
    text.chars()
        .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
}

fn dictionary_title(title: &str) -> Option<String> {
    let lower = title.to_lowercase();
    let mut entries = CHINESE_DICTIONARY.to_vec();
    entries.sort_by_key(|(keyword, _)| std::cmp::Reverse(keyword.len()));
    entries
        .into_iter()
        .find(|(keyword, _)| lower.contains(keyword))
        .map(|(_, translated)| translated.to_string())
}

async fn translate_text_mymemory(text: &str) -> Option<String> {
    let mut url = reqwest::Url::parse("https://api.mymemory.translated.net/get").ok()?;
    url.query_pairs_mut()
        .append_pair("q", text)
        .append_pair("langpair", "en|zh-CN");
    let client = quick_http_client().ok()?;
    let result = tokio::time::timeout(Duration::from_secs(6), async {
        let response = client.get(url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let value: serde_json::Value = response.json().await.ok()?;
        let translated = value
            .pointer("/responseData/translatedText")
            .and_then(|value| value.as_str())?
            .trim()
            .to_string();
        (!translated.is_empty()).then_some(translated)
    })
    .await
    .ok()
    .flatten()?;
    if !result.is_empty() {
        return Some(result);
    }
    translate_text_google(text).await
}

async fn translate_text_google(text: &str) -> Option<String> {
    let mut url =
        reqwest::Url::parse("https://translate.googleapis.com/translate_a/single").ok()?;
    url.query_pairs_mut()
        .append_pair("client", "gtx")
        .append_pair("sl", "auto")
        .append_pair("tl", "zh-CN")
        .append_pair("dt", "t")
        .append_pair("q", text);
    let client = quick_http_client().ok()?;
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let response = client.get(url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let value: serde_json::Value = response.json().await.ok()?;
        let translated = value
            .as_array()?
            .first()?
            .as_array()?
            .iter()
            .filter_map(|segment| {
                segment
                    .as_array()
                    .and_then(|parts| parts.first())
                    .and_then(|part| part.as_str())
            })
            .collect::<String>();
        (!translated.is_empty()).then_some(translated)
    })
    .await
    .ok()
    .flatten()?;
    Some(result)
}

fn localize_titles(projects: &mut [OnlineProject]) {
    static LOADED: AtomicBool = AtomicBool::new(false);
    if !LOADED.swap(true, Ordering::SeqCst) {
        load_translation_cache();
    }
    let Ok(cache) = translation_cache().lock() else {
        return;
    };
    for project in projects.iter_mut() {
        if !contains_cjk(&project.title) && project.title_zh.is_none() {
            project.title_zh =
                dictionary_title(&project.title).or_else(|| cache.get(&project.title).cloned());
        }
        if !contains_cjk(&project.description) && project.description_zh.is_none() {
            project.description_zh = cache.get(&project.description).cloned();
        }
    }
}

#[tauri::command]
async fn translate_search_text(text: String) -> Result<Option<String>, LauncherError> {
    log::info!("翻译搜索文本：len={}", text.chars().count());
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 500 {
        return Ok(None);
    }
    if contains_cjk(text) {
        return Ok(Some(text.to_string()));
    }
    static LOADED: AtomicBool = AtomicBool::new(false);
    if !LOADED.swap(true, Ordering::SeqCst) {
        load_translation_cache();
    }
    if let Some(cached) = translation_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(text).cloned())
    {
        return Ok(Some(cached));
    }
    if let Some(dictionary) = dictionary_title(text) {
        if let Ok(mut cache) = translation_cache().lock() {
            cache.insert(text.to_string(), dictionary.clone());
            save_translation_cache();
        }
        return Ok(Some(dictionary));
    }
    match translate_text_mymemory(text).await {
        Some(translated) => {
            if let Ok(mut cache) = translation_cache().lock() {
                cache.insert(text.to_string(), translated.clone());
                save_translation_cache();
            }
            Ok(Some(translated))
        }
        None => Ok(None),
    }
}

#[tauri::command]
async fn search_curseforge_projects(
    query: String,
    project_type: String,
    game_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<OnlineProject>, LauncherError> {
    log::info!("搜索 CurseForge：query={query} type={project_type}");
    if query.chars().count() > 80 || !matches!(project_type.as_str(), "mod" | "modpack") {
        return Err(LauncherError::validation("搜索条件无效。"));
    }
    if let Some(version) = game_version.as_deref().filter(|value| !value.is_empty()) {
        validate_instance_field(version, 64)?;
    }
    let mut url = reqwest::Url::parse("https://api.curse.tools/v1/cf/mods/search")
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if url.host_str() != Some("api.curse.tools") {
        return Err(LauncherError::validation("CurseForge 搜索地址不受信任。"));
    }
    {
        let mut query_pairs = url.query_pairs_mut();
        query_pairs
            .append_pair("gameId", "432")
            .append_pair("searchFilter", query.trim())
            .append_pair(
                "classId",
                if project_type == "modpack" {
                    "4471"
                } else {
                    "6"
                },
            )
            .append_pair("pageSize", "20")
            .append_pair("sortField", "2")
            .append_pair("sortOrder", "desc");
        if let Some(version) = game_version.as_deref().filter(|value| !value.is_empty()) {
            query_pairs.append_pair("gameVersion", version);
        }
        if project_type == "mod" {
            let loader_type = loader.as_deref().unwrap_or("").trim();
            if !loader_type.is_empty() && loader_type != "vanilla" {
                validate_loader_type(loader_type)?;
                let loader_id = match loader_type {
                    "forge" => "1",
                    "fabric" => "4",
                    "quilt" => "5",
                    "neoforge" => "6",
                    _ => "0",
                };
                query_pairs.append_pair("modLoaderType", loader_id);
            }
        }
    }
    let response = send_download_request(&quick_http_client()?, &url, None)
        .await?
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("CurseForge 搜索返回错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|size| size > 4 * 1024 * 1024)
    {
        return Err(LauncherError::validation(
            "CurseForge 搜索响应超过安全限制。",
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(LauncherError::validation(
            "CurseForge 搜索响应超过安全限制。",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(format!("CurseForge 搜索数据无效：{error}")))?;
    let items = value
        .get("data")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LauncherError::storage("CurseForge 搜索结果缺少 data。"))?;
    let mut projects = items
        .iter()
        .filter_map(|item| {
            let project_id = item.get("id")?.as_u64()?.to_string();
            let title = item.get("name")?.as_str()?.to_string();
            let description = item
                .get("summary")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let author = item
                .get("authors")
                .and_then(|value| value.as_array())
                .and_then(|authors| authors.first())
                .and_then(|author| author.get("name"))
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let downloads = item
                .get("downloadCount")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let icon_url = item
                .get("logo")
                .and_then(|value| value.get("url"))
                .or_else(|| item.get("logo").and_then(|value| value.get("thumbnailUrl")))
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let mut versions = Vec::new();
            let mut seen_versions = HashSet::new();
            if let Some(indexes) = item
                .get("latestFilesIndexes")
                .and_then(|value| value.as_array())
            {
                for index in indexes {
                    if let Some(version) = index.get("gameVersion").and_then(|value| value.as_str())
                    {
                        if seen_versions.insert(version.to_string()) && versions.len() < 8 {
                            versions.push(version.to_string());
                        }
                    }
                }
            }
            let categories = item
                .get("categories")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|value| {
                            value
                                .get("name")
                                .and_then(|v| v.as_str())
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(OnlineProject {
                source: "curseforge".into(),
                title_zh: None,
                description_zh: None,
                slug: item
                    .get("slug")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                loader_type: item
                    .get("latestFilesIndexes")
                    .and_then(|value| value.as_array())
                    .and_then(|indexes| {
                        indexes.iter().find_map(|index| {
                            match index.get("modLoader").and_then(|value| value.as_i64()) {
                                Some(1) => Some("forge".to_string()),
                                Some(4) => Some("fabric".to_string()),
                                Some(5) => Some("quilt".to_string()),
                                Some(6) => Some("neoforge".to_string()),
                                _ => None,
                            }
                        })
                    }),
                project_id,
                title,
                description,
                author,
                project_type: project_type.clone(),
                downloads,
                icon_url,
                versions,
                categories,
            })
        })
        .collect::<Vec<_>>();
    localize_titles(&mut projects);
    Ok(projects)
}

async fn fetch_project_versions(
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
) -> Result<Vec<serde_json::Value>, LauncherError> {
    validate_modrinth_project_id(project_id)?;
    let mut url = reqwest::Url::parse(&format!(
        "https://api.modrinth.com/v2/project/{project_id}/version"
    ))
    .map_err(|error| LauncherError::storage(error.to_string()))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("include_changelog", "false");
        if let Some(version) = game_version {
            query.append_pair(
                "game_versions",
                &serde_json::to_string(&[version]).unwrap_or_default(),
            );
        }
        if let Some(loader) = loader {
            query.append_pair(
                "loaders",
                &serde_json::to_string(&[loader]).unwrap_or_default(),
            );
        }
    }
    let versions = fetch_modrinth_json(url).await?;
    versions
        .as_array()
        .cloned()
        .ok_or_else(|| LauncherError::storage("Modrinth 版本结果无效。"))
}

fn pick_best_modrinth_version(versions: &[serde_json::Value]) -> Option<serde_json::Value> {
    let mut best: Option<&serde_json::Value> = None;
    for candidate in versions {
        let candidate_release = candidate
            .get("version_type")
            .and_then(|value| value.as_str())
            == Some("release");
        let candidate_date = candidate
            .get("date_published")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let candidate_number = candidate
            .get("version_number")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let better = match best {
            None => true,
            Some(current) => {
                let current_release =
                    current.get("version_type").and_then(|value| value.as_str()) == Some("release");
                let current_date = current
                    .get("date_published")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let current_number = current
                    .get("version_number")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                if candidate_release != current_release {
                    candidate_release
                } else if candidate_date != current_date {
                    candidate_date > current_date
                } else {
                    candidate_number > current_number
                }
            }
        };
        if better {
            best = Some(candidate);
        }
    }
    best.cloned()
}

/// 按“当前版本 + 当前加载器”优先逐级放宽匹配，尽量找到可用的 Modrinth 版本。
const MOD_LOADER_FAMILIES: [&str; 4] = ["fabric", "quilt", "forge", "neoforge"];

fn version_supports_mod_loader(version: &serde_json::Value) -> bool {
    let Some(loaders) = version.get("loaders").and_then(|value| value.as_array()) else {
        return true;
    };
    if loaders.is_empty() {
        return true;
    }
    loaders.iter().any(|loader| {
        loader.as_str().is_some_and(|name| {
            MOD_LOADER_FAMILIES
                .iter()
                .any(|family| name.eq_ignore_ascii_case(family))
        })
    })
}

async fn modrinth_best_version(
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
) -> Result<serde_json::Value, LauncherError> {
    validate_modrinth_project_id(project_id)?;
    let mut attempts = vec![(game_version, loader)];
    if loader.is_some() {
        attempts.push((None, loader));
    }
    if game_version.is_some() {
        attempts.push((game_version, None));
    }
    attempts.push((None, None));
    for (version, loaders) in attempts {
        let versions = fetch_project_versions(project_id, version, loaders).await?;
        // 放宽到“任意加载器”时，只保留真正的单机模组加载器版本，
        // 避免误选 Spigot/Paper 等服务端插件版本。
        let candidates: Vec<_> = if loaders.is_none() {
            versions
                .into_iter()
                .filter(version_supports_mod_loader)
                .collect()
        } else {
            versions
        };
        if let Some(best) = pick_best_modrinth_version(&candidates) {
            return Ok(best);
        }
    }
    Err(LauncherError::validation(format!(
        "{project_id} 没有与当前实例兼容的模组版本；若该模组仅支持 Spigot/Paper 等服务端插件平台，则无法在单机模组实例中自动安装。"
    )))
}

async fn modrinth_primary_file_with_loaders(
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
    extension: &str,
) -> Result<(String, String, String, u64, Vec<String>), LauncherError> {
    let selected = modrinth_best_version(project_id, game_version, loader).await?;
    let loaders = selected
        .get("loaders")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let files = selected
        .get("files")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LauncherError::storage("Modrinth 版本缺少文件。"))?;
    let matches_extension = |file: &&serde_json::Value| {
        file.get("filename")
            .and_then(|value| value.as_str())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(extension))
    };
    let file = files
        .iter()
        .filter(matches_extension)
        .find(|file| file.get("primary").and_then(|value| value.as_bool()) == Some(true))
        .or_else(|| files.iter().find(matches_extension))
        .ok_or_else(|| LauncherError::validation("Modrinth 版本没有可安装的主文件。"))?;
    let filename = file
        .get("filename")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("Modrinth 文件名缺失。"))?;
    validate_instance_field(filename, 240)?;
    let url = file
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("Modrinth 下载地址缺失。"))?;
    validate_resource_url(url)?;
    let sha1 = file
        .pointer("/hashes/sha1")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("Modrinth 文件缺少 SHA-1。"))?;
    let size = file
        .get("size")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| LauncherError::storage("Modrinth 文件大小缺失。"))?;
    Ok((
        url.to_string(),
        sha1.to_string(),
        filename.to_string(),
        size,
        loaders,
    ))
}

async fn modrinth_primary_file(
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
    extension: &str,
) -> Result<(String, String, String, u64), LauncherError> {
    let (url, sha1, filename, size, _) =
        modrinth_primary_file_with_loaders(project_id, game_version, loader, extension).await?;
    Ok((url, sha1, filename, size))
}

async fn modrinth_compatible_version(
    project_id: &str,
    game_version: &str,
    loader: &str,
) -> Result<serde_json::Value, LauncherError> {
    modrinth_best_version(project_id, Some(game_version), Some(loader)).await
}

fn collect_modrinth_dependency_order<'a>(
    project_id: String,
    game_version: &'a str,
    loader: &'a str,
    visiting: &'a mut HashSet<String>,
    visited: &'a mut HashSet<String>,
    order: &'a mut Vec<String>,
) -> futures_util::future::BoxFuture<'a, Result<(), LauncherError>> {
    Box::pin(async move {
        if visited.contains(&project_id) {
            return Ok(());
        }
        if !visiting.insert(project_id.clone()) {
            // 出现循环依赖（例如 A 依赖 B、B 又依赖 A）时，A 已经在展开队列中，
            // 直接跳过即可，避免把“互相依赖”误判为致命错误。
            return Ok(());
        }
        let version = modrinth_compatible_version(&project_id, game_version, loader).await?;
        if let Some(dependencies) = version
            .get("dependencies")
            .and_then(|value| value.as_array())
        {
            for dependency in dependencies {
                if dependency
                    .get("dependency_type")
                    .and_then(|value| value.as_str())
                    != Some("required")
                {
                    continue;
                }
                let dependency_project = if let Some(project) = dependency
                    .get("project_id")
                    .and_then(|value| value.as_str())
                {
                    Some(project.to_string())
                } else if let Some(version_id) = dependency
                    .get("version_id")
                    .and_then(|value| value.as_str())
                {
                    validate_modrinth_project_id(version_id)?;
                    let url = reqwest::Url::parse(&format!(
                        "https://api.modrinth.com/v2/version/{version_id}"
                    ))
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
                    fetch_modrinth_json(url)
                        .await?
                        .get("project_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                } else {
                    None
                };
                if let Some(dependency_project) = dependency_project {
                    collect_modrinth_dependency_order(
                        dependency_project,
                        game_version,
                        loader,
                        visiting,
                        visited,
                        order,
                    )
                    .await?;
                }
            }
        }
        visiting.remove(&project_id);
        visited.insert(project_id.clone());
        order.push(project_id);
        Ok(())
    })
}

async fn install_single_modrinth_mod(
    app: &AppHandle,
    instance_id: i64,
    project_id: &str,
    game_version: &str,
    loader: &str,
) -> Result<ContentItem, LauncherError> {
    {
        let connection = open_database(app)?;
        let mut statement = connection
            .prepare("SELECT id,instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at FROM content_items WHERE instance_id=?1 AND kind='mod'")
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let items = statement
            .query_map([instance_id], content_item_from_row)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        for item in items.flatten() {
            let is_same_project = item
                .metadata_json
                .as_deref()
                .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
                .and_then(|value| {
                    value
                        .get("modrinthProjectId")
                        .and_then(|entry| entry.as_str())
                        .map(str::to_string)
                })
                .is_some_and(|installed_project| installed_project == project_id);
            if is_same_project {
                return Ok(item);
            }
        }
    }
    let (url, sha1, filename, size) =
        modrinth_primary_file(project_id, Some(game_version), Some(loader), ".jar").await?;
    let cache = launcher_data_directory()?
        .join("cache")
        .join("modrinth")
        .join(format!("{}-{filename}", &sha1[..12]));
    download_verified_file(app, instance_id, &url, &sha1, Some(size), &cache).await?;
    let info = inspect_mod_jar_path(&cache)?;
    if let Some(mod_id) = info.mod_id.as_deref() {
        let connection = open_database(app)?;
        let mut statement = connection
            .prepare("SELECT id,instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at FROM content_items WHERE instance_id=?1 AND kind='mod'")
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let items = statement
            .query_map([instance_id], content_item_from_row)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        for item in items.flatten() {
            let installed_mod_id = item
                .metadata_json
                .as_deref()
                .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
                .and_then(|value| {
                    value
                        .get("modId")
                        .and_then(|entry| entry.as_str())
                        .map(str::to_string)
                });
            if installed_mod_id.is_some_and(|installed| installed.eq_ignore_ascii_case(mod_id)) {
                return Ok(item);
            }
        }
    }
    let mut item = install_mod(
        app.clone(),
        instance_id,
        cache.to_string_lossy().to_string(),
    )?;
    let connection = open_database(app)?;
    let mut metadata = item
        .metadata_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "modrinthProjectId".into(),
            serde_json::Value::String(project_id.to_string()),
        );
        object.insert(
            "modrinthSha1".into(),
            serde_json::Value::String(sha1.clone()),
        );
    }
    let metadata_json = metadata.to_string();
    connection
        .execute(
            "UPDATE content_items SET source='modrinth',metadata_json=?1 WHERE id=?2",
            params![metadata_json, item.id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    item.source = "modrinth".into();
    item.metadata_json = Some(metadata.to_string());
    connection
        .execute(
            "INSERT INTO content_provenance(content_id, provider, project_id, version_id, file_id, source_url, sha1, sha256, installed_at)
             VALUES(?1, 'modrinth', ?2, NULL, NULL, ?3, ?4, ?5, ?6)
             ON CONFLICT(content_id) DO UPDATE SET
                provider='modrinth', project_id=excluded.project_id, source_url=excluded.source_url,
                sha1=excluded.sha1, sha256=excluded.sha256, installed_at=excluded.installed_at",
            params![
                item.id,
                project_id,
                url,
                sha1,
                info.sha256,
                item.installed_at
            ],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(item)
}

#[tauri::command]
async fn install_modrinth_mod(
    app: AppHandle,
    instance_id: i64,
    project_id: String,
) -> Result<ContentItem, LauncherError> {
    let connection = open_database(&app)?;
    let (game_version, loader): (String, String) = connection
        .query_row(
            "SELECT game_version, loader_type FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    drop(connection);
    if loader == "vanilla" {
        return Err(LauncherError::validation(
            "Vanilla 实例不能安装加载器模组。",
        ));
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut install_order = Vec::new();
    collect_modrinth_dependency_order(
        project_id.clone(),
        &game_version,
        &loader,
        &mut visiting,
        &mut visited,
        &mut install_order,
    )
    .await?;

    let mut requested_item = None;
    for dependency_project_id in install_order {
        let item = install_single_modrinth_mod(
            &app,
            instance_id,
            &dependency_project_id,
            &game_version,
            &loader,
        )
        .await?;
        if dependency_project_id == project_id {
            requested_item = Some(item);
        }
    }
    requested_item.ok_or_else(|| LauncherError::storage("模组安装结果丢失。"))
}

pub(crate) async fn install_managed_mod(
    app: AppHandle,
    instance_id: i64,
    project_id: String,
) -> Result<ContentItem, LauncherError> {
    install_modrinth_mod(app, instance_id, project_id).await
}

fn modrinth_slug_candidates(value: &str) -> Vec<String> {
    let lower = value.to_ascii_lowercase();
    let mut candidates = vec![lower.clone()];
    let hyphenated = lower.replace('_', "-");
    if hyphenated != lower {
        candidates.push(hyphenated);
    }
    candidates
}

async fn resolve_modrinth_project_id(input: &str) -> Result<String, LauncherError> {
    if input.is_empty()
        || input.len() > 64
        || !input
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || value == '_' || value == '-')
    {
        return Err(LauncherError::validation(format!(
            "前置模组标识无效：{input}"
        )));
    }
    // 可信别名映射：这些 modId 与 Modrinth slug 不一致，禁止再靠 slug 猜测。
    // 每个条目都来自官方项目页核对的 project_id。
    let trusted_aliases: &[(&str, &str)] = &[
        ("kotlinforforge", "ordsPcFz"),
        ("bookshelf", "uy4Cnpcm"),
        ("prism", "1OE8wbN0"),
        ("alexscaves", "U6GY0xp0"),
        ("irons_spellbooks", "s4OWxYQQ"),
        ("tacz", "SzzJttH8"),
        ("expandability", "X5dUUm4k"),
        ("fzzy_config", "hYykXjDp"),
        ("l2library", "4Vh3BQ3F"),
        ("goety", "4ZVIxU8x"),
    ];
    let normalized_input = input.to_ascii_lowercase().replace('_', "-");
    if let Some((_, project_id)) = trusted_aliases
        .iter()
        .find(|(key, _)| *key == input.to_ascii_lowercase() || *key == normalized_input)
    {
        return Ok((*project_id).to_string());
    }
    // SHA-1 反向查项目：Modrinth 官方 version_file 端点，属于可信 hash 身份。
    if input.len() == 40 && input.chars().all(|value| value.is_ascii_hexdigit()) {
        if let Some(project_id) = modrinth_project_by_hash(input).await {
            return Ok(project_id);
        }
    }
    let candidates = modrinth_slug_candidates(input);
    for candidate in &candidates {
        if candidate.is_empty()
            || candidate.len() > 64
            || !candidate
                .chars()
                .all(|value| value.is_ascii_alphanumeric() || value == '-')
        {
            continue;
        }
        let url = reqwest::Url::parse(&format!("https://api.modrinth.com/v2/project/{candidate}"))
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if let Ok(value) = fetch_modrinth_json(url).await {
            if value.get("project_type").and_then(|value| value.as_str()) == Some("mod") {
                if let Some(project_id) = value.get("id").and_then(|value| value.as_str()) {
                    if (3..=64).contains(&project_id.len())
                        && project_id
                            .chars()
                            .all(|value| value.is_ascii_alphanumeric())
                    {
                        return Ok(project_id.to_string());
                    }
                }
            }
        }
    }
    for candidate in &candidates {
        let mut url = reqwest::Url::parse("https://api.modrinth.com/v2/search")
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        url.query_pairs_mut()
            .append_pair("query", candidate)
            .append_pair(
                "facets",
                &serde_json::to_string(&vec![vec!["project_type:mod"]])
                    .unwrap_or_else(|_| "[]".into()),
            )
            .append_pair("limit", "20");
        let Ok(value) = fetch_modrinth_json(url).await else {
            continue;
        };
        let Some(hits) = value.get("hits").and_then(|value| value.as_array()) else {
            continue;
        };
        let matches = exact_candidate_matches(candidate, hits);
        if matches.len() == 1 {
            return Ok(matches[0].clone());
        }
        if matches.len() > 1 {
            return Err(LauncherError::validation(format!(
                "“{input}”在 Modrinth 存在多个同名项目，无法安全自动安装。候选：{}。请手动选择。",
                matches.join("、")
            )));
        }
    }
    Err(LauncherError::validation(format!(
        "没有在 Modrinth 找到前置模组“{input}”。请确认模组来源，或手动安装该前置模组。"
    )))
}

fn exact_candidate_matches(candidate: &str, hits: &[serde_json::Value]) -> Vec<String> {
    let mut matches = Vec::new();
    for hit in hits {
        let slug = hit.get("slug").and_then(|value| value.as_str());
        let project_id = hit.get("project_id").and_then(|value| value.as_str());
        let title = hit.get("title").and_then(|value| value.as_str());
        let title_matches = title.is_some_and(|value| {
            value.eq_ignore_ascii_case(candidate)
                || value
                    .replace([' ', '_'], "-")
                    .eq_ignore_ascii_case(candidate)
        });
        if slug.is_some_and(|value| value.eq_ignore_ascii_case(candidate))
            || project_id.is_some_and(|value| value.eq_ignore_ascii_case(candidate))
            || title_matches
        {
            if let Some(project_id) = project_id {
                if !matches.contains(&project_id.to_string()) {
                    matches.push(project_id.to_string());
                }
            }
        }
    }
    matches
}

async fn modrinth_project_by_hash(sha1: &str) -> Option<String> {
    let url = reqwest::Url::parse(&format!(
        "https://api.modrinth.com/v2/version_file/{sha1}?algorithm=sha1"
    ))
    .ok()?;
    let value = fetch_modrinth_json(url).await.ok()?;
    let project_id = value.get("project_id").and_then(|value| value.as_str())?;
    let project_id = project_id.to_string();
    (3..=64)
        .contains(&project_id.len())
        .then_some(project_id)
        .filter(|id| id.chars().all(|value| value.is_ascii_alphanumeric()))
}

fn normalize_modrinth_identity(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
        })
        .map(|character| {
            if character == '_' {
                '-'
            } else {
                character.to_ascii_lowercase()
            }
        })
        .collect()
}

/// Provider metadata → dependency metadata：从整合包/已安装项目的版本依赖反查 dep 的 project_id。
async fn resolve_modrinth_dependency_metadata(
    app: &AppHandle,
    instance_id: i64,
    dep: &str,
    game_version: &str,
    loader: &str,
) -> Result<Option<String>, LauncherError> {
    let connection = open_database(app)?;
    let mut project_ids: Vec<String> = Vec::new();
    if let Ok(Some(pack_project)) = connection.query_row(
        "SELECT project_id FROM instance_pack_source WHERE instance_id=?1 AND provider='modrinth'",
        [instance_id],
        |row| row.get::<_, Option<String>>(0),
    ) {
        project_ids.push(pack_project);
    }
    {
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT metadata_json FROM content_items
                 WHERE instance_id=?1 AND kind='mod' AND source='modrinth' LIMIT 16",
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let rows = statement
            .query_map([instance_id], |row| row.get::<_, Option<String>>(0))
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .filter_map(Result::ok)
            .flatten()
            .collect::<Vec<_>>();
        for metadata in rows {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&metadata) {
                if let Some(project_id) = value
                    .get("modrinthProjectId")
                    .and_then(|value| value.as_str())
                {
                    if !project_ids.contains(&project_id.to_string()) {
                        project_ids.push(project_id.to_string());
                    }
                }
            }
        }
    }
    drop(connection);
    let target = normalize_modrinth_identity(dep);
    for project_id in project_ids.into_iter().take(12) {
        let Ok(versions) =
            fetch_project_versions(&project_id, Some(game_version), Some(loader)).await
        else {
            continue;
        };
        for version in versions {
            let Some(dependencies) = version
                .get("dependencies")
                .and_then(|value| value.as_array())
            else {
                continue;
            };
            for dependency in dependencies {
                let Some(candidate_project) = dependency
                    .get("project_id")
                    .and_then(|value| value.as_str())
                else {
                    continue;
                };
                if candidate_project == project_id.as_str() {
                    continue;
                }
                let Ok(project_value) = fetch_modrinth_json(
                    reqwest::Url::parse(&format!(
                        "https://api.modrinth.com/v2/project/{candidate_project}"
                    ))
                    .map_err(|error| LauncherError::storage(error.to_string()))?,
                )
                .await
                else {
                    continue;
                };
                let title = normalize_modrinth_identity(
                    project_value
                        .get("title")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default(),
                );
                let slug = normalize_modrinth_identity(
                    project_value
                        .get("slug")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default(),
                );
                if title == target || slug == target {
                    let connection = open_database(app)?;
                    let _ = connection.execute(
                        "INSERT INTO content_identity_cache(local_mod_id, game_version, loader, provider, project_id, confidence, source, updated_at)
                         VALUES(?1, ?2, ?3, 'modrinth', ?4, 'TRUSTED_MAPPING', 'provider_dependency_metadata', ?5)
                         ON CONFLICT(local_mod_id, game_version, loader, provider) DO UPDATE SET
                           project_id=excluded.project_id, confidence=excluded.confidence, source=excluded.source, updated_at=excluded.updated_at",
                        rusqlite::params![
                            dep.to_ascii_lowercase(),
                            game_version,
                            loader,
                            candidate_project,
                            chrono_like_timestamp()
                        ],
                    );
                    return Ok(Some(candidate_project.to_string()));
                }
            }
        }
    }
    Ok(None)
}

/// 统一解析并安装缺失前置：优先 CurseForge 索引的 Mod ID 精确匹配，其次 Modrinth。
async fn resolve_missing_mod_dependency(
    app: &AppHandle,
    instance_id: i64,
    dep: &str,
) -> Result<ContentItem, LauncherError> {
    if let Ok(item) = resolve_curseforge_dependency(app, instance_id, dep).await {
        return Ok(item);
    }
    let connection = open_database(app)?;
    let (game_version, loader): (String, String) = connection
        .query_row(
            "SELECT game_version, loader_type FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    drop(connection);
    // Provider metadata（整合包版本依赖）→ 已安装项目版本依赖。
    if let Some(project_id) =
        resolve_modrinth_dependency_metadata(app, instance_id, dep, &game_version, &loader).await?
    {
        if let Ok(item) = install_modrinth_mod(app.clone(), instance_id, project_id).await {
            return Ok(item);
        }
    }
    // kotlinforforge 是语言加载器：Modrinth 项目名是 “kotlin-for-forge”，
    // 直接按官方项目 ID 安装，避免搜索不到。
    if dep.eq_ignore_ascii_case("kotlinforforge") {
        if let Ok(item) = install_modrinth_mod(app.clone(), instance_id, "ordsPcFz".into()).await {
            return Ok(item);
        }
    }
    let project_id = resolve_modrinth_project_id(dep).await?;
    install_modrinth_mod(app.clone(), instance_id, project_id).await
}

async fn auto_install_missing_mod_dependencies(
    app: &AppHandle,
    instance_id: i64,
    root_path: &str,
    loader: &str,
) -> Result<(), LauncherError> {
    if loader == "vanilla" {
        return Ok(());
    }
    let mods = PathBuf::from(root_path).join(".minecraft").join("mods");
    if !mods.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(&mods)
        .map_err(|error| LauncherError::storage(format!("无法读取模组文件夹：{error}")))?;
    let mut inspections = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .path();
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|value| !value.eq_ignore_ascii_case("jar"))
        {
            continue;
        }
        if let Ok(inspection) = inspect_mod_jar_path(&path) {
            inspections.push(inspection);
        }
    }
    let mut installed_ids = installed_mod_ids(&inspections);
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    for inspection in &inspections {
        for dependency in &inspection.dependencies {
            dependents
                .entry(dependency.to_ascii_lowercase())
                .or_default()
                .push(inspection.file_name.clone());
        }
    }
    let kotlin_forge_present = has_kotlinforforge_file(&mods);
    let missing = missing_dependencies(
        inspections
            .iter()
            .flat_map(|inspection| inspection.dependencies.iter().map(|id| id.as_str())),
        &installed_ids,
        kotlin_forge_present,
    );
    let mut failures = Vec::new();
    let auto_fill_started = std::time::Instant::now();
    for missing_id in missing {
        if auto_fill_started.elapsed().as_secs() >= 60 {
            failures.push(
                "自动补齐超过 60 秒时间预算，已停止尝试；可先启动游戏，稍后重试补齐。".to_string(),
            );
            break;
        }
        let dependent_mods = dependents
            .get(&missing_id)
            .map(|names| names.join("、"))
            .unwrap_or_default();
        let resolved = tokio::time::timeout(
            Duration::from_secs(30),
            resolve_missing_mod_dependency(app, instance_id, &missing_id),
        )
        .await;
        match resolved {
            Ok(Ok(item)) => {
                if let Some(metadata_json) = item.metadata_json.as_deref() {
                    if let Ok(metadata) = serde_json::from_str::<serde_json::Value>(metadata_json) {
                        if let Some(mod_id) = metadata.get("modId").and_then(|value| value.as_str())
                        {
                            installed_ids.insert(mod_id.to_ascii_lowercase());
                        }
                    }
                }
            }
            Ok(Err(error)) => {
                failures.push(format!(
                    "{missing_id}：{}{}",
                    error.message,
                    if dependent_mods.is_empty() {
                        String::new()
                    } else {
                        format!("\n  需要它的模组：{dependent_mods}")
                    }
                ));
            }
            Err(_) => {
                failures.push(format!(
                    "{missing_id}：自动补齐超时（网络较慢），可稍后重试。{}",
                    if dependent_mods.is_empty() {
                        String::new()
                    } else {
                        format!("\n  需要它的模组：{dependent_mods}")
                    }
                ));
            }
        }
    }
    if !failures.is_empty() {
        return Err(LauncherError::validation(format!(
            "自动补齐前置模组未完成：\n- {}\n请检查网络后重试，或在模组页删除需要这些前置的模组。",
            failures.join("\n- ")
        )));
    }
    Ok(())
}

#[tauri::command]
async fn repair_missing_mod_dependencies(
    app: AppHandle,
    instance_id: i64,
) -> Result<String, LauncherError> {
    let connection = open_database(&app)?;
    let (root_path, loader): (String, String) = connection
        .query_row(
            "SELECT root_path, loader_type FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    auto_install_missing_mod_dependencies(&app, instance_id, &root_path, &loader).await?;
    Ok("前置模组已全部补齐，可以重新开始游戏。".to_string())
}

#[tauri::command]
async fn check_mod_updates(
    app: AppHandle,
    instance_id: i64,
) -> Result<Vec<ModUpdateInfo>, LauncherError> {
    let (game_version, loader, installed) = {
        let connection = open_database(&app)?;
        let (game_version, loader): (String, String) = connection
            .query_row(
                "SELECT game_version,loader_type FROM instances WHERE id=?1",
                [instance_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
        let installed = {
            let mut statement = connection
                .prepare("SELECT id,metadata_json FROM content_items WHERE instance_id=?1 AND kind='mod' AND source='modrinth'")
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            let rows = statement
                .query_map([instance_id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .map_err(|error| LauncherError::storage(error.to_string()))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            rows
        };
        (game_version, loader, installed)
    };

    let mut updates = Vec::new();
    for (content_id, metadata_json) in installed {
        let metadata = metadata_json
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let Some(project_id) = metadata
            .get("modrinthProjectId")
            .and_then(|value| value.as_str())
            .map(str::to_string)
        else {
            continue;
        };
        let selected = modrinth_compatible_version(&project_id, &game_version, &loader).await?;
        let latest_version = selected
            .get("version_number")
            .and_then(|value| value.as_str())
            .unwrap_or("未知版本")
            .to_string();
        let installed_version = metadata
            .get("version")
            .and_then(|value| value.as_str())
            .unwrap_or("未知版本")
            .to_string();
        let latest_sha1 = selected
            .get("files")
            .and_then(|value| value.as_array())
            .and_then(|files| {
                files
                    .iter()
                    .find(|file| {
                        file.get("primary").and_then(|value| value.as_bool()) == Some(true)
                    })
                    .or_else(|| files.first())
            })
            .and_then(|file| file.pointer("/hashes/sha1"))
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let installed_sha1 = metadata
            .get("modrinthSha1")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        updates.push(ModUpdateInfo {
            content_id,
            project_id,
            installed_version,
            latest_version,
            update_available: !latest_sha1.is_empty()
                && !latest_sha1.eq_ignore_ascii_case(installed_sha1),
        });
    }
    Ok(updates)
}

#[tauri::command]
async fn update_modrinth_mod(
    app: AppHandle,
    content_id: i64,
) -> Result<ContentItem, LauncherError> {
    let connection = open_database(&app)?;
    let (mut item, root) = content_location(&connection, content_id)?;
    if item.source != "modrinth" {
        return Err(LauncherError::validation(
            "只有从 Modrinth 安装的模组可以在线更新。",
        ));
    }
    let (game_version, loader): (String, String) = connection
        .query_row(
            "SELECT game_version,loader_type FROM instances WHERE id=?1",
            [item.instance_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    let mut metadata = item
        .metadata_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .ok_or_else(|| LauncherError::validation("这个模组缺少在线来源信息，无法自动更新。"))?;
    let project_id = metadata
        .get("modrinthProjectId")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::validation("这个模组缺少 Modrinth 项目编号。"))?
        .to_string();
    drop(connection);

    let (url, sha1, filename, size) =
        modrinth_primary_file(&project_id, Some(&game_version), Some(&loader), ".jar").await?;
    if metadata
        .get("modrinthSha1")
        .and_then(|value| value.as_str())
        .is_some_and(|installed| installed.eq_ignore_ascii_case(&sha1))
    {
        return Ok(item);
    }
    let cache = launcher_data_directory()?
        .join("cache")
        .join("modrinth")
        .join(format!("{}-{filename}", &sha1[..12]));
    download_verified_file(&app, item.instance_id, &url, &sha1, Some(size), &cache).await?;
    let inspection = inspect_mod_jar_path(&cache)?;
    ensure_loader_compatible(&loader, &inspection.loader_type)?;

    let mods = root.join(".minecraft").join("mods");
    let active_directory = if item.enabled {
        mods.clone()
    } else {
        mods.join("disabled")
    };
    fs::create_dir_all(&active_directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    safe_jar_file_name(Path::new(&item.file_name))?;
    let old_path = active_directory.join(&item.file_name);
    if !old_path.is_file() {
        return Err(LauncherError::validation(
            "原模组文件不存在，已取消更新以免丢失数据。",
        ));
    }
    let destination = active_directory.join(&filename);
    if destination != old_path && destination.exists() {
        return Err(LauncherError::validation(
            "更新文件与现有模组重名，已取消更新。",
        ));
    }
    let staged = active_directory.join(format!(".update-{}.jar", unique_timestamp()));
    fs::copy(&cache, &staged).map_err(|error| LauncherError::storage(error.to_string()))?;
    let copied = inspect_mod_jar_path(&staged)?;
    if copied.sha256 != inspection.sha256 {
        let _ = fs::remove_file(&staged);
        return Err(LauncherError::validation("更新文件复制后校验不一致。"));
    }
    let backup_directory = launcher_data_directory()?
        .join("backups")
        .join("mods")
        .join(item.instance_id.to_string());
    fs::create_dir_all(&backup_directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let backup = backup_directory.join(format!("{}-{}", unique_timestamp(), item.file_name));
    let mut transaction = fs_safe::FsTransaction::new(format!("mod-update-{content_id}"));
    transaction.move_with_undo(&old_path, &backup)?;
    if let Err(error) = transaction.move_with_undo(&staged, &destination) {
        transaction.rollback()?;
        return Err(LauncherError::storage(format!(
            "替换模组失败：{}",
            error.message
        )));
    }

    metadata = serde_json::to_value(&inspection)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "modrinthProjectId".into(),
            serde_json::Value::String(project_id),
        );
        object.insert("modrinthSha1".into(), serde_json::Value::String(sha1));
    }
    let metadata_json = metadata.to_string();
    let installed_at = chrono_like_timestamp();
    let connection = open_database(&app)?;
    if let Err(error) = connection.execute(
        "UPDATE content_items SET file_name=?1,hash=?2,metadata_json=?3,installed_at=?4 WHERE id=?5",
        params![filename, inspection.sha256, metadata_json, installed_at, content_id],
    ) {
        transaction.rollback()?;
        return Err(LauncherError::storage(error.to_string()));
    }
    transaction.commit();
    item.file_name = filename;
    item.hash = inspection.sha256;
    item.metadata_json = Some(metadata_json);
    item.installed_at = installed_at;
    Ok(item)
}

#[tauri::command]
async fn install_modrinth_modpack(
    app: AppHandle,
    project_id: String,
) -> Result<ImportedModpack, LauncherError> {
    log::info!("开始安装 Modrinth 整合包：project={project_id}");
    let (url, sha1, filename, size) =
        modrinth_primary_file(&project_id, None, None, ".mrpack").await?;
    let cache = launcher_data_directory()?
        .join("cache")
        .join("modrinth")
        .join(format!("{}-{filename}", &sha1[..12]));
    download_verified_file(&app, 0, &url, &sha1, Some(size), &cache).await?;
    import_modrinth_pack(app, cache.to_string_lossy().to_string()).await
}

fn pack_target_path(game: &Path, value: &str) -> Result<PathBuf, LauncherError> {
    let relative = safe_relative_download_path(value)?;
    let first = relative
        .components()
        .next()
        .and_then(|value| match value {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        first.as_str(),
        "versions" | "libraries" | "assets" | ".launcher-backup"
    ) {
        return Err(LauncherError::validation("整合包试图写入启动器管理目录。"));
    }
    Ok(game.join(relative))
}

fn move_pack_collision_to_backup(game: &Path, output: &Path) -> Result<(), LauncherError> {
    let file_name = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| LauncherError::validation("整合包覆盖文件名无效。"))?;
    let backup_directory = game.join(".launcher-backup").join("pack-overwrites");
    fs::create_dir_all(&backup_directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let backup = backup_directory.join(format!("{}-{file_name}", unique_timestamp()));
    fs::rename(output, backup)
        .map_err(|error| LauncherError::storage(format!("备份被覆盖文件失败：{error}")))
}

fn extract_pack_overrides(source: &Path, game: &Path) -> Result<usize, LauncherError> {
    let limits = fs_safe::ArchiveLimits::default();
    let staging = game
        .join(".staging")
        .join(format!("overrides-{}", unique_timestamp()));
    let report = fs_safe::extract_zip_securely(source, &staging, &limits)?;
    let overrides_root = ["overrides", "client-overrides"]
        .iter()
        .map(|name| staging.join(name))
        .find(|path| path.is_dir())
        .unwrap_or_else(|| staging.clone());
    let mut count = 0usize;
    let mut pending = vec![overrides_root.clone()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current)
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .flatten()
        {
            let path = entry.path();
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                pending.push(path);
                continue;
            }
            let Ok(relative) = path.strip_prefix(&overrides_root) else {
                continue;
            };
            if relative.as_os_str().is_empty() {
                continue;
            }
            let output = pack_target_path(game, &relative.to_string_lossy())?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
            }
            if output.exists() {
                move_pack_collision_to_backup(game, &output)?;
            }
            fs::copy(&path, &output).map_err(|error| LauncherError::storage(error.to_string()))?;
            count += 1;
        }
    }
    let _ = fs::remove_dir_all(&staging);
    log::info!(
        "整合包 overrides 解压完成：entries={} files={}",
        report.entries,
        count
    );
    Ok(count)
}

#[tauri::command]
async fn import_modrinth_pack(
    app: AppHandle,
    source_path: String,
) -> Result<ImportedModpack, LauncherError> {
    download_cancel_flag().store(false, Ordering::Release);
    let source = PathBuf::from(&source_path);
    let inspection = inspect_modpack_path(&source)?;
    if inspection.format != "modrinth" {
        return Err(LauncherError::validation(
            "自动导入当前仅支持标准 Modrinth .mrpack；其他格式可先安全预览。",
        ));
    }
    let game_version = inspection
        .game_version
        .clone()
        .ok_or_else(|| LauncherError::validation("整合包未声明 Minecraft 版本。"))?;
    let loader_type = inspection
        .loader_type
        .clone()
        .ok_or_else(|| LauncherError::validation("整合包未声明受支持的加载器。"))?;
    validate_loader_type(&loader_type)?;
    let instance = create_instance_profile(
        app.clone(),
        inspection
            .name
            .clone()
            .unwrap_or_else(|| "Imported Modpack".into()),
        game_version.clone(),
        loader_type.clone(),
    )?;
    let file =
        fs::File::open(&source).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| LauncherError::validation(error.to_string()))?;
    let bytes = read_descriptor(&mut archive, "modrinth.index.json")?
        .ok_or_else(|| LauncherError::validation("Modrinth index 缺失。"))?;
    drop(archive);
    let index: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::validation(error.to_string()))?;
    let files = index
        .get("files")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LauncherError::validation("Modrinth index 缺少 files。"))?;
    let game = PathBuf::from(&instance.root_path).join(".minecraft");
    let mut downloaded_files = 0usize;
    for entry in files {
        if entry
            .pointer("/env/client")
            .and_then(|value| value.as_str())
            == Some("unsupported")
        {
            continue;
        }
        let relative = entry
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LauncherError::validation("整合包文件路径缺失。"))?;
        let target = pack_target_path(&game, relative)?;
        let sha1 = entry
            .pointer("/hashes/sha1")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LauncherError::validation("整合包文件缺少 SHA-1。"))?;
        let size = entry.get("fileSize").and_then(|value| value.as_u64());
        let downloads = entry
            .get("downloads")
            .and_then(|value| value.as_array())
            .ok_or_else(|| LauncherError::validation("整合包文件缺少下载源。"))?;
        let url = downloads
            .iter()
            .filter_map(|value| value.as_str())
            .find(|value| validate_resource_url(value).is_ok())
            .ok_or_else(|| LauncherError::validation("整合包文件没有受信任的 HTTPS 下载源。"))?;
        download_verified_file(&app, instance.id, url, sha1, size, &target).await?;
        downloaded_files += 1;
        if target
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("jar"))
            && relative
                .replace('\\', "/")
                .to_ascii_lowercase()
                .starts_with("mods/")
        {
            let mod_info = inspect_mod_jar_path(&target)?;
            ensure_loader_compatible(&loader_type, &mod_info.loader_type)?;
            ensure_game_version_compatible(&game_version, &mod_info)?;
            let connection = open_database(&app)?;
            let metadata = serde_json::to_string(&mod_info)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            connection.execute("INSERT OR IGNORE INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,'mod',?2,?3,?4,1,'modrinth',?5)", params![instance.id, target.file_name().and_then(|value| value.to_str()).unwrap_or("mod.jar"), mod_info.sha256, metadata, chrono_like_timestamp()]).map_err(|error| LauncherError::storage(error.to_string()))?;
        }
    }
    let override_files = extract_pack_overrides(&source, &game)?;
    let connection = open_database(&app)?;
    connection
        .execute(
            "UPDATE instances SET source='modrinth' WHERE id=?1",
            [instance.id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(ImportedModpack {
        instance: Instance {
            source: "modrinth".into(),
            ..instance
        },
        downloaded_files,
        override_files,
    })
}

const CURSEFORGE_API_BASE: &str = "https://www.curseforge.com/api/v1";
const CURSEFORGE_PROXY_BASE: &str = "https://api.curse.tools/v1/cf";

fn parse_curseforge_file_info(value: &serde_json::Value) -> Result<(String, u64), LauncherError> {
    let data = value
        .get("data")
        .ok_or_else(|| LauncherError::validation("CurseForge 文件信息缺少 data。"))?;
    let file_name = data
        .get("fileName")
        .and_then(|entry| entry.as_str())
        .ok_or_else(|| LauncherError::validation("CurseForge 文件缺少文件名。"))?;
    validate_instance_field(file_name, 240)?;
    let size = data
        .get("fileLength")
        .and_then(|entry| entry.as_u64())
        .ok_or_else(|| LauncherError::validation("CurseForge 文件缺少大小。"))?;
    if size > 2 * 1024 * 1024 * 1024 {
        return Err(LauncherError::validation(
            "CurseForge 文件超过安全大小限制。",
        ));
    }
    Ok((file_name.to_string(), size))
}

async fn fetch_curseforge_file_info_json(
    client: &reqwest::Client,
    base: &str,
    project_id: i64,
    file_id: i64,
) -> Result<serde_json::Value, LauncherError> {
    let url = format!("{base}/mods/{project_id}/files/{file_id}");
    let parsed =
        reqwest::Url::parse(&url).map_err(|error| LauncherError::storage(error.to_string()))?;
    if !matches!(
        parsed.host_str(),
        Some("www.curseforge.com") | Some("api.curse.tools")
    ) {
        return Err(LauncherError::validation("CurseForge 信息地址不受信任。"));
    }
    let response = send_download_request(client, &parsed, None)
        .await?
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("CurseForge 文件信息返回错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|length| length > 2 * 1024 * 1024)
    {
        return Err(LauncherError::validation(
            "CurseForge 文件信息超过安全限制。",
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(LauncherError::validation(
            "CurseForge 文件信息超过安全限制。",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(format!("CurseForge 文件信息无效：{error}")))
}

async fn curseforge_file_info(
    client: &reqwest::Client,
    project_id: i64,
    file_id: i64,
) -> Result<(String, u64), LauncherError> {
    if project_id <= 0 || file_id <= 0 {
        return Err(LauncherError::validation("CurseForge 项目或文件编号无效。"));
    }
    let mut last_error = None;
    for base in [CURSEFORGE_API_BASE, CURSEFORGE_PROXY_BASE] {
        match fetch_curseforge_file_info_json(client, base, project_id, file_id).await {
            Ok(value) => return parse_curseforge_file_info(&value),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| LauncherError::validation("无法获取 CurseForge 文件信息。")))
}

async fn download_curseforge_file(
    app: &AppHandle,
    instance_id: i64,
    project_id: i64,
    file_id: i64,
    size: u64,
    target: &Path,
) -> Result<u64, LauncherError> {
    let client = shared_download_client()?;
    let mut download_url =
        format!("{CURSEFORGE_API_BASE}/mods/{project_id}/files/{file_id}/download");
    if let Ok(value) =
        fetch_curseforge_file_info_json(&client, CURSEFORGE_PROXY_BASE, project_id, file_id).await
    {
        if let Some(url) = value
            .pointer("/data/downloadUrl")
            .and_then(|entry| entry.as_str())
        {
            if let Ok(parsed) = reqwest::Url::parse(url) {
                if parsed.host_str() == Some("edge.forgecdn.net") {
                    download_url = url.to_string();
                }
            }
        }
    }
    let parsed = reqwest::Url::parse(&download_url)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if !matches!(
        parsed.host_str(),
        Some("www.curseforge.com") | Some("edge.forgecdn.net")
    ) {
        return Err(LauncherError::validation("CurseForge 下载地址不受信任。"));
    }
    // CurseForge 不提供 SHA-1，仅按大小校验；文件仍来自 HTTPS 官方 CDN。
    download_verified_file(app, instance_id, &download_url, "", Some(size), target).await
}

fn normalize_curseforge_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn curseforge_match_score(dep: &str, name: &str, slug: &str, file_name: &str) -> i32 {
    if dep.is_empty() {
        return 0;
    }
    let file_key = normalize_curseforge_key(file_name);
    if !file_key.is_empty() {
        if file_key == dep {
            return 100;
        }
        if file_key.starts_with(dep) || dep.starts_with(&file_key) {
            return 95;
        }
        if file_key.contains(dep) {
            return 85;
        }
    }
    if (!name.is_empty() && name == dep) || (!slug.is_empty() && slug == dep) {
        return 100;
    }
    if (!name.is_empty() && (name.starts_with(dep) || dep.starts_with(name)))
        || (!slug.is_empty() && (slug.starts_with(dep) || dep.starts_with(slug)))
    {
        return 90;
    }
    if (!name.is_empty() && name.contains(dep)) || (!slug.is_empty() && slug.contains(dep)) {
        return 70;
    }
    let common = dep
        .chars()
        .zip(name.chars())
        .take_while(|(left, right)| left == right)
        .count();
    if common >= 8 {
        return 85;
    }
    if common >= 5 {
        return 75;
    }
    0
}

fn collect_curseforge_index_files(root_path: &str) -> Vec<serde_json::Value> {
    let mut candidates = Vec::new();
    let mut paths = vec![PathBuf::from(root_path).join(".curseforge-index.json")];
    if let Ok(root) = launcher_data_directory() {
        paths.push(
            root.join("cache")
                .join("curseforge")
                .join("global-index.json"),
        );
    }
    for path in paths {
        if !path.is_file() {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(files) = value.get("files").and_then(|entry| entry.as_array()) {
                    candidates.extend(files.iter().cloned());
                }
            }
        }
    }
    candidates
}

fn best_curseforge_match(files: &[serde_json::Value], dep: &str) -> Option<(i64, i64)> {
    // 权威匹配：依赖名本身就是 Mod ID，先按索引中记录的 modId 精确匹配
    for file in files {
        let project_id = file.get("projectId").and_then(|v| v.as_i64()).unwrap_or(0);
        let file_id = file.get("fileId").and_then(|v| v.as_i64()).unwrap_or(0);
        if project_id <= 0 || file_id <= 0 {
            continue;
        }
        let mod_id = file
            .get("modId")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !mod_id.is_empty() && mod_id.eq_ignore_ascii_case(dep) {
            return Some((project_id, file_id));
        }
    }
    let normalized = normalize_curseforge_key(dep);
    let mut best_score = 0i32;
    let mut best: Option<(i64, i64)> = None;
    for file in files {
        let project_id = file.get("projectId").and_then(|v| v.as_i64()).unwrap_or(0);
        let file_id = file.get("fileId").and_then(|v| v.as_i64()).unwrap_or(0);
        if project_id <= 0 || file_id <= 0 {
            continue;
        }
        let name = normalize_curseforge_key(
            file.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        );
        let slug = normalize_curseforge_key(
            file.get("slug")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        );
        let file_name = normalize_curseforge_key(
            file.get("fileName")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        );
        let score = curseforge_match_score(&normalized, &name, &slug, &file_name);
        if score > best_score {
            best_score = score;
            best = Some((project_id, file_id));
        }
    }
    best.filter(|_| best_score >= 60)
}

async fn install_curseforge_ids(
    app: &AppHandle,
    instance_id: i64,
    project_id: i64,
    file_id: i64,
) -> Result<ContentItem, LauncherError> {
    let connection = open_database(app)?;
    let (root_path, loader_type, game_version): (String, String, String) = connection
        .query_row(
            "SELECT root_path, loader_type, game_version FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    drop(connection);
    let client = shared_download_client()?;
    let (file_name, size) = curseforge_file_info(&client, project_id, file_id).await?;
    let cache_dir = launcher_data_directory()?.join("cache").join("curseforge");
    let cache_target = cache_dir
        .join(format!("{project_id}-{file_id}"))
        .join(&file_name);
    download_curseforge_file(app, instance_id, project_id, file_id, size, &cache_target).await?;
    let info = inspect_mod_jar_path(&cache_target)?;
    if info.loader_type != "unknown" {
        ensure_loader_compatible(&loader_type, &info.loader_type)?;
    }
    ensure_game_version_compatible(&game_version, &info)?;
    let mods_dir = PathBuf::from(&root_path).join(".minecraft").join("mods");
    fs::create_dir_all(&mods_dir).map_err(|error| LauncherError::storage(error.to_string()))?;
    let output = mods_dir.join(&file_name);
    let final_path = if output.exists() {
        mods_dir.join(format!("{}-{}", unique_timestamp(), file_name))
    } else {
        output
    };
    fs::copy(&cache_target, &final_path)
        .map_err(|error| LauncherError::storage(format!("写入模组文件夹失败：{error}")))?;
    let metadata =
        serde_json::to_string(&info).map_err(|error| LauncherError::storage(error.to_string()))?;
    let connection = open_database(app)?;
    connection
        .execute(
            "INSERT OR IGNORE INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,'mod',?2,?3,?4,1,'curseforge',?5)",
            params![
                instance_id,
                final_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("mod.jar"),
                info.sha256,
                metadata,
                chrono_like_timestamp()
            ],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let item = connection
        .query_row(
            "SELECT id,instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at FROM content_items WHERE instance_id=?1 AND kind='mod' AND hash=?2",
            params![instance_id, info.sha256],
            content_item_from_row,
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    connection
        .execute(
            "INSERT INTO content_provenance(content_id, provider, project_id, version_id, file_id, source_url, sha1, sha256, installed_at)
             VALUES(?1, 'curseforge', ?2, NULL, ?3, ?4, NULL, ?5, ?6)
             ON CONFLICT(content_id) DO UPDATE SET
                provider='curseforge', project_id=excluded.project_id, file_id=excluded.file_id,
                source_url=excluded.source_url, sha256=excluded.sha256, installed_at=excluded.installed_at",
            params![
                item.id,
                project_id.to_string(),
                file_id.to_string(),
                format!("{CURSEFORGE_API_BASE}/mods/{project_id}/files/{file_id}"),
                info.sha256,
                item.installed_at
            ],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(item)
}

fn curseforge_modloader_type(loader: &str) -> i32 {
    match loader {
        "forge" => 1,
        "fabric" => 4,
        "quilt" => 5,
        "neoforge" => 6,
        _ => 0,
    }
}

async fn fetch_curseforge_files_page(
    client: &reqwest::Client,
    base: &str,
    project_id: i64,
    version: Option<&str>,
    loader_type: Option<i32>,
) -> Result<Vec<serde_json::Value>, LauncherError> {
    let mut url = reqwest::Url::parse(&format!("{base}/mods/{project_id}/files"))
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("pageSize", "50");
        if let Some(value) = version {
            query.append_pair("gameVersion", value);
        }
        if let Some(value) = loader_type {
            query.append_pair("modLoaderType", &value.to_string());
        }
    }
    if !matches!(
        url.host_str(),
        Some("www.curseforge.com") | Some("api.curse.tools")
    ) {
        return Err(LauncherError::validation(
            "CurseForge 文件列表地址不受信任。",
        ));
    }
    let response = send_download_request(client, &url, None)
        .await?
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("CurseForge 文件列表错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|length| length > 4 * 1024 * 1024)
    {
        return Err(LauncherError::validation(
            "CurseForge 文件列表超过安全限制。",
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(LauncherError::validation(
            "CurseForge 文件列表超过安全限制。",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(format!("CurseForge 文件列表无效：{error}")))?;
    Ok(value
        .get("data")
        .and_then(|entry| entry.as_array())
        .cloned()
        .unwrap_or_default())
}

fn select_best_curseforge_file(
    files: &[serde_json::Value],
    game_version: &str,
    loader: &str,
    version_filter: Option<&str>,
    loader_filter: Option<i32>,
) -> Option<(i64, String, u64)> {
    let game_version_lc = game_version.to_ascii_lowercase();
    let loader_lc = loader.to_ascii_lowercase();
    let mut candidates = Vec::new();
    for file in files {
        let versions: Vec<String> = file
            .get("gameVersions")
            .and_then(|entry| entry.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|v| v.to_ascii_lowercase()))
                    .collect()
            })
            .unwrap_or_default();
        if version_filter.is_some() && !versions.contains(&game_version_lc) {
            continue;
        }
        if loader_filter.is_some() && !versions.contains(&loader_lc) {
            continue;
        }
        candidates.push(file.clone());
    }
    let best = candidates.iter().max_by_key(|file| {
        let release = file
            .get("releaseType")
            .and_then(|v| v.as_i64())
            .unwrap_or(3);
        let rank = if release == 1 {
            2
        } else if release == 2 {
            1
        } else {
            0
        };
        let id = file.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        (rank, id)
    })?;
    let file_id = best.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let file_name = best
        .get("fileName")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let size = best.get("fileLength").and_then(|v| v.as_u64()).unwrap_or(0);
    (file_id > 0 && !file_name.is_empty() && size > 0).then_some((file_id, file_name, size))
}

async fn curseforge_best_file(
    client: &reqwest::Client,
    project_id: i64,
    game_version: &str,
    loader: &str,
) -> Result<(i64, String, u64), LauncherError> {
    let mod_loader_type = curseforge_modloader_type(loader);
    let attempts: Vec<(Option<&str>, Option<i32>)> = vec![
        (Some(game_version), Some(mod_loader_type)),
        (Some(game_version), None),
        (None, Some(mod_loader_type)),
        (None, None),
    ];
    let mut last_error = None;
    for (version, loader_type) in attempts {
        for base in [CURSEFORGE_API_BASE, CURSEFORGE_PROXY_BASE] {
            match fetch_curseforge_files_page(client, base, project_id, version, loader_type).await
            {
                Ok(files) => {
                    if let Some(candidate) = select_best_curseforge_file(
                        &files,
                        game_version,
                        loader,
                        version,
                        loader_type,
                    ) {
                        return Ok(candidate);
                    }
                }
                Err(error) => last_error = Some(error),
            }
        }
    }
    Err(last_error
        .unwrap_or_else(|| LauncherError::validation("CurseForge 没有找到与该实例兼容的文件。")))
}

async fn resolve_curseforge_dependency(
    app: &AppHandle,
    instance_id: i64,
    dep: &str,
) -> Result<ContentItem, LauncherError> {
    let connection = open_database(app)?;
    let (root_path, game_version, loader): (String, String, String) = connection
        .query_row(
            "SELECT root_path, game_version, loader_type FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    drop(connection);
    let files = collect_curseforge_index_files(&root_path);
    if files.is_empty() {
        return Err(LauncherError::validation(
            "没有可用的 CurseForge 索引；请先导入包含该模组的 CurseForge 整合包。",
        ));
    }
    let (project_id, indexed_file_id) = best_curseforge_match(&files, dep)
        .ok_or_else(|| LauncherError::validation("在 CurseForge 索引中未匹配到该项目。"))?;
    // 优先按实例版本与加载器选择最优文件；接口不可用时退回索引中的文件
    let client = shared_download_client()?;
    let file_id = match curseforge_best_file(&client, project_id, &game_version, &loader).await {
        Ok((file_id, _, _)) => file_id,
        Err(_) => indexed_file_id,
    };
    install_curseforge_ids(app, instance_id, project_id, file_id).await
}

fn extract_curseforge_slug(url: &str) -> Option<String> {
    let index = url.find("/mc-mods/")?;
    let rest = &url[index + "/mc-mods/".len()..];
    let slug: String = rest
        .chars()
        .take_while(|character| !matches!(character, '/' | '?' | '#'))
        .collect();
    (!slug.is_empty()).then_some(slug)
}

fn extract_curseforge_file_id(url: &str) -> Option<i64> {
    let index = url.find("/files/")?;
    let rest = &url[index + "/files/".len()..];
    let digits: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse::<i64>().ok().filter(|value| *value > 0)
}

#[tauri::command]
async fn install_curseforge_url(
    app: AppHandle,
    instance_id: i64,
    url: String,
) -> Result<ContentItem, LauncherError> {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://www.curseforge.com/") {
        return Err(LauncherError::validation(
            "仅支持 www.curseforge.com 的项目或文件链接。",
        ));
    }
    let connection = open_database(&app)?;
    let root_path: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    drop(connection);
    let files = collect_curseforge_index_files(&root_path);
    if files.is_empty() {
        return Err(LauncherError::validation(
            "没有可用的 CurseForge 索引；请先导入包含该模组的 CurseForge 整合包。",
        ));
    }
    let slug = extract_curseforge_slug(trimmed).map(|value| normalize_curseforge_key(&value));
    let url_file_id = extract_curseforge_file_id(trimmed);
    let mut best: Option<(i64, i64)> = None;
    let mut best_score = 0i32;
    for file in &files {
        let project_id = file.get("projectId").and_then(|v| v.as_i64()).unwrap_or(0);
        let file_id = file.get("fileId").and_then(|v| v.as_i64()).unwrap_or(0);
        if project_id <= 0 || file_id <= 0 {
            continue;
        }
        if let Some(target_id) = url_file_id {
            if file_id == target_id {
                best = Some((project_id, file_id));
                break;
            }
        }
        let name = normalize_curseforge_key(
            file.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        );
        let entry_slug = normalize_curseforge_key(
            file.get("slug")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        );
        let score = if let Some(target) = &slug {
            if *target == entry_slug || *target == name {
                100
            } else if entry_slug.starts_with(target) || name.starts_with(target) {
                90
            } else {
                0
            }
        } else {
            0
        };
        if score > best_score {
            best_score = score;
            best = Some((project_id, file_id));
        }
    }
    let (project_id, file_id) = best.ok_or_else(|| {
        LauncherError::validation(
            "未在 CurseForge 索引中找到该链接对应的项目；请先导入包含它的整合包。",
        )
    })?;
    install_curseforge_ids(&app, instance_id, project_id, file_id).await
}

#[tauri::command]
async fn install_curseforge_project(
    app: AppHandle,
    instance_id: i64,
    project_id: String,
    game_version: String,
    loader: String,
) -> Result<ContentItem, LauncherError> {
    let project_id = project_id.trim();
    if project_id.is_empty()
        || project_id.len() > 16
        || !project_id
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(LauncherError::validation("CurseForge 项目标识无效。"));
    }
    validate_instance_field(&game_version, 64)?;
    validate_loader_type(&loader)?;
    let project_id: i64 = project_id
        .parse()
        .map_err(|_| LauncherError::validation("CurseForge 项目标识无效。"))?;
    let client = shared_download_client()?;
    let (file_id, _, _) = curseforge_best_file(&client, project_id, &game_version, &loader).await?;
    install_curseforge_ids(&app, instance_id, project_id, file_id).await
}

#[tauri::command]
async fn download_curseforge_modpack(
    app: AppHandle,
    instance_id: i64,
    project_id: String,
    game_version: String,
    loader: String,
) -> Result<String, LauncherError> {
    log::info!("开始下载 CurseForge 整合包：project={project_id}");
    let project_id = project_id.trim();
    if project_id.is_empty()
        || project_id.len() > 16
        || !project_id
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(LauncherError::validation("CurseForge 项目标识无效。"));
    }
    validate_instance_field(&game_version, 64)?;
    let loader = if loader.trim().is_empty() {
        "forge".to_string()
    } else {
        loader.trim().to_string()
    };
    validate_loader_type(&loader)?;
    let project_id: i64 = project_id
        .parse()
        .map_err(|_| LauncherError::validation("CurseForge 项目标识无效。"))?;
    let client = shared_download_client()?;
    let (file_id, file_name, size) =
        curseforge_best_file(&client, project_id, &game_version, &loader).await?;
    validate_instance_field(&file_name, 240)?;
    let cache_dir = launcher_data_directory()?
        .join("cache")
        .join("curseforge")
        .join("modpacks");
    let target = cache_dir
        .join(format!("{project_id}-{file_id}"))
        .join(&file_name);
    download_curseforge_file(&app, instance_id, project_id, file_id, size, &target).await?;
    Ok(target.to_string_lossy().to_string())
}

#[tauri::command]
async fn import_local_pack(
    app: AppHandle,
    instance_id: i64,
    source_path: String,
) -> Result<ImportedLocalPack, LauncherError> {
    log::info!("开始导入本地整合包：instance={instance_id} path={source_path}");
    let source = PathBuf::from(&source_path);
    let inspection = inspect_modpack_path(&source)?;
    if inspection.format == "modrinth" {
        return Err(LauncherError::validation(
            "Modrinth 包请使用标准 .mrpack 导入入口。",
        ));
    }
    let connection = open_database(&app)?;
    let (root_path, loader_type, game_version): (String, String, String) = connection
        .query_row(
            "SELECT root_path,loader_type,game_version FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    let game = PathBuf::from(&root_path).join(".minecraft");
    let file =
        fs::File::open(&source).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| LauncherError::validation(error.to_string()))?;
    let allowed_roots = [
        "mods",
        "config",
        "resourcepacks",
        "shaderpacks",
        "kubejs",
        "scripts",
        "defaultconfigs",
    ];
    let mut imported_files = 0usize;
    let mut imported_mods = 0usize;
    let mut skipped_mods = Vec::new();
    let mut installed_mod_names = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(LauncherError::validation("整合包包含不允许的符号链接。"));
        }
        let normalized = entry.name().replace('\\', "/");
        let relative = if inspection.format == "curseforge" {
            normalized
                .strip_prefix("overrides/")
                .or_else(|| normalized.strip_prefix("client-overrides/"))
        } else {
            let first = normalized
                .split('/')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            allowed_roots
                .contains(&first.as_str())
                .then_some(normalized.as_str())
        };
        let Some(relative) = relative else { continue };
        let output = pack_target_path(&game, relative)?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        let is_mod = relative.to_ascii_lowercase().starts_with("mods/")
            && relative.to_ascii_lowercase().ends_with(".jar");
        let temporary = if is_mod {
            game.join("mods")
                .join(format!(".incoming-{}.jar", unique_timestamp()))
        } else {
            output.with_extension(format!("part-{}", unique_timestamp()))
        };
        let mut temporary_file = fs::File::create(&temporary)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        std::io::copy(&mut entry, &mut temporary_file)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        drop(temporary_file);
        let mod_info_result = if is_mod {
            inspect_mod_jar_path(&temporary).and_then(|info| {
                ensure_loader_compatible(&loader_type, &info.loader_type)?;
                ensure_game_version_compatible(&game_version, &info)?;
                Ok(Some(info))
            })
        } else {
            Ok(None)
        };
        let mod_info = match mod_info_result {
            Ok(info) => info,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                let jar_name = relative.rsplit('/').next().unwrap_or("未知模组");
                skipped_mods.push(format!("{jar_name}：{}", error.message));
                continue;
            }
        };
        if output.exists() {
            move_pack_collision_to_backup(&game, &output)?;
        }
        fs::rename(&temporary, &output)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if let Some(info) = mod_info {
            if let Some(name) = output.file_name().and_then(|value| value.to_str()) {
                installed_mod_names.insert(name.to_ascii_lowercase());
            }
            let metadata = serde_json::to_string(&info)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            connection.execute("INSERT OR IGNORE INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,'mod',?2,?3,?4,1,'local-pack',?5)", params![instance_id, output.file_name().and_then(|value| value.to_str()).unwrap_or("mod.jar"), info.sha256, metadata, chrono_like_timestamp()]).map_err(|error| LauncherError::storage(error.to_string()))?;
            imported_mods += 1;
        }
        imported_files += 1;
    }
    let mut downloaded_remote_files = 0usize;
    let mut unresolved_remote_files = 0usize;
    if inspection.format == "curseforge" {
        let bytes = read_descriptor(&mut archive, "manifest.json")?
            .ok_or_else(|| LauncherError::validation("CurseForge manifest 缺失。"))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            LauncherError::validation(format!("CurseForge manifest 无效：{error}"))
        })?;
        let files = value
            .get("files")
            .and_then(|entry| entry.as_array())
            .cloned()
            .unwrap_or_default();
        // CurseForge 索引改用真实下载文件名构建（不依赖 modlist 顺序），避免项目与文件错配。
        let index_path = PathBuf::from(&root_path).join(".curseforge-index.json");
        let client = shared_download_client()?;
        let cache_dir = launcher_data_directory()?.join("cache").join("curseforge");
        let mods_dir = game.join("mods");
        let tasks = files
            .into_iter()
            .filter(|file| {
                file.get("required")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true)
            })
            .map(|file| {
                let project_id = file.get("projectID").and_then(|v| v.as_i64()).unwrap_or(0);
                let file_id = file.get("fileID").and_then(|v| v.as_i64()).unwrap_or(0);
                (
                    project_id,
                    file_id,
                    app.clone(),
                    client.clone(),
                    cache_dir.clone(),
                    mods_dir.clone(),
                    loader_type.clone(),
                    game_version.clone(),
                    installed_mod_names.clone(),
                    instance_id,
                )
            });
        let results: Vec<Result<(i64, i64, String, Option<String>), String>> =
            futures_util::stream::iter(tasks)
            .map(
                |(project_id, file_id, app, client, cache_dir, mods_dir, loader_type, game_version, installed_mod_names, instance_id)| async move {
                    let (file_name, size) = curseforge_file_info(&client, project_id, file_id)
                        .await
                        .map_err(|error| format!("项目 {project_id}/文件 {file_id}：{}", error.message))?;
                    if installed_mod_names.contains(&file_name.to_ascii_lowercase()) {
                        let mod_id = std::fs::metadata(mods_dir.join(&file_name))
                            .ok()
                            .and_then(|_| inspect_mod_jar_path(&mods_dir.join(&file_name)).ok())
                            .and_then(|info| info.mod_id);
                        return Ok((project_id, file_id, file_name, mod_id));
                    }
                    let cache_target = cache_dir
                        .join(format!("{project_id}-{file_id}"))
                        .join(&file_name);
                    download_curseforge_file(&app, instance_id, project_id, file_id, size, &cache_target)
                        .await
                        .map_err(|error| format!("{file_name}：{}", error.message))?;
                    let info = inspect_mod_jar_path(&cache_target)
                        .map_err(|error| format!("{file_name}：{}", error.message))?;
                    ensure_loader_compatible(&loader_type, &info.loader_type)
                        .map_err(|error| format!("{file_name}：{}", error.message))?;
                    ensure_game_version_compatible(&game_version, &info)
                        .map_err(|error| format!("{file_name}：{}", error.message))?;
                    fs::create_dir_all(&mods_dir)
                        .map_err(|error| format!("{file_name}：无法创建模组目录 {error}"))?;
                    let output = mods_dir.join(&file_name);
                    let final_path = if output.exists() {
                        mods_dir.join(format!(
                            "{}-{}",
                            unique_timestamp(),
                            output
                                .file_name()
                                .and_then(|value| value.to_str())
                                .unwrap_or("mod.jar")
                        ))
                    } else {
                        output
                    };
                    fs::copy(&cache_target, &final_path)
                        .map_err(|error| format!("{file_name}：写入模组文件夹失败 {error}"))?;
                    let metadata = serde_json::to_string(&info)
                        .map_err(|error| format!("{file_name}：元数据写入失败 {error}"))?;
                    let connection = open_database(&app)
                        .map_err(|error| format!("{file_name}：数据库打开失败 {}", error.message))?;
                    connection
                        .execute(
                            "INSERT OR IGNORE INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,'mod',?2,?3,?4,1,'curseforge',?5)",
                            params![
                                instance_id,
                                final_path
                                    .file_name()
                                    .and_then(|value| value.to_str())
                                    .unwrap_or("mod.jar"),
                                info.sha256,
                                metadata,
                                chrono_like_timestamp()
                            ],
                        )
                        .map_err(|error| format!("{file_name}：记录写入失败 {error}"))?;
                    Ok((project_id, file_id, file_name, info.mod_id.clone()))
                },
            )
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await;
        let mut index_entries = Vec::new();
        for result in results {
            match result {
                Ok((project_id, file_id, file_name, mod_id)) => {
                    downloaded_remote_files += 1;
                    index_entries.push(serde_json::json!({
                        "projectId": project_id,
                        "fileId": file_id,
                        "fileName": file_name,
                        "modId": mod_id.unwrap_or_default(),
                        "slug": "",
                        "name": "",
                    }));
                }
                Err(reason) => {
                    unresolved_remote_files += 1;
                    skipped_mods.push(reason);
                }
            }
        }
        if !index_entries.is_empty() {
            let _ = fs::write(
                &index_path,
                serde_json::to_vec(&serde_json::json!({ "files": index_entries }))
                    .unwrap_or_default(),
            );
            let global_path = launcher_data_directory()?
                .join("cache")
                .join("curseforge")
                .join("global-index.json");
            let mut known: Vec<serde_json::Value> = Vec::new();
            if let Ok(bytes) = fs::read(&global_path) {
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    known = value
                        .get("files")
                        .and_then(|entry| entry.as_array())
                        .cloned()
                        .unwrap_or_default();
                }
            }
            let mut seen = HashSet::new();
            for entry in &known {
                let project_id = entry.get("projectId").and_then(|v| v.as_i64()).unwrap_or(0);
                let file_id = entry.get("fileId").and_then(|v| v.as_i64()).unwrap_or(0);
                if project_id > 0 && file_id > 0 {
                    seen.insert((project_id, file_id));
                }
            }
            for entry in &index_entries {
                let project_id = entry.get("projectId").and_then(|v| v.as_i64()).unwrap_or(0);
                let file_id = entry.get("fileId").and_then(|v| v.as_i64()).unwrap_or(0);
                if project_id > 0 && file_id > 0 && seen.insert((project_id, file_id)) {
                    known.push(entry.clone());
                }
            }
            if let Some(parent) = global_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(
                &global_path,
                serde_json::to_vec(&serde_json::json!({ "files": known })).unwrap_or_default(),
            );
        }
    }
    Ok(ImportedLocalPack {
        instance_id,
        imported_files,
        imported_mods,
        downloaded_remote_files,
        unresolved_remote_files,
        skipped_mods,
    })
}

#[tauri::command]
async fn import_mmc_pack(
    app: AppHandle,
    source_path: String,
) -> Result<ImportedModpack, LauncherError> {
    let source = PathBuf::from(&source_path);
    let inspection = inspect_modpack_path(&source)?;
    if inspection.format != "mmc" {
        return Err(LauncherError::validation("该整合包不是 MultiMC 格式。"));
    }
    let game_version = inspection
        .game_version
        .clone()
        .ok_or_else(|| LauncherError::validation("MultiMC 整合包未声明 Minecraft 版本。"))?;
    let loader_type = inspection
        .loader_type
        .clone()
        .ok_or_else(|| LauncherError::validation("MultiMC 整合包未声明受支持的加载器。"))?;
    validate_loader_type(&loader_type)?;
    let instance = create_instance_profile(
        app.clone(),
        inspection
            .name
            .clone()
            .unwrap_or_else(|| "MultiMC Pack".into()),
        game_version.clone(),
        loader_type.clone(),
    )?;
    let file =
        fs::File::open(&source).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| LauncherError::validation(error.to_string()))?;
    let game = PathBuf::from(&instance.root_path).join(".minecraft");
    let mut imported_files = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(LauncherError::validation(
                "MultiMC 整合包包含不允许的符号链接。",
            ));
        }
        let normalized = entry.name().replace('\\', "/");
        let Some(relative) = normalized.strip_prefix(".minecraft/") else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        let output = pack_target_path(&game, relative)?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        let is_mod = relative.to_ascii_lowercase().starts_with("mods/")
            && relative.to_ascii_lowercase().ends_with(".jar");
        let temporary = if is_mod {
            game.join("mods")
                .join(format!(".incoming-{}.jar", unique_timestamp()))
        } else {
            output.with_extension(format!("part-{}", unique_timestamp()))
        };
        let mut temporary_file = fs::File::create(&temporary)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        std::io::copy(&mut entry, &mut temporary_file)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        drop(temporary_file);
        if is_mod {
            let info = inspect_mod_jar_path(&temporary)?;
            if let Err(error) = ensure_loader_compatible(&loader_type, &info.loader_type)
                .and_then(|_| ensure_game_version_compatible(&game_version, &info))
            {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
            if output.exists() {
                move_pack_collision_to_backup(&game, &output)?;
            }
            fs::rename(&temporary, &output)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            let metadata = serde_json::to_string(&info)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            let connection = open_database(&app)?;
            connection
                .execute(
                    "INSERT OR IGNORE INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,'mod',?2,?3,?4,1,'mmc',?5)",
                    params![
                        instance.id,
                        output
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("mod.jar"),
                        info.sha256,
                        metadata,
                        chrono_like_timestamp()
                    ],
                )
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        } else {
            if output.exists() {
                move_pack_collision_to_backup(&game, &output)?;
            }
            fs::rename(&temporary, &output)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        imported_files += 1;
    }
    Ok(ImportedModpack {
        instance,
        downloaded_files: 0,
        override_files: imported_files,
    })
}

#[tauri::command]
async fn import_override_pack(
    app: AppHandle,
    source_path: String,
) -> Result<ImportedModpack, LauncherError> {
    let source = PathBuf::from(&source_path);
    let inspection = inspect_modpack_path(&source)?;
    if !matches!(inspection.format.as_str(), "hmcl" | "mcbbs") {
        return Err(LauncherError::validation(
            "该整合包格式暂不支持此导入方式。",
        ));
    }
    let game_version = inspection
        .game_version
        .clone()
        .ok_or_else(|| LauncherError::validation("整合包未声明 Minecraft 版本。"))?;
    let loader_type = inspection
        .loader_type
        .clone()
        .unwrap_or_else(|| "vanilla".to_string());
    validate_loader_type(&loader_type)?;
    let prefix = if inspection.format == "hmcl" {
        "minecraft/"
    } else {
        "overrides/"
    };
    let instance = create_instance_profile(
        app.clone(),
        inspection
            .name
            .clone()
            .unwrap_or_else(|| "Imported Pack".into()),
        game_version.clone(),
        loader_type.clone(),
    )?;
    let file =
        fs::File::open(&source).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| LauncherError::validation(error.to_string()))?;
    let game = PathBuf::from(&instance.root_path).join(".minecraft");
    let mut imported_files = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(LauncherError::validation("整合包包含不允许的符号链接。"));
        }
        let normalized = entry.name().replace('\\', "/");
        let Some(relative) = normalized.strip_prefix(prefix) else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        let output = pack_target_path(&game, relative)?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        let is_mod = loader_type != "vanilla"
            && relative.to_ascii_lowercase().starts_with("mods/")
            && relative.to_ascii_lowercase().ends_with(".jar");
        let temporary = if is_mod {
            game.join("mods")
                .join(format!(".incoming-{}.jar", unique_timestamp()))
        } else {
            output.with_extension(format!("part-{}", unique_timestamp()))
        };
        let mut temporary_file = fs::File::create(&temporary)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        std::io::copy(&mut entry, &mut temporary_file)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        drop(temporary_file);
        if is_mod {
            let info = inspect_mod_jar_path(&temporary)?;
            if let Err(error) = ensure_loader_compatible(&loader_type, &info.loader_type)
                .and_then(|_| ensure_game_version_compatible(&game_version, &info))
            {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
            if output.exists() {
                move_pack_collision_to_backup(&game, &output)?;
            }
            fs::rename(&temporary, &output)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            let metadata = serde_json::to_string(&info)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            let connection = open_database(&app)?;
            connection
                .execute(
                    "INSERT OR IGNORE INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,'mod',?2,?3,?4,1,?5,?6)",
                    params![
                        instance.id,
                        output
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("mod.jar"),
                        info.sha256,
                        metadata,
                        inspection.format,
                        chrono_like_timestamp()
                    ],
                )
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        } else {
            if output.exists() {
                move_pack_collision_to_backup(&game, &output)?;
            }
            fs::rename(&temporary, &output)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        imported_files += 1;
    }
    Ok(ImportedModpack {
        instance,
        downloaded_files: 0,
        override_files: imported_files,
    })
}

#[tauri::command]
fn inspect_mod_jar(path: String) -> Result<ModInspection, LauncherError> {
    inspect_mod_jar_path(Path::new(&path))
}

fn content_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContentItem> {
    Ok(ContentItem {
        id: row.get(0)?,
        instance_id: row.get(1)?,
        kind: row.get(2)?,
        file_name: row.get(3)?,
        hash: row.get(4)?,
        metadata_json: row.get(5)?,
        enabled: row.get::<_, i64>(6)? != 0,
        source: row.get(7)?,
        installed_at: row.get(8)?,
    })
}

#[tauri::command]
fn list_content_items(
    app: AppHandle,
    instance_id: i64,
    kind: Option<String>,
) -> Result<Vec<ContentItem>, LauncherError> {
    let connection = open_database(&app)?;
    let sql = if kind.is_some() {
        "SELECT id, instance_id, kind, file_name, hash, metadata_json, enabled, source, installed_at FROM content_items WHERE instance_id=?1 AND kind=?2 ORDER BY installed_at DESC, id DESC"
    } else {
        "SELECT id, instance_id, kind, file_name, hash, metadata_json, enabled, source, installed_at FROM content_items WHERE instance_id=?1 ORDER BY installed_at DESC, id DESC"
    };
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = if let Some(kind) = kind {
        statement.query_map(params![instance_id, kind], content_item_from_row)
    } else {
        statement.query_map(params![instance_id], content_item_from_row)
    }
    .map_err(|error| LauncherError::storage(error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| LauncherError::storage(error.to_string()))
}

#[tauri::command]
fn install_mod(
    app: AppHandle,
    instance_id: i64,
    source_path: String,
) -> Result<ContentItem, LauncherError> {
    let source = Path::new(&source_path);
    let mut inspection = inspect_mod_jar_path(source)?;
    let source_name = safe_jar_file_name(source)?;
    let connection = open_database(&app)?;
    let (root_path, instance_loader, game_version): (String, String, String) = connection
        .query_row(
            "SELECT root_path, loader_type, game_version FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => LauncherError::validation("目标实例不存在。"),
            _ => LauncherError::storage(error.to_string()),
        })?;
    ensure_loader_compatible(&instance_loader, &inspection.loader_type)?;
    ensure_game_version_compatible(&game_version, &inspection)?;

    if let Ok(existing) = connection.query_row(
        "SELECT id, instance_id, kind, file_name, hash, metadata_json, enabled, source, installed_at FROM content_items WHERE instance_id=?1 AND kind='mod' AND hash=?2",
        params![instance_id, inspection.sha256],
        content_item_from_row,
    ) {
        return Ok(existing);
    }

    let mut statement = connection
        .prepare("SELECT metadata_json FROM content_items WHERE instance_id=?1 AND kind='mod'")
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let installed_metadata = statement
        .query_map([instance_id], |row| row.get::<_, Option<String>>(0))
        .map_err(|error| LauncherError::storage(error.to_string()))?
        .filter_map(Result::ok)
        .flatten()
        .filter_map(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .collect::<Vec<_>>();
    let installed_ids = installed_metadata
        .iter()
        .filter_map(|value| value.get("modId").and_then(|entry| entry.as_str()))
        .collect::<HashSet<_>>();
    if inspection
        .mod_id
        .as_deref()
        .is_some_and(|id| installed_ids.contains(id))
    {
        return Err(LauncherError::validation(format!(
            "实例中已存在相同 Mod ID：{}。",
            inspection.mod_id.as_deref().unwrap_or_default()
        )));
    }
    if let Some(conflict) = inspection
        .conflicts
        .iter()
        .find(|id| installed_ids.contains(id.as_str()))
    {
        return Err(LauncherError::validation(format!(
            "模组声明与已安装的 {conflict} 冲突。"
        )));
    }
    if let Some(incoming_id) = inspection.mod_id.as_deref() {
        for installed in &installed_metadata {
            if installed
                .get("conflicts")
                .and_then(|value| value.as_array())
                .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(incoming_id)))
            {
                return Err(LauncherError::validation(format!(
                    "已安装模组声明与 {incoming_id} 冲突。"
                )));
            }
        }
    }
    let mods_directory = PathBuf::from(root_path).join(".minecraft").join("mods");
    let kotlin_forge_present = has_kotlinforforge_file(&mods_directory);
    let missing = missing_dependencies(
        inspection.dependencies.iter().map(|id| id.as_str()),
        &installed_ids,
        kotlin_forge_present,
    )
    .into_iter()
    .collect::<Vec<_>>();
    if !missing.is_empty() {
        inspection.warnings.push(format!(
            "尚未检测到必需依赖：{}。安装后启动前请补齐。",
            missing.join(", ")
        ));
    }

    fs::create_dir_all(&mods_directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut destination_name = source_name;
    let mut destination = mods_directory.join(&destination_name);
    if destination.exists() {
        let stem = Path::new(&destination_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("mod");
        destination_name = format!("{}-{}.jar", stem, &inspection.sha256[..8]);
        destination = mods_directory.join(&destination_name);
    }
    let temporary =
        mods_directory.join(format!(".{}.{}.part", destination_name, unique_timestamp()));
    fs::copy(source, &temporary).map_err(|error| LauncherError::storage(error.to_string()))?;
    // Inspection requires a .jar suffix; rename the private temporary file only for verification.
    let verification_path = mods_directory.join(format!(".verify-{}.jar", unique_timestamp()));
    fs::rename(&temporary, &verification_path)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let verification = inspect_mod_jar_path(&verification_path);
    if let Err(error) = &verification {
        let _ = fs::remove_file(&verification_path);
        return Err(LauncherError::validation(format!(
            "复制后的模组校验失败：{}",
            error.message
        )));
    }
    let verification = verification?;
    if verification.sha256 != inspection.sha256 {
        let _ = fs::remove_file(&verification_path);
        return Err(LauncherError::validation(
            "复制后的模组哈希不一致，安装已取消。",
        ));
    }
    fs::rename(&verification_path, &destination)
        .map_err(|error| LauncherError::storage(error.to_string()))?;

    let metadata_json = serde_json::to_string(&inspection)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let installed_at = chrono_like_timestamp();
    connection
        .execute(
            "INSERT INTO content_items(instance_id, kind, file_name, hash, metadata_json, enabled, source, installed_at) VALUES(?1, 'mod', ?2, ?3, ?4, 1, 'local', ?5)",
            params![instance_id, destination_name, inspection.sha256, metadata_json, installed_at],
        )
        .map_err(|error| {
            let _ = fs::remove_file(&destination);
            LauncherError::storage(error.to_string())
        })?;
    Ok(ContentItem {
        id: connection.last_insert_rowid(),
        instance_id,
        kind: "mod".into(),
        file_name: destination_name,
        hash: inspection.sha256,
        metadata_json: Some(metadata_json),
        enabled: true,
        source: "local".into(),
        installed_at,
    })
}

fn content_location(
    connection: &Connection,
    content_id: i64,
) -> Result<(ContentItem, PathBuf), LauncherError> {
    connection
        .query_row(
            "SELECT c.id, c.instance_id, c.kind, c.file_name, c.hash, c.metadata_json, c.enabled, c.source, c.installed_at, i.root_path FROM content_items c JOIN instances i ON i.id=c.instance_id WHERE c.id=?1 AND c.kind='mod'",
            [content_id],
            |row| {
                let item = ContentItem {
                    id: row.get(0)?, instance_id: row.get(1)?, kind: row.get(2)?, file_name: row.get(3)?, hash: row.get(4)?, metadata_json: row.get(5)?, enabled: row.get::<_, i64>(6)? != 0, source: row.get(7)?, installed_at: row.get(8)?,
                };
                Ok((item, PathBuf::from(row.get::<_, String>(9)?)))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => LauncherError::validation("模组记录不存在。"),
            _ => LauncherError::storage(error.to_string()),
        })
}

#[tauri::command]
fn set_mod_enabled(
    app: AppHandle,
    content_id: i64,
    enabled: bool,
) -> Result<ContentItem, LauncherError> {
    let connection = open_database(&app)?;
    let (mut item, root) = content_location(&connection, content_id)?;
    safe_jar_file_name(Path::new(&item.file_name))?;
    if item.enabled == enabled {
        return Ok(item);
    }
    let mods = root.join(".minecraft").join("mods");
    let disabled = mods.join("disabled");
    fs::create_dir_all(&disabled).map_err(|error| LauncherError::storage(error.to_string()))?;
    let (source, destination) = if enabled {
        (disabled.join(&item.file_name), mods.join(&item.file_name))
    } else {
        (mods.join(&item.file_name), disabled.join(&item.file_name))
    };
    if destination.exists() {
        return Err(LauncherError::validation(
            "目标位置已存在同名文件，操作已取消。",
        ));
    }
    fs::rename(&source, &destination)
        .map_err(|error| LauncherError::storage(format!("移动模组失败：{error}")))?;
    if let Err(error) = connection.execute(
        "UPDATE content_items SET enabled=?1 WHERE id=?2",
        params![enabled as i64, content_id],
    ) {
        let _ = fs::rename(&destination, &source);
        return Err(LauncherError::storage(error.to_string()));
    }
    item.enabled = enabled;
    Ok(item)
}

#[tauri::command]
fn remove_mod_to_backup(app: AppHandle, content_id: i64) -> Result<RemovedContent, LauncherError> {
    let connection = open_database(&app)?;
    let (item, root) = content_location(&connection, content_id)?;
    safe_jar_file_name(Path::new(&item.file_name))?;
    let game = root.join(".minecraft");
    let source = if item.enabled {
        game.join("mods").join(&item.file_name)
    } else {
        game.join("mods").join("disabled").join(&item.file_name)
    };
    let backup_directory = game.join(".launcher-backup").join("mods");
    fs::create_dir_all(&backup_directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let backup = backup_directory.join(format!("{}-{}", unique_timestamp(), item.file_name));
    fs::rename(&source, &backup)
        .map_err(|error| LauncherError::storage(format!("备份模组失败：{error}")))?;
    if let Err(error) = connection.execute("DELETE FROM content_items WHERE id=?1", [content_id]) {
        let _ = fs::rename(&backup, &source);
        return Err(LauncherError::storage(error.to_string()));
    }
    Ok(RemovedContent {
        id: content_id,
        backup_path: backup.to_string_lossy().to_string(),
    })
}

#[tauri::command]
fn install_content_archive(
    app: AppHandle,
    instance_id: i64,
    kind: String,
    source_path: String,
) -> Result<ContentItem, LauncherError> {
    let directory_name = content_kind_directory(&kind)?;
    let source = PathBuf::from(&source_path);
    let file_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| LauncherError::validation("内容文件名无效。"))?
        .to_string();
    if file_name.contains(['/', '\\']) {
        return Err(LauncherError::validation("内容文件名无效。"));
    }
    let (hash, size) = inspect_content_archive(&source, &kind)?;
    let connection = open_database(&app)?;
    let root: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    if let Ok(existing) = connection.query_row("SELECT id,instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at FROM content_items WHERE instance_id=?1 AND kind=?2 AND hash=?3", params![instance_id,kind,hash], content_item_from_row) { return Ok(existing); }
    let directory = PathBuf::from(root).join(".minecraft").join(directory_name);
    fs::create_dir_all(&directory).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut destination_name = file_name;
    let mut destination = directory.join(&destination_name);
    if destination.exists() {
        let stem = Path::new(&destination_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("content");
        destination_name = format!("{}-{}.zip", stem, &hash[..8]);
        destination = directory.join(&destination_name);
    }
    let temporary = directory.join(format!(".incoming-{}.zip", unique_timestamp()));
    fs::copy(&source, &temporary).map_err(|error| LauncherError::storage(error.to_string()))?;
    if sha256_file_sync(&temporary)? != hash {
        let _ = fs::remove_file(&temporary);
        return Err(LauncherError::validation("复制后的内容哈希不一致。"));
    }
    fs::rename(&temporary, &destination)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let installed_at = chrono_like_timestamp();
    let metadata = serde_json::json!({"size":size}).to_string();
    connection.execute("INSERT INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,?2,?3,?4,?5,1,'local',?6)", params![instance_id,kind,destination_name,hash,metadata,installed_at]).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(ContentItem {
        id: connection.last_insert_rowid(),
        instance_id,
        kind,
        file_name: destination_name,
        hash,
        metadata_json: Some(metadata),
        enabled: true,
        source: "local".into(),
        installed_at,
    })
}

fn archive_content_location(
    connection: &Connection,
    content_id: i64,
) -> Result<(ContentItem, PathBuf), LauncherError> {
    connection.query_row("SELECT c.id,c.instance_id,c.kind,c.file_name,c.hash,c.metadata_json,c.enabled,c.source,c.installed_at,i.root_path FROM content_items c JOIN instances i ON i.id=c.instance_id WHERE c.id=?1 AND c.kind IN ('resourcepack','shaderpack')", [content_id], |row| Ok((ContentItem { id:row.get(0)?,instance_id:row.get(1)?,kind:row.get(2)?,file_name:row.get(3)?,hash:row.get(4)?,metadata_json:row.get(5)?,enabled:row.get::<_,i64>(6)? != 0,source:row.get(7)?,installed_at:row.get(8)? }, PathBuf::from(row.get::<_,String>(9)?)))).map_err(|_| LauncherError::validation("内容记录不存在。"))
}

#[tauri::command]
fn set_content_enabled(
    app: AppHandle,
    content_id: i64,
    enabled: bool,
) -> Result<ContentItem, LauncherError> {
    let connection = open_database(&app)?;
    let (mut item, root) = archive_content_location(&connection, content_id)?;
    if item.enabled == enabled {
        return Ok(item);
    }
    let directory_name = content_kind_directory(&item.kind)?;
    let active = root.join(".minecraft").join(directory_name);
    let disabled = root
        .join(".minecraft")
        .join(".launcher-disabled")
        .join(directory_name);
    fs::create_dir_all(&disabled).map_err(|error| LauncherError::storage(error.to_string()))?;
    let (source, destination) = if enabled {
        (disabled.join(&item.file_name), active.join(&item.file_name))
    } else {
        (active.join(&item.file_name), disabled.join(&item.file_name))
    };
    if destination.exists() {
        return Err(LauncherError::validation("目标位置已有同名内容。"));
    }
    fs::rename(&source, &destination).map_err(|error| LauncherError::storage(error.to_string()))?;
    connection
        .execute(
            "UPDATE content_items SET enabled=?1 WHERE id=?2",
            params![enabled as i64, content_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    item.enabled = enabled;
    Ok(item)
}

#[tauri::command]
fn remove_content_to_backup(
    app: AppHandle,
    content_id: i64,
) -> Result<RemovedContent, LauncherError> {
    let connection = open_database(&app)?;
    let (item, root) = archive_content_location(&connection, content_id)?;
    let directory_name = content_kind_directory(&item.kind)?;
    let game = root.join(".minecraft");
    let source = if item.enabled {
        game.join(directory_name).join(&item.file_name)
    } else {
        game.join(".launcher-disabled")
            .join(directory_name)
            .join(&item.file_name)
    };
    let backup_directory = game.join(".launcher-backup").join(directory_name);
    fs::create_dir_all(&backup_directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let backup = backup_directory.join(format!("{}-{}", unique_timestamp(), item.file_name));
    fs::rename(&source, &backup).map_err(|error| LauncherError::storage(error.to_string()))?;
    connection
        .execute("DELETE FROM content_items WHERE id=?1", [content_id])
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(RemovedContent {
        id: content_id,
        backup_path: backup.to_string_lossy().to_string(),
    })
}

fn copy_world_directory(source: &Path, destination: &Path) -> Result<usize, LauncherError> {
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    let mut count = 0usize;
    let mut total = 0u64;
    while let Some((from, to)) = pending.pop() {
        fs::create_dir_all(&to).map_err(|error| LauncherError::storage(error.to_string()))?;
        for entry in
            fs::read_dir(&from).map_err(|error| LauncherError::storage(error.to_string()))?
        {
            let entry = entry.map_err(|error| LauncherError::storage(error.to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            if file_type.is_symlink() {
                return Err(LauncherError::validation("存档包含不允许的符号链接。"));
            }
            let target = to.join(entry.file_name());
            if file_type.is_dir() {
                pending.push((entry.path(), target));
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let size = entry
                .metadata()
                .map_err(|error| LauncherError::storage(error.to_string()))?
                .len();
            total = total.saturating_add(size);
            count += 1;
            if count > 200_000 || total > 20 * 1024 * 1024 * 1024 {
                return Err(LauncherError::validation(
                    "存档超过文件数量或 20 GB 安全限制。",
                ));
            }
            fs::copy(entry.path(), target)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
    }
    Ok(count)
}

fn locate_world_directory(source: &Path) -> Result<PathBuf, LauncherError> {
    if source.join("level.dat").is_file() {
        return Ok(source.to_path_buf());
    }
    let candidates = fs::read_dir(source)
        .map_err(|error| LauncherError::storage(error.to_string()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("level.dat").is_file())
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(LauncherError::validation(
            "目录根部或唯一一级子目录中未找到 level.dat。",
        ));
    }
    Ok(candidates[0].clone())
}

fn extract_world_zip(source: &Path, destination: &Path) -> Result<usize, LauncherError> {
    let limits = fs_safe::ArchiveLimits {
        max_entries: 200_000,
        max_total_uncompressed: 20 * 1024 * 1024 * 1024,
        max_single_file: 2 * 1024 * 1024 * 1024,
        ..fs_safe::ArchiveLimits::default()
    };
    let staging = destination
        .join(".staging")
        .join(format!("world-{}", unique_timestamp()));
    fs_safe::extract_zip_securely(source, &staging, &limits)?;
    let mut level_dat = None;
    let mut file_count = 0usize;
    for entry in walkdir::WalkDir::new(&staging)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() {
            file_count += 1;
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("level.dat")
            {
                level_dat = Some(entry.path().to_path_buf());
            }
        }
    }
    let level_dat =
        level_dat.ok_or_else(|| LauncherError::validation("ZIP 中未找到 level.dat。"))?;
    let world_root = level_dat
        .parent()
        .map(Path::to_path_buf)
        .filter(|parent| parent != &staging)
        .unwrap_or_else(|| staging.clone());
    let world_name = world_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|name| *name != ".staging")
        .unwrap_or("world");
    let target = destination.join(world_name);
    if target.exists() {
        return Err(LauncherError::validation(format!(
            "存档“{world_name}”已存在，请先移除或改名。"
        )));
    }
    fs::rename(&world_root, &target).map_err(|error| LauncherError::storage(error.to_string()))?;
    if world_root != staging {
        let _ = fs::remove_dir_all(&staging);
    }
    Ok(file_count)
}

#[tauri::command]
fn import_world(
    app: AppHandle,
    instance_id: i64,
    source_path: String,
) -> Result<ContentItem, LauncherError> {
    let source = PathBuf::from(&source_path);
    let connection = open_database(&app)?;
    let root: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    let (source_world, suggested_name) = if source.is_dir() {
        let world = locate_world_directory(&source)?;
        let name = world
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Imported World")
            .to_string();
        (Some(world), name)
    } else if source
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
    {
        (
            None,
            source
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("Imported World")
                .to_string(),
        )
    } else {
        return Err(LauncherError::validation(
            "存档仅支持包含 level.dat 的目录或 ZIP。",
        ));
    };
    validate_instance_field(&suggested_name, 96)?;
    let saves = PathBuf::from(root).join(".minecraft").join("saves");
    fs::create_dir_all(&saves).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut destination_name = suggested_name;
    let mut destination = saves.join(&destination_name);
    if destination.exists() {
        destination_name = format!("{}-imported-{}", destination_name, unique_timestamp());
        destination = saves.join(&destination_name);
    }
    let count = if let Some(world) = source_world {
        copy_world_directory(&world, &destination)?
    } else {
        fs::create_dir_all(&destination)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        extract_world_zip(&source, &destination)?
    };
    if !destination.join("level.dat").is_file() {
        return Err(LauncherError::validation(
            "导入结果缺少 level.dat；不完整目录已保留以便恢复。",
        ));
    }
    let hash = sha256_file_sync(&destination.join("level.dat"))?;
    let installed_at = chrono_like_timestamp();
    let metadata = serde_json::json!({"fileCount":count}).to_string();
    connection.execute("INSERT INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,'world',?2,?3,?4,1,'local',?5)", params![instance_id,destination_name,hash,metadata,installed_at]).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(ContentItem {
        id: connection.last_insert_rowid(),
        instance_id,
        kind: "world".into(),
        file_name: destination_name,
        hash,
        metadata_json: Some(metadata),
        enabled: true,
        source: "local".into(),
        installed_at,
    })
}

#[tauri::command]
fn remove_world_to_backup(
    app: AppHandle,
    content_id: i64,
) -> Result<RemovedContent, LauncherError> {
    let connection = open_database(&app)?;
    let (file_name, root): (String,String) = connection.query_row("SELECT c.file_name,i.root_path FROM content_items c JOIN instances i ON i.id=c.instance_id WHERE c.id=?1 AND c.kind='world'", [content_id], |row| Ok((row.get(0)?,row.get(1)?))).map_err(|_| LauncherError::validation("存档记录不存在。"))?;
    validate_instance_field(&file_name, 160)?;
    let game = PathBuf::from(root).join(".minecraft");
    let source = game.join("saves").join(&file_name);
    let backup_directory = game.join(".launcher-backup").join("saves");
    fs::create_dir_all(&backup_directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let backup = backup_directory.join(format!("{}-{}", unique_timestamp(), file_name));
    fs::rename(&source, &backup).map_err(|error| LauncherError::storage(error.to_string()))?;
    if let Err(error) = connection.execute("DELETE FROM content_items WHERE id=?1", [content_id]) {
        let rollback = fs::rename(&backup, &source);
        return Err(LauncherError::storage(match rollback {
            Ok(()) => format!("存档记录更新失败，文件移动已回滚：{error}"),
            Err(rollback_error) => format!(
                "存档记录更新失败且文件回滚失败：{error}；备份仍位于 {}（{rollback_error}）",
                backup.display()
            ),
        }));
    }
    Ok(RemovedContent {
        id: content_id,
        backup_path: backup.to_string_lossy().to_string(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeletedContent {
    id: i64,
    path: String,
}

fn validated_world_delete_target(saves: &Path, file_name: &str) -> Result<PathBuf, LauncherError> {
    validate_instance_field(file_name, 160)?;
    if Path::new(file_name).components().count() != 1 {
        return Err(LauncherError::validation("存档名称包含异常路径。"));
    }
    let saves = saves
        .canonicalize()
        .map_err(|_| LauncherError::validation("存档目录不存在。"))?;
    let target = saves.join(file_name);
    let canonical = target.canonicalize().unwrap_or_else(|_| target.clone());
    if canonical == saves || !canonical.starts_with(&saves) {
        return Err(LauncherError::validation(
            "存档路径不在安全范围内，已拒绝删除。",
        ));
    }
    Ok(canonical)
}

#[tauri::command]
fn delete_world_permanently(
    app: AppHandle,
    content_id: i64,
) -> Result<DeletedContent, LauncherError> {
    let connection = open_database(&app)?;
    let (file_name, root): (String, String) = connection
        .query_row(
            "SELECT c.file_name,i.root_path FROM content_items c JOIN instances i ON i.id=c.instance_id WHERE c.id=?1 AND c.kind='world'",
            [content_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| LauncherError::validation("存档记录不存在。"))?;
    let game = PathBuf::from(root).join(".minecraft");
    let canonical = validated_world_delete_target(&game.join("saves"), &file_name)?;
    if canonical.exists() {
        fs::remove_dir_all(&canonical)
            .map_err(|error| LauncherError::storage(format!("删除存档失败：{error}")))?;
    }
    connection
        .execute(
            "DELETE FROM content_items WHERE id=?1 AND kind='world'",
            [content_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(DeletedContent {
        id: content_id,
        path: canonical.to_string_lossy().into_owned(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IncompatibleTarget {
    instance_id: i64,
    file_names: Vec<String>,
}

#[tauri::command]
fn remove_incompatible_mods(
    app: AppHandle,
    targets: Vec<IncompatibleTarget>,
) -> Result<usize, LauncherError> {
    let connection = open_database(&app)?;
    let mut removed = 0usize;
    for target in targets {
        if target.file_names.is_empty() {
            continue;
        }
        let root: String = connection
            .query_row(
                "SELECT root_path FROM instances WHERE id=?1",
                [target.instance_id],
                |row| row.get(0),
            )
            .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
        let mods_dir = PathBuf::from(&root).join(".minecraft").join("mods");
        let backup_dir = PathBuf::from(&root)
            .join(".minecraft")
            .join(".launcher-backup")
            .join("mods");
        fs::create_dir_all(&backup_dir)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        for file_name in &target.file_names {
            validate_instance_field(file_name, 240)?;
            if Path::new(file_name).components().count() != 1 {
                return Err(LauncherError::validation("模组文件名包含异常路径。"));
            }
            let source = mods_dir.join(file_name);
            if source.is_file() {
                let backup = backup_dir.join(format!("{}-{}", unique_timestamp(), file_name));
                fs::rename(&source, &backup).map_err(|error| {
                    LauncherError::storage(format!("移出不兼容模组失败：{error}"))
                })?;
                removed += 1;
            }
            let _ = connection.execute(
                "DELETE FROM content_items WHERE instance_id=?1 AND kind='mod' AND file_name=?2",
                params![target.instance_id, file_name],
            );
        }
    }
    Ok(removed)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheCleanResult {
    removed_files: u64,
    freed_bytes: u64,
}

fn delete_cache_tree(
    directory: &Path,
    remove_all_files: bool,
    removed_files: &mut u64,
    freed_bytes: &mut u64,
) -> Result<(), LauncherError> {
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.ends_with("-part"))
                {
                    let bytes = backup_entry_size(&path);
                    fs::remove_dir_all(&path)
                        .map_err(|error| LauncherError::storage(error.to_string()))?;
                    *removed_files += 1;
                    *freed_bytes += bytes;
                } else {
                    pending.push(path);
                }
            } else if kind.is_file() {
                let Ok(metadata) = path.metadata() else {
                    continue;
                };
                let is_part = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("part"));
                if remove_all_files || is_part {
                    fs::remove_file(&path)
                        .map_err(|error| LauncherError::storage(error.to_string()))?;
                    *removed_files += 1;
                    *freed_bytes += metadata.len();
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn clean_launcher_cache() -> Result<CacheCleanResult, LauncherError> {
    let root = launcher_data_directory()?;
    let mut removed_files = 0u64;
    let mut freed_bytes = 0u64;
    for target in [root.join("cache"), root.join("runtimes").join(".downloads")] {
        if target.is_dir() {
            delete_cache_tree(&target, true, &mut removed_files, &mut freed_bytes)?;
        }
    }
    for base in [root.join("instances"), root.join("runtimes")] {
        if base.is_dir() {
            delete_cache_tree(&base, false, &mut removed_files, &mut freed_bytes)?;
        }
    }
    Ok(CacheCleanResult {
        removed_files,
        freed_bytes,
    })
}

#[tauri::command]
fn backup_world(app: AppHandle, content_id: i64) -> Result<RemovedContent, LauncherError> {
    let connection = open_database(&app)?;
    let (file_name, root): (String, String) = connection
        .query_row(
            "SELECT c.file_name,i.root_path FROM content_items c JOIN instances i ON i.id=c.instance_id WHERE c.id=?1 AND c.kind='world'",
            [content_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| LauncherError::validation("存档记录不存在。"))?;
    let source = PathBuf::from(root)
        .join(".minecraft")
        .join("saves")
        .join(&file_name);
    if !source.is_dir() {
        return Err(LauncherError::validation("存档目录不存在。"));
    }
    let backup_root = launcher_data_directory()?.join("backups").join("worlds");
    fs::create_dir_all(&backup_root).map_err(|error| LauncherError::storage(error.to_string()))?;
    let destination = backup_root.join(format!(
        "{}-{}-{}",
        content_id,
        unique_timestamp(),
        file_name
    ));
    copy_world_directory(&source, &destination)?;
    Ok(RemovedContent {
        id: content_id,
        backup_path: destination.to_string_lossy().to_string(),
    })
}

#[tauri::command]
fn duplicate_world(app: AppHandle, content_id: i64) -> Result<ContentItem, LauncherError> {
    let connection = open_database(&app)?;
    let (instance_id, file_name, root): (i64, String, String) = connection
        .query_row(
            "SELECT c.instance_id,c.file_name,i.root_path FROM content_items c JOIN instances i ON i.id=c.instance_id WHERE c.id=?1 AND c.kind='world'",
            [content_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| LauncherError::validation("存档记录不存在。"))?;
    validate_instance_field(&file_name, 160)?;
    let saves = PathBuf::from(root).join(".minecraft").join("saves");
    let source = saves.join(&file_name);
    if !source.join("level.dat").is_file() {
        return Err(LauncherError::validation("存档不完整，缺少 level.dat。"));
    }
    let duplicate_name = format!("{}-副本-{}", file_name, unique_timestamp());
    let destination = saves.join(&duplicate_name);
    let file_count = copy_world_directory(&source, &destination)?;
    let hash = sha256_file_sync(&destination.join("level.dat"))?;
    let installed_at = chrono_like_timestamp();
    let metadata =
        serde_json::json!({"fileCount":file_count,"duplicatedFrom":file_name}).to_string();
    if let Err(error) = connection.execute(
        "INSERT INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,'world',?2,?3,?4,1,'duplicate',?5)",
        params![instance_id, duplicate_name, hash, metadata, installed_at],
    ) {
        let _ = fs::remove_dir_all(&destination);
        return Err(LauncherError::storage(format!("复制存档记录失败：{error}")));
    }
    Ok(ContentItem {
        id: connection.last_insert_rowid(),
        instance_id,
        kind: "world".into(),
        file_name: duplicate_name,
        hash,
        metadata_json: Some(metadata),
        enabled: true,
        source: "duplicate".into(),
        installed_at,
    })
}

fn removed_backup_directory(root: &Path, kind: &str) -> Result<PathBuf, LauncherError> {
    let directory = match kind {
        "mod" => "mods",
        "resourcepack" => "resourcepacks",
        "shaderpack" => "shaderpacks",
        "world" => "saves",
        _ => return Err(LauncherError::validation("不支持的备份类型。")),
    };
    Ok(root
        .join(".minecraft")
        .join(".launcher-backup")
        .join(directory))
}

fn backup_original_name(backup_name: &str) -> Result<String, LauncherError> {
    validate_instance_field(backup_name, 260)?;
    let (stamp, original) = backup_name
        .split_once('-')
        .ok_or_else(|| LauncherError::validation("备份文件名无效。"))?;
    if stamp.is_empty() || !stamp.bytes().all(|value| value.is_ascii_digit()) || original.is_empty()
    {
        return Err(LauncherError::validation("备份文件名无效。"));
    }
    validate_instance_field(original, 240)?;
    Ok(original.to_string())
}

fn backup_entry_size(path: &Path) -> u64 {
    if path.is_file() {
        return path.metadata().map(|value| value.len()).unwrap_or(0);
    }
    let mut total = 0u64;
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                total =
                    total.saturating_add(entry.metadata().map(|value| value.len()).unwrap_or(0));
            }
        }
    }
    total
}

#[tauri::command]
fn list_removed_backups(
    app: AppHandle,
    instance_id: i64,
    kind: String,
) -> Result<Vec<BackupItem>, LauncherError> {
    let connection = open_database(&app)?;
    let root: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    let directory = removed_backup_directory(Path::new(&root), &kind)?;
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut backups = fs::read_dir(&directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let backup_name = entry.file_name().to_str()?.to_string();
            let original_name = backup_original_name(&backup_name).ok()?;
            Some(BackupItem {
                kind: kind.clone(),
                backup_name,
                original_name,
                size: backup_entry_size(&entry.path()),
            })
        })
        .collect::<Vec<_>>();
    backups.sort_by(|left, right| right.backup_name.cmp(&left.backup_name));
    Ok(backups)
}

#[tauri::command]
fn restore_removed_backup(
    app: AppHandle,
    instance_id: i64,
    kind: String,
    backup_name: String,
) -> Result<ContentItem, LauncherError> {
    let connection = open_database(&app)?;
    let root: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("目标实例不存在。"))?;
    drop(connection);
    let original_name = backup_original_name(&backup_name)?;
    let backup_directory = removed_backup_directory(Path::new(&root), &kind)?;
    let source = backup_directory.join(&backup_name);
    if !source.exists() || !source.starts_with(&backup_directory) {
        return Err(LauncherError::validation("备份不存在。"));
    }
    let staging_root = launcher_data_directory()?
        .join("cache")
        .join("restore")
        .join(unique_timestamp().to_string());
    fs::create_dir_all(&staging_root).map_err(|error| LauncherError::storage(error.to_string()))?;
    let staged = staging_root.join(&original_name);
    let result = match kind.as_str() {
        "mod" => {
            fs::copy(&source, &staged)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            install_mod(
                app.clone(),
                instance_id,
                staged.to_string_lossy().to_string(),
            )
        }
        "resourcepack" | "shaderpack" => {
            fs::copy(&source, &staged)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            install_content_archive(
                app.clone(),
                instance_id,
                kind.clone(),
                staged.to_string_lossy().to_string(),
            )
        }
        "world" => {
            if !source.is_dir() {
                return Err(LauncherError::validation("存档备份不是有效目录。"));
            }
            copy_world_directory(&source, &staged)?;
            import_world(
                app.clone(),
                instance_id,
                staged.to_string_lossy().to_string(),
            )
        }
        _ => Err(LauncherError::validation("不支持的备份类型。")),
    };
    let _ = fs::remove_dir_all(&staging_root);
    let restored = result?;
    if source.is_dir() {
        fs::remove_dir_all(&source).map_err(|error| {
            LauncherError::storage(format!("恢复成功，但清理旧备份失败：{error}"))
        })?;
    } else {
        fs::remove_file(&source).map_err(|error| {
            LauncherError::storage(format!("恢复成功，但清理旧备份失败：{error}"))
        })?;
    }
    Ok(restored)
}

#[tauri::command]
fn list_instances(app: AppHandle) -> Result<Vec<Instance>, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection.prepare("SELECT id, name, root_path, game_version, loader_type, memory_mb, status, source FROM instances ORDER BY id DESC").map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok(Instance {
                id: row.get(0)?,
                name: row.get(1)?,
                root_path: row.get(2)?,
                game_version: row.get(3)?,
                loader_type: row.get(4)?,
                memory_mb: row.get(5)?,
                status: row.get(6)?,
                source: row.get(7)?,
            })
        })
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let instances = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(instances)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootJavaCheck {
    detected_count: usize,
    has_64_bit: bool,
    recommended_major: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootInstanceCheck {
    id: i64,
    name: String,
    game_version: String,
    loader_type: String,
    status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootIncompatibleMod {
    file_name: String,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootProblemMod {
    file_name: String,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootModsSummary {
    instance_id: i64,
    mod_count: usize,
    missing_dependencies: Vec<String>,
    incompatible_mods: Vec<BootIncompatibleMod>,
    problem_mods: Vec<BootProblemMod>,
}

fn loaders_compatible(instance_loader: &str, mod_loader: &str) -> bool {
    if instance_loader.eq_ignore_ascii_case(mod_loader) {
        return true;
    }
    // Quilt 可以运行绝大多数 Fabric 模组，不算不兼容。
    instance_loader.eq_ignore_ascii_case("quilt") && mod_loader.eq_ignore_ascii_case("fabric")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootHealthReport {
    java: BootJavaCheck,
    instances: Vec<BootInstanceCheck>,
    mods: Vec<BootModsSummary>,
}

fn scan_boot_mods(
    instance_id: i64,
    root_path: &str,
    loader_type: &str,
    game_version: &str,
) -> BootModsSummary {
    let mut inspections = Vec::new();
    if loader_type != "vanilla" {
        let mods = PathBuf::from(root_path).join(".minecraft").join("mods");
        if let Ok(entries) = fs::read_dir(&mods) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("jar"))
                {
                    if let Ok(inspection) = inspect_mod_jar_path(&path) {
                        inspections.push(inspection);
                    }
                }
            }
        }
    }
    let mod_count = inspections.len();
    let installed_ids = installed_mod_ids(&inspections);
    let mods_path = PathBuf::from(&root_path).join(".minecraft").join("mods");
    let kotlin_forge_present = has_kotlinforforge_file(&mods_path);
    let provided = [
        "minecraft",
        "java",
        "fabricloader",
        "fabric-loader",
        "quilt_loader",
        "quilt-loader",
        "forge",
        "neoforge",
    ];
    let missing = missing_dependencies(
        inspections
            .iter()
            .flat_map(|inspection| inspection.dependencies.iter().map(|id| id.as_str())),
        &installed_ids,
        kotlin_forge_present,
    );
    let mut incompatible_mods = Vec::new();
    let mut problem_mods = Vec::new();
    for inspection in &inspections {
        let mut reasons = Vec::new();
        if inspection.loader_type != "unknown"
            && !loaders_compatible(loader_type, &inspection.loader_type)
        {
            let reason = format!("需要 {}，当前实例为 {loader_type}", inspection.loader_type);
            incompatible_mods.push(BootIncompatibleMod {
                file_name: inspection.file_name.clone(),
                reason: reason.clone(),
            });
            reasons.push(reason);
        } else if let Err(error) = ensure_game_version_compatible(game_version, inspection) {
            incompatible_mods.push(BootIncompatibleMod {
                file_name: inspection.file_name.clone(),
                reason: error.message.clone(),
            });
            reasons.push(error.message.clone());
        }
        let missing_deps = inspection
            .dependencies
            .iter()
            .map(|id| id.to_ascii_lowercase())
            .filter(|id| {
                !provided.contains(&id.as_str())
                    && !(id == "kotlinforforge" && kotlin_forge_present)
                    && !installed_ids.contains(id)
            })
            .collect::<Vec<_>>();
        if !missing_deps.is_empty() {
            reasons.push(format!(
                "缺少前置模组：{}（启动时会尝试自动补齐）",
                missing_deps.join("、")
            ));
        }
        if !reasons.is_empty() {
            problem_mods.push(BootProblemMod {
                file_name: inspection.file_name.clone(),
                reason: reasons.join("；"),
            });
        }
    }
    BootModsSummary {
        instance_id,
        mod_count,
        missing_dependencies: missing.into_iter().collect(),
        incompatible_mods,
        problem_mods,
    }
}

#[tauri::command]
fn boot_health_check(app: AppHandle) -> Result<BootHealthReport, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection
        .prepare(
            "SELECT id, name, root_path, game_version, loader_type, status FROM instances ORDER BY id DESC",
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut instances = Vec::new();
    let mut mods = Vec::new();
    for row in rows {
        let (id, name, root_path, game_version, loader_type, status) =
            row.map_err(|error| LauncherError::storage(error.to_string()))?;
        instances.push(BootInstanceCheck {
            id,
            name,
            game_version: game_version.clone(),
            loader_type: loader_type.clone(),
            status,
        });
        mods.push(scan_boot_mods(id, &root_path, &loader_type, &game_version));
    }
    let runtimes = detect_java_runtimes();
    let has_64_bit = runtimes.iter().any(|runtime| runtime.is_64_bit);
    let recommended_major = runtimes
        .iter()
        .filter(|runtime| runtime.is_64_bit)
        .filter_map(|runtime| runtime.major_version)
        .max();
    Ok(BootHealthReport {
        java: BootJavaCheck {
            detected_count: runtimes.len(),
            has_64_bit,
            recommended_major,
        },
        instances,
        mods,
    })
}

#[tauri::command]
fn create_vanilla_instance(
    app: AppHandle,
    name: String,
    game_version: String,
) -> Result<Instance, LauncherError> {
    create_instance_profile(app, name, game_version, "vanilla".into())
}

#[tauri::command]
fn create_instance_profile(
    app: AppHandle,
    name: String,
    game_version: String,
    loader_type: String,
) -> Result<Instance, LauncherError> {
    validate_instance_field(name.trim(), 64)?;
    validate_instance_field(game_version.trim(), 64)?;
    let loader_type = loader_type.trim().to_ascii_lowercase();
    validate_loader_type(&loader_type)?;
    let connection = open_database(&app)?;
    let created_at = chrono_like_timestamp();
    let base = launcher_data_directory()?.join("instances");
    fs::create_dir_all(&base).map_err(|error| LauncherError::storage(error.to_string()))?;
    let provisional_path = base
        .join(format!("pending-{}", unique_timestamp()))
        .to_string_lossy()
        .to_string();
    let status = if loader_type == "vanilla" {
        "missing"
    } else {
        "base_missing"
    };
    connection.execute("INSERT INTO instances(name, root_path, game_version, loader_type, memory_mb, status, source, created_at) VALUES(?1, ?2, ?3, ?4, 4096, ?5, 'new', ?6)", params![name.trim(), provisional_path, game_version.trim(), loader_type, status, created_at]).map_err(|error| LauncherError::storage(error.to_string()))?;
    let id = connection.last_insert_rowid();
    let root = base.join(id.to_string());
    let game = root.join(".minecraft");
    for directory in [
        "mods",
        "config",
        "versions",
        "logs",
        "saves",
        "resourcepacks",
        "shaderpacks",
    ] {
        if let Err(error) = fs::create_dir_all(game.join(directory)) {
            let _ = connection.execute("DELETE FROM instances WHERE id=?1", [id]);
            return Err(LauncherError::storage(error.to_string()));
        }
    }
    let root_path = root.to_string_lossy().to_string();
    connection
        .execute(
            "UPDATE instances SET root_path=?1 WHERE id=?2",
            params![root_path, id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(Instance {
        id,
        name: name.trim().into(),
        root_path,
        game_version: game_version.trim().into(),
        loader_type,
        memory_mb: 4096,
        status: status.into(),
        source: "new".into(),
    })
}

#[tauri::command]
fn rename_instance(
    app: AppHandle,
    instance_id: i64,
    name: String,
) -> Result<Instance, LauncherError> {
    validate_instance_field(name.trim(), 64)?;
    let connection = open_database(&app)?;
    connection
        .execute(
            "UPDATE instances SET name=?1 WHERE id=?2",
            params![name.trim(), instance_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if connection.changes() == 0 {
        return Err(LauncherError::validation("实例不存在。"));
    }
    connection.query_row(
        "SELECT id,name,root_path,game_version,loader_type,memory_mb,status,source FROM instances WHERE id=?1",
        [instance_id],
        |row| Ok(Instance { id:row.get(0)?, name:row.get(1)?, root_path:row.get(2)?, game_version:row.get(3)?, loader_type:row.get(4)?, memory_mb:row.get(5)?, status:row.get(6)?, source:row.get(7)? }),
    ).map_err(|error| LauncherError::storage(error.to_string()))
}

#[tauri::command]
fn update_instance_memory(
    app: AppHandle,
    instance_id: i64,
    memory_mb: i64,
) -> Result<Instance, LauncherError> {
    if !(2048..=65536).contains(&memory_mb) {
        return Err(LauncherError::validation("内存须在 2048–65536 MB 之间。"));
    }
    let connection = open_database(&app)?;
    connection
        .execute(
            "UPDATE instances SET memory_mb=?1 WHERE id=?2",
            params![memory_mb, instance_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    connection
        .query_row(
            "SELECT id,name,root_path,game_version,loader_type,memory_mb,status,source FROM instances WHERE id=?1",
            [instance_id],
            |row| {
                Ok(Instance {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    root_path: row.get(2)?,
                    game_version: row.get(3)?,
                    loader_type: row.get(4)?,
                    memory_mb: row.get(5)?,
                    status: row.get(6)?,
                    source: row.get(7)?,
                })
            },
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))
}

#[tauri::command]
fn clone_instance(
    app: AppHandle,
    instance_id: i64,
    name: String,
) -> Result<Instance, LauncherError> {
    validate_instance_field(name.trim(), 64)?;
    let connection = open_database(&app)?;
    if running_games()
        .lock()
        .map_err(|_| LauncherError::storage("无法读取游戏运行状态。"))?
        .contains_key(&instance_id)
    {
        return Err(LauncherError::validation("实例正在运行，不能复制。"));
    }
    let (source_root, version, loader, loader_version, memory_mb, resolution, icon, java_profile): (
        String,
        String,
        String,
        Option<String>,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = connection.query_row(
        "SELECT root_path,game_version,loader_type,loader_version,memory_mb,resolution,icon,java_profile FROM instances WHERE id=?1",
        [instance_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        },
    ).map_err(|_| LauncherError::validation("实例不存在。"))?;
    drop(connection);
    let mut cloned = create_instance_profile(app.clone(), name, version, loader.clone())?;
    let source_game = PathBuf::from(source_root).join(".minecraft");
    let target_game = PathBuf::from(&cloned.root_path).join(".minecraft");
    for directory in [
        "mods",
        "config",
        "saves",
        "resourcepacks",
        "shaderpacks",
        "versions",
        "libraries",
        "assets",
    ] {
        copy_directory_contents(&source_game.join(directory), &target_game.join(directory))?;
    }
    let cloned_status = if loader == "vanilla" {
        "missing"
    } else {
        "loader_missing"
    };
    let connection = open_database(&app)?;
    connection
        .execute(
            "UPDATE instances SET loader_version=?1,memory_mb=?2,resolution=?3,icon=?4,java_profile=?5,status=?6,source='clone' WHERE id=?7",
            params![
                loader_version,
                memory_mb,
                resolution,
                icon,
                java_profile,
                cloned_status,
                cloned.id
            ],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    connection
        .execute(
            "INSERT INTO instance_launch_settings(instance_id, memory_min_mb, memory_max_mb, java_mode, java_path, jvm_args_json, game_args_json, width, height, account_id)
             SELECT ?1, memory_min_mb, memory_max_mb, java_mode, java_path, jvm_args_json, game_args_json, width, height, account_id
             FROM instance_launch_settings WHERE instance_id=?2",
            params![cloned.id, instance_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    connection
        .execute(
            "INSERT INTO instance_pack_source(instance_id, provider, project_id, version_id, pack_version, source_url, installed_at)
             SELECT ?1, provider, project_id, version_id, pack_version, source_url, installed_at
             FROM instance_pack_source WHERE instance_id=?2",
            params![cloned.id, instance_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    {
        let mut statement = connection
            .prepare("SELECT kind,file_name,hash,metadata_json,enabled FROM content_items WHERE instance_id=?1")
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let rows = statement
            .query_map([instance_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        drop(statement);
        for (kind, file_name, hash, metadata_json, enabled) in rows {
            connection
                .execute(
                    "INSERT OR IGNORE INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at)
                     VALUES(?1,?2,?3,?4,?5,?6,'clone',?7)",
                    rusqlite::params![
                        cloned.id,
                        kind,
                        file_name,
                        hash,
                        metadata_json,
                        enabled,
                        chrono_like_timestamp()
                    ],
                )
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
    }
    connection
        .execute(
            "INSERT INTO managed_content(id, instance_id, kind, provider, project_id, version_id, file_sha1, file_sha256, installed_path, installed_by_launcher, created_at)
             SELECT 'clone-' || ?1 || '-' || id, ?1, kind, provider, project_id, version_id, file_sha1, file_sha256, installed_path, 0, ?2
             FROM managed_content WHERE instance_id=?3",
            params![cloned.id, chrono_like_timestamp(), instance_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    cloned.memory_mb = memory_mb;
    cloned.status = cloned_status.into();
    cloned.source = "clone".into();
    Ok(cloned)
}

#[tauri::command]
fn delete_instance_to_backup(
    app: AppHandle,
    instance_id: i64,
) -> Result<RemovedContent, LauncherError> {
    let connection = open_database(&app)?;
    if running_games()
        .lock()
        .map_err(|_| LauncherError::storage("无法读取游戏运行状态。"))?
        .contains_key(&instance_id)
    {
        return Err(LauncherError::validation("实例正在运行，不能删除。"));
    }
    let (name, root, game_version, loader_type, memory_mb, status, source): (
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
    ) = connection
        .query_row(
            "SELECT name, root_path, game_version, loader_type, memory_mb, status, source FROM instances WHERE id=?1",
            [instance_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))?;
    let root = PathBuf::from(root);
    let instances_root = launcher_data_directory()?.join("instances");
    if !root.starts_with(&instances_root) || root == instances_root {
        return Err(LauncherError::validation(
            "实例目录不在安全范围内，已拒绝删除。",
        ));
    }
    let backup_root = launcher_data_directory()?
        .join("backups")
        .join("deleted-instances");
    fs::create_dir_all(&backup_root).map_err(|error| LauncherError::storage(error.to_string()))?;
    let destination = backup_root.join(format!("{}-{}", instance_id, unique_timestamp()));
    let mut transaction = fs_safe::FsTransaction::new(format!("delete-instance-{instance_id}"));
    if root.exists() {
        transaction.move_with_undo(&root, &destination)?;
    }
    let size_bytes = storage::directory_size(&destination);
    let instance_json = serde_json::to_string(&serde_json::json!({
        "name": name,
        "root_path": root.to_string_lossy(),
        "game_version": game_version,
        "loader_type": loader_type,
        "memory_mb": memory_mb,
        "status": status,
        "source": source,
    }))
    .map_err(|error| LauncherError::storage(error.to_string()))?;
    if let Err(error) = storage::record_deleted_instance(
        &connection,
        instance_id,
        &name,
        &destination.to_string_lossy(),
        size_bytes,
        &instance_json,
    ) {
        transaction.rollback()?;
        return Err(error);
    }
    if let Err(error) = connection.execute("DELETE FROM instances WHERE id=?1", [instance_id]) {
        transaction.rollback()?;
        return Err(LauncherError::storage(error.to_string()));
    }
    transaction.commit();
    Ok(RemovedContent {
        id: instance_id,
        backup_path: destination.to_string_lossy().to_string(),
    })
}

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MAX_MANIFEST_BYTES: usize = 10 * 1024 * 1024;
const MAX_VERSION_JSON_BYTES: usize = 20 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
struct LatestVersions {
    release: String,
    snapshot: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct MinecraftVersionSummary {
    id: String,
    #[serde(rename = "type")]
    version_type: String,
    url: String,
    sha1: String,
    compliance_level: Option<i64>,
}

#[derive(Deserialize, Serialize)]
struct VersionManifest {
    latest: LatestVersions,
    versions: Vec<MinecraftVersionSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VanillaInstallPreview {
    instance_id: i64,
    game_version: String,
    client_bytes: u64,
    library_count: usize,
    library_bytes: u64,
    java_major_version: Option<u64>,
    main_class: String,
}

fn install_preview_from_details(
    instance_id: i64,
    details: &serde_json::Value,
) -> Result<VanillaInstallPreview, LauncherError> {
    let game_version = details
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("版本元数据缺少 id。"))?
        .to_string();
    let client_bytes = details
        .pointer("/downloads/client/size")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| LauncherError::storage("版本元数据缺少客户端大小。"))?;
    let libraries = details
        .get("libraries")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LauncherError::storage("版本元数据缺少 libraries。"))?;
    let (library_count, library_bytes) = libraries
        .iter()
        .filter_map(|library| {
            library
                .pointer("/downloads/artifact/size")
                .and_then(|value| value.as_u64())
        })
        .fold((0usize, 0u64), |(count, bytes), size| {
            (count + 1, bytes.saturating_add(size))
        });
    let main_class = details
        .get("mainClass")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("版本元数据缺少 mainClass。"))?
        .to_string();
    Ok(VanillaInstallPreview {
        instance_id,
        game_version,
        client_bytes,
        library_count,
        library_bytes,
        java_major_version: details
            .pointer("/javaVersion/majorVersion")
            .and_then(|value| value.as_u64()),
        main_class,
    })
}

#[tauri::command]
async fn preview_vanilla_install(
    app: AppHandle,
    instance_id: i64,
    url: String,
    expected_sha1: String,
) -> Result<VanillaInstallPreview, LauncherError> {
    let connection = open_database(&app)?;
    let expected_version: String = connection
        .query_row(
            "SELECT game_version FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|error| LauncherError::storage(format!("实例不存在：{error}")))?;
    drop(connection);
    let details = fetch_version_details(url, expected_sha1).await?;
    let preview = install_preview_from_details(instance_id, &details)?;
    if preview.game_version != expected_version {
        return Err(LauncherError::validation("版本元数据与实例版本不一致。"));
    }
    Ok(preview)
}

fn parse_version_manifest(bytes: &[u8]) -> Result<VersionManifest, LauncherError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(LauncherError::validation("版本清单超过安全大小限制。"));
    }
    serde_json::from_slice(bytes)
        .map_err(|error| LauncherError::storage(format!("版本清单格式无效：{error}")))
}

fn verify_sha1(bytes: &[u8], expected: &str) -> bool {
    format!("{:x}", Sha1::digest(bytes)).eq_ignore_ascii_case(expected)
}

fn validate_metadata_url(value: &str) -> Result<reqwest::Url, LauncherError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| LauncherError::validation("版本元数据 URL 无效。"))?;
    let allowed_host = matches!(
        url.host_str(),
        Some("piston-meta.mojang.com")
            | Some("launchermeta.mojang.com")
            | Some("bmclapi2.bangbang93.com")
    );
    if url.scheme() != "https" || !allowed_host {
        return Err(LauncherError::validation(
            "仅允许 Mojang 官方 HTTPS 元数据来源。",
        ));
    }
    Ok(url)
}

fn validate_resource_url(value: &str) -> Result<reqwest::Url, LauncherError> {
    // Some official loader metadata contains a trailing line break in URL values.
    // Treat surrounding ASCII/Unicode whitespace as formatting, not as part of the URL.
    let url = reqwest::Url::parse(value.trim())
        .map_err(|_| LauncherError::validation("下载地址格式不正确。请刷新后重试。"))?;
    let allowed_host = matches!(
        url.host_str(),
        Some("piston-data.mojang.com")
            | Some("piston-meta.mojang.com")
            | Some("launcher.mojang.com")
            | Some("libraries.minecraft.net")
            | Some("resources.download.minecraft.net")
            | Some("maven.fabricmc.net")
            | Some("maven.quiltmc.org")
            | Some("maven.minecraftforge.net")
            | Some("maven.neoforged.net")
            | Some("cdn.modrinth.com")
            | Some("bmclapi2.bangbang93.com")
            | Some("www.curseforge.com")
            | Some("edge.forgecdn.net")
    );
    if url.scheme() != "https" || !allowed_host {
        return Err(LauncherError::validation(
            "仅允许 Minecraft 官方 HTTPS 下载来源。",
        ));
    }
    Ok(url)
}

/// 把 Mojang 官方下载地址映射为 BMCLAPI 国内镜像地址。
/// 所有镜像文件下载后仍按 SHA-1 / 大小校验，镜像无法篡改内容。
fn bmclapi_mirror_url(original: &reqwest::Url) -> Option<reqwest::Url> {
    let host = original.host_str()?;
    let path = original.path();
    let mirror = match host {
        "piston-meta.mojang.com" | "launchermeta.mojang.com" => {
            if path.ends_with("version_manifest.json") || path.ends_with("version_manifest_v2.json")
            {
                "https://bmclapi2.bangbang93.com/mc/game/version_manifest.json".to_string()
            } else {
                let stem = path
                    .rsplit('/')
                    .next()
                    .and_then(|name| name.strip_suffix(".json"))
                    .filter(|stem| !stem.is_empty())?;
                format!("https://bmclapi2.bangbang93.com/version/{stem}/json")
            }
        }
        "piston-data.mojang.com" => format!("https://bmclapi2.bangbang93.com{path}"),
        "resources.download.minecraft.net" => {
            format!("https://bmclapi2.bangbang93.com/assets{path}")
        }
        "libraries.minecraft.net"
        | "maven.fabricmc.net"
        | "maven.quiltmc.org"
        | "maven.minecraftforge.net"
        | "maven.neoforged.net" => format!("https://bmclapi2.bangbang93.com/maven{path}"),
        _ => return None,
    };
    reqwest::Url::parse(&mirror).ok()
}

fn validate_loader_token(value: &str) -> Result<(), LauncherError> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '+' | '-' | '_')
        })
    {
        return Err(LauncherError::validation("加载器版本标识无效。"));
    }
    Ok(())
}

fn loader_meta_base(loader: &str) -> Result<&'static str, LauncherError> {
    match loader {
        "fabric" => Ok("https://meta.fabricmc.net/v2"),
        "quilt" => Ok("https://meta.quiltmc.org/v3"),
        _ => Err(LauncherError::validation(
            "此加载器不使用 profile 元数据安装。",
        )),
    }
}

async fn fetch_loader_json(url: &str) -> Result<serde_json::Value, LauncherError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| LauncherError::validation("加载器元数据 URL 无效。"))?;
    if parsed.scheme() != "https"
        || !matches!(
            parsed.host_str(),
            Some("meta.fabricmc.net") | Some("meta.quiltmc.org")
        )
    {
        return Err(LauncherError::validation(
            "仅允许 Fabric/Quilt 官方 HTTPS 元数据来源。",
        ));
    }
    let client = shared_download_client()?;
    let response = send_download_request(&client, &parsed, None)
        .await?
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("加载器元数据服务返回错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|size| size > 10 * 1024 * 1024)
    {
        return Err(LauncherError::validation("加载器元数据超过安全限制。"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(LauncherError::validation("加载器元数据超过安全限制。"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(format!("加载器元数据无效：{error}")))
}

async fn fetch_official_loader_text(url: &str) -> Result<String, LauncherError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| LauncherError::validation("加载器元数据 URL 无效。"))?;
    if parsed.scheme() != "https"
        || !matches!(
            parsed.host_str(),
            Some("files.minecraftforge.net") | Some("maven.neoforged.net")
        )
    {
        return Err(LauncherError::validation(
            "仅允许 Forge/NeoForge 官方 HTTPS 元数据来源。",
        ));
    }
    let client = shared_download_client()?;
    let response = send_download_request(&client, &parsed, None)
        .await?
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("加载器元数据服务返回错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|size| size > 10 * 1024 * 1024)
    {
        return Err(LauncherError::validation("加载器元数据超过安全限制。"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(LauncherError::validation("加载器元数据超过安全限制。"));
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|_| LauncherError::validation("加载器元数据不是有效 UTF-8。"))
}

fn neoforge_game_prefix(game_version: &str) -> Result<String, LauncherError> {
    let normalized = game_version.strip_prefix("1.").unwrap_or(game_version);
    let mut parts = normalized.split('.');
    let first = parts.next().unwrap_or_default();
    let second = parts.next().unwrap_or_default();
    if first.is_empty()
        || second.is_empty()
        || !first.chars().all(|value| value.is_ascii_digit())
        || !second.chars().all(|value| value.is_ascii_digit())
    {
        return Err(LauncherError::validation(
            "该 Minecraft 版本无法映射到 NeoForge 版本规则。",
        ));
    }
    Ok(format!("{first}.{second}."))
}

#[tauri::command]
async fn list_loader_versions(
    loader_type: String,
    game_version: String,
) -> Result<Vec<String>, LauncherError> {
    let loader = loader_type.trim().to_ascii_lowercase();
    validate_loader_token(&game_version)?;
    let mut versions = Vec::new();
    match loader.as_str() {
        "fabric" | "quilt" => {
            let base = loader_meta_base(&loader)?;
            let value =
                fetch_loader_json(&format!("{base}/versions/loader/{game_version}")).await?;
            let entries = value
                .as_array()
                .ok_or_else(|| LauncherError::storage("加载器版本列表格式无效。"))?;
            for entry in entries {
                if let Some(version) = entry
                    .pointer("/loader/version")
                    .and_then(|value| value.as_str())
                {
                    validate_loader_token(version)?;
                    versions.push(version.to_string());
                }
            }
        }
        "forge" => {
            let text = fetch_official_loader_text(
                "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
            )
            .await?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|error| LauncherError::storage(format!("Forge 版本列表无效：{error}")))?;
            for channel in ["recommended", "latest"] {
                if let Some(version) = value
                    .pointer(&format!("/promos/{game_version}-{channel}"))
                    .and_then(|value| value.as_str())
                {
                    validate_loader_token(version)?;
                    if !versions.iter().any(|existing| existing == version) {
                        versions.push(version.to_string());
                    }
                }
            }
        }
        "neoforge" => {
            let prefix = neoforge_game_prefix(&game_version)?;
            let text = fetch_official_loader_text(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml",
            )
            .await?;
            for fragment in text.split("<version>").skip(1) {
                let Some(version) = fragment.split("</version>").next() else {
                    continue;
                };
                if version.starts_with(&prefix) {
                    validate_loader_token(version)?;
                    versions.push(version.to_string());
                }
            }
            versions.reverse();
        }
        _ => return Err(LauncherError::validation("不支持的加载器类型。")),
    }
    if versions.is_empty() {
        return Err(LauncherError::validation(
            "该 Minecraft 版本没有可用的加载器版本。",
        ));
    }
    Ok(versions)
}

fn maven_artifact_path(coordinate: &str) -> Result<String, LauncherError> {
    let (coordinate, extension) = coordinate
        .split_once('@')
        .map_or((coordinate, "jar"), |(left, right)| (left, right));
    let parts: Vec<_> = coordinate.split(':').collect();
    if !(3..=4).contains(&parts.len())
        || parts
            .iter()
            .any(|part| part.is_empty() || part.contains(['/', '\\']))
        || !extension.chars().all(|value| value.is_ascii_alphanumeric())
    {
        return Err(LauncherError::validation("Maven 依赖坐标无效。"));
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts
        .get(3)
        .map(|value| format!("-{value}"))
        .unwrap_or_default();
    Ok(format!(
        "{group}/{artifact}/{version}/{artifact}-{version}{classifier}.{extension}"
    ))
}

async fn fetch_sha1_sidecar(url: &str) -> Result<String, LauncherError> {
    let parsed = validate_resource_url(url)?;
    let client = shared_download_client()?;
    let response = send_download_request(&client, &parsed, None)
        .await?
        .error_for_status()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let text = response
        .text()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let hash = text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if hash.len() != 40 || !hash.chars().all(|value| value.is_ascii_hexdigit()) {
        return Err(LauncherError::validation("加载器依赖 SHA-1 sidecar 无效。"));
    }
    Ok(hash)
}

async fn download_file_with_local_hash(
    app: &AppHandle,
    instance_id: i64,
    url: &str,
    target: &Path,
) -> Result<String, LauncherError> {
    let parsed = validate_resource_url(url)?;
    let client = shared_download_client()?;
    let response = send_download_request(&client, &parsed, None)
        .await?
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("安装器下载失败：{error}")))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(format!("安装器下载内容读取失败：{error}")))?;
    if bytes.len() > 512 * 1024 * 1024 {
        return Err(LauncherError::validation("Forge 安装器超过安全大小限制。"));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let part = target.with_extension("part");
    tokio::fs::write(&part, &bytes)
        .await
        .map_err(|error| LauncherError::storage(format!("保存 Forge 安装器失败：{error}")))?;
    let _ = tokio::fs::remove_file(target).await;
    tokio::fs::rename(&part, target)
        .await
        .map_err(|error| LauncherError::storage(format!("完成 Forge 安装器保存失败：{error}")))?;
    let hash = format!("{:x}", Sha1::digest(&bytes));
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            instance_id,
            downloaded_bytes: bytes.len() as u64,
            total_bytes: Some(bytes.len() as u64),
            ..Default::default()
        },
    );
    Ok(hash)
}

fn merge_loader_profile(
    base: &mut serde_json::Value,
    profile: &serde_json::Value,
    libraries: Vec<serde_json::Value>,
) -> Result<(), LauncherError> {
    let base_object = base
        .as_object_mut()
        .ok_or_else(|| LauncherError::storage("基础版本元数据无效。"))?;
    let profile_main = profile
        .get("mainClass")
        .cloned()
        .ok_or_else(|| LauncherError::storage("加载器 profile 缺少 mainClass。"))?;
    base_object.insert("mainClass".into(), profile_main);
    let base_libraries = base_object
        .get_mut("libraries")
        .and_then(|value| value.as_array_mut())
        .ok_or_else(|| LauncherError::storage("基础版本元数据缺少 libraries。"))?;
    let mut known_artifacts = base_libraries
        .iter()
        .filter_map(|library| {
            library
                .pointer("/downloads/artifact/path")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or_else(|| {
                    library
                        .get("name")
                        .and_then(|value| value.as_str())
                        .and_then(|coordinate| maven_artifact_path(coordinate).ok())
                })
        })
        .collect::<HashSet<_>>();
    for library in libraries {
        let artifact = library
            .pointer("/downloads/artifact/path")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| {
                library
                    .get("name")
                    .and_then(|value| value.as_str())
                    .and_then(|coordinate| maven_artifact_path(coordinate).ok())
            });
        if artifact.is_none_or(|path| known_artifacts.insert(path)) {
            base_libraries.push(library);
        }
    }
    for kind in ["jvm", "game"] {
        let additions = profile
            .pointer(&format!("/arguments/{kind}"))
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        if additions.is_empty() {
            continue;
        }
        let arguments = base_object
            .entry("arguments")
            .or_insert_with(|| serde_json::json!({"jvm":[],"game":[]}));
        let target = arguments
            .get_mut(kind)
            .and_then(|value| value.as_array_mut())
            .ok_or_else(|| LauncherError::storage("基础启动参数格式无效。"))?;
        target.extend(additions);
    }
    if let Some(loader_arguments) = profile
        .get("minecraftArguments")
        .and_then(|value| value.as_str())
    {
        let existing = base_object
            .get("minecraftArguments")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        base_object.insert(
            "minecraftArguments".into(),
            serde_json::Value::String(format!("{existing} {loader_arguments}").trim().to_string()),
        );
    }
    Ok(())
}

#[tauri::command]
async fn install_profile_loader(
    app: AppHandle,
    instance_id: i64,
    loader_version: String,
) -> Result<Instance, LauncherError> {
    download_cancel_flag().store(false, Ordering::Release);
    validate_loader_token(&loader_version)?;
    let connection = open_database(&app)?;
    let (name, root_path, game_version, loader_type, memory_mb, source): (String, String, String, String, i64, String) = connection.query_row("SELECT name,root_path,game_version,loader_type,memory_mb,source FROM instances WHERE id=?1", [instance_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).map_err(|_| LauncherError::validation("实例不存在。"))?;
    let installed: i64 = connection.query_row("SELECT COUNT(*) FROM installation_states WHERE instance_id=?1 AND component_kind='game' AND status='verified'", [instance_id], |row| row.get(0)).map_err(|error| LauncherError::storage(error.to_string()))?;
    if installed == 0 {
        return Err(LauncherError::validation("请先完成基础游戏安装。"));
    }
    let base = loader_meta_base(&loader_type)?;
    validate_loader_token(&game_version)?;
    drop(connection);
    let profile = fetch_loader_json(&format!(
        "{base}/versions/loader/{game_version}/{loader_version}/profile/json"
    ))
    .await?;
    if profile.get("inheritsFrom").and_then(|value| value.as_str()) != Some(game_version.as_str()) {
        return Err(LauncherError::validation(
            "加载器 profile 与 Minecraft 版本不匹配。",
        ));
    }
    let game = PathBuf::from(&root_path).join(".minecraft");
    let profile_libraries = profile
        .get("libraries")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LauncherError::storage("加载器 profile 缺少 libraries。"))?;
    let mut normalized = Vec::new();
    for library in profile_libraries {
        let name = library
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LauncherError::storage("加载器依赖缺少名称。"))?;
        let path = maven_artifact_path(name)?;
        let base_url = library
            .get("url")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LauncherError::storage("加载器依赖缺少 Maven URL。"))?;
        let url = format!("{}{path}", base_url.trim_end_matches('/').to_string() + "/");
        validate_resource_url(&url)?;
        let sha1 = if let Some(hash) = library.get("sha1").and_then(|value| value.as_str()) {
            hash.to_string()
        } else {
            fetch_sha1_sidecar(&format!("{url}.sha1")).await?
        };
        let size = library.get("size").and_then(|value| value.as_u64());
        let downloaded = download_verified_file(
            &app,
            instance_id,
            &url,
            &sha1,
            size,
            &game.join("libraries").join(&path),
        )
        .await?;
        let mut entry = library.clone();
        entry.as_object_mut().ok_or_else(|| LauncherError::storage("加载器依赖格式无效。"))?.insert("downloads".into(), serde_json::json!({"artifact":{"path":path,"url":url,"sha1":sha1,"size":downloaded}}));
        normalized.push(entry);
    }
    let base_path = game
        .join("versions")
        .join(&game_version)
        .join(format!("{game_version}.json"));
    let bytes = tokio::fs::read(&base_path)
        .await
        .map_err(|_| LauncherError::validation("基础版本元数据缺失。"))?;
    let mut effective: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    merge_loader_profile(&mut effective, &profile, normalized)?;
    tokio::fs::write(
        game.join("versions")
            .join(&game_version)
            .join("launcher-effective.json"),
        serde_json::to_vec_pretty(&effective)
            .map_err(|error| LauncherError::storage(error.to_string()))?,
    )
    .await
    .map_err(|error| LauncherError::storage(error.to_string()))?;
    let connection = open_database(&app)?;
    connection
        .execute(
            "UPDATE instances SET loader_version=?1,status='ready' WHERE id=?2",
            params![loader_version, instance_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    connection.execute("INSERT INTO installation_states(instance_id,component_kind,component_key,status) VALUES(?1,'loader',?2,'verified') ON CONFLICT(instance_id,component_kind,component_key) DO UPDATE SET status='verified'", params![instance_id, loader_version]).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(Instance {
        id: instance_id,
        name,
        root_path,
        game_version,
        loader_type,
        memory_mb,
        status: "ready".into(),
        source,
    })
}

fn locate_java_loader_profile(
    game: &Path,
    game_version: &str,
    loader_type: &str,
) -> Result<serde_json::Value, LauncherError> {
    let versions = game.join("versions");
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&versions)
        .map_err(|error| LauncherError::storage(format!("无法读取加载器版本目录：{error}")))?
    {
        let entry = entry.map_err(|error| LauncherError::storage(error.to_string()))?;
        if !entry
            .file_type()
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .is_dir()
        {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if !id.to_ascii_lowercase().contains(loader_type) {
            continue;
        }
        let profile_path = entry.path().join(format!("{id}.json"));
        if !profile_path.is_file() {
            continue;
        }
        let modified = profile_path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((modified, profile_path));
    }
    candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, profile_path) in candidates {
        let bytes = fs::read(&profile_path)
            .map_err(|error| LauncherError::storage(format!("读取加载器 profile 失败：{error}")))?;
        if bytes.len() > MAX_VERSION_JSON_BYTES {
            continue;
        }
        let profile: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| LauncherError::storage(format!("加载器 profile 无效：{error}")))?;
        if profile.get("inheritsFrom").and_then(|value| value.as_str()) == Some(game_version) {
            return Ok(profile);
        }
    }
    Err(LauncherError::validation(
        "官方安装器已结束，但未找到与该 Minecraft 版本匹配的加载器 profile。",
    ))
}

fn installer_profile_libraries(
    installer: &Path,
) -> Result<Vec<(String, String, Option<String>, Option<u64>)>, LauncherError> {
    let file =
        fs::File::open(installer).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| LauncherError::validation(format!("加载器安装 JAR 无效：{error}")))?;
    let mut profiles = Vec::new();
    for name in ["install_profile.json", "version.json"] {
        let Ok(mut entry) = archive.by_name(name) else {
            continue;
        };
        if entry.size() > 10 * 1024 * 1024 {
            return Err(LauncherError::validation(
                "加载器安装 profile 超过安全限制。",
            ));
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let profile: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| LauncherError::storage(format!("加载器安装 profile 无效：{error}")))?;
        profiles.push(profile);
    }
    let mut libraries = Vec::new();
    let mut paths = HashSet::new();
    for profile in profiles {
        let Some(entries) = profile.get("libraries").and_then(|value| value.as_array()) else {
            continue;
        };
        for library in entries {
            let coordinate = library
                .get("name")
                .and_then(|value| value.as_str())
                .ok_or_else(|| LauncherError::storage("安装器依赖缺少 Maven 坐标。"))?;
            let artifact = library.pointer("/downloads/artifact");
            let path = artifact
                .and_then(|value| value.get("path"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or(maven_artifact_path(coordinate)?);
            safe_relative_download_path(&path)?;
            if !paths.insert(path.clone()) {
                continue;
            }
            let url = artifact
                .and_then(|value| value.get("url"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    library
                        .get("url")
                        .and_then(|value| value.as_str())
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(|base| format!("{}/{path}", base.trim_end_matches('/')))
                });
            // Forge lists generated artifacts (for example its client jar) beside
            // downloadable libraries. They intentionally have no URL and are created
            // later by the official installer processors, so prefetch must skip them.
            let Some(url) = url else {
                continue;
            };
            validate_resource_url(&url)?;
            let sha1 = artifact
                .and_then(|value| value.get("sha1"))
                .and_then(|value| value.as_str())
                .or_else(|| library.get("sha1").and_then(|value| value.as_str()))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let size = artifact
                .and_then(|value| value.get("size"))
                .and_then(|value| value.as_u64())
                .or_else(|| library.get("size").and_then(|value| value.as_u64()));
            libraries.push((path, url, sha1, size));
        }
    }
    Ok(libraries)
}

async fn prefetch_installer_libraries(
    app: &AppHandle,
    instance_id: i64,
    installer: &Path,
    game: &Path,
) -> Result<usize, LauncherError> {
    let libraries = installer_profile_libraries(installer)?;
    let app = app.clone();
    let game = game.to_path_buf();
    let results =
        futures_util::stream::iter(libraries.into_iter().map(|(path, url, sha1, size)| {
            let app = app.clone();
            let game = game.clone();
            async move {
                let _permit = download_perf::download_concurrency()
                    .library
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| LauncherError::storage("下载并发控制异常。"))?;
                let expected = match sha1 {
                    Some(value) => value,
                    None => fetch_sha1_sidecar(&format!("{url}.sha1")).await?,
                };
                download_verified_file(
                    &app,
                    instance_id,
                    &url,
                    &expected,
                    size,
                    &game.join("libraries").join(path),
                )
                .await
            }
        }))
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
    for result in &results {
        if let Err(error) = result {
            return Err(LauncherError {
                code: error.code.clone(),
                message: format!("预取加载器依赖失败：{}", error.message),
                recoverable: true,
            });
        }
    }
    Ok(results.len())
}

#[tauri::command]
async fn install_java_loader(
    app: AppHandle,
    instance_id: i64,
    loader_version: String,
    java_path: String,
) -> Result<Instance, LauncherError> {
    download_cancel_flag().store(false, Ordering::Release);
    validate_loader_token(&loader_version)?;
    let java = PathBuf::from(&java_path);
    if !java.is_file()
        || java
            .file_name()
            .and_then(|value| value.to_str())
            .is_none_or(|name| !name.eq_ignore_ascii_case("java.exe"))
    {
        return Err(LauncherError::validation("Java 可执行文件无效。"));
    }
    let connection = open_database(&app)?;
    let (name, root_path, game_version, loader_type, memory_mb, source): (
        String,
        String,
        String,
        String,
        i64,
        String,
    ) = connection
        .query_row(
            "SELECT name,root_path,game_version,loader_type,memory_mb,source FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))?;
    if !matches!(loader_type.as_str(), "forge" | "neoforge") {
        return Err(LauncherError::validation(
            "该命令仅用于 Forge/NeoForge 官方安装器。",
        ));
    }
    let installed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM installation_states WHERE instance_id=?1 AND component_kind='game' AND status='verified'",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if installed == 0 {
        return Err(LauncherError::validation("请先完成基础游戏安装。"));
    }
    validate_loader_token(&game_version)?;
    drop(connection);

    let (installer_url, installer_name) = if loader_type == "forge" {
        let artifact_version = format!("{game_version}-{loader_version}");
        (
            format!(
                "https://maven.minecraftforge.net/net/minecraftforge/forge/{artifact_version}/forge-{artifact_version}-installer.jar"
            ),
            format!("forge-{artifact_version}-installer.jar"),
        )
    } else {
        (
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar"
            ),
            format!("neoforge-{loader_version}-installer.jar"),
        )
    };
    validate_resource_url(&installer_url)?;
    // Forge 的部分历史版本没有提供可访问的 .sha1 sidecar；此时下载后在本机计算 SHA-1，
    // 仍然使用 .part + 原子改名，避免把半截安装器当成完整文件。
    let installer_sha1 = fetch_sha1_sidecar(&format!("{installer_url}.sha1"))
        .await
        .ok();
    let game = PathBuf::from(&root_path).join(".minecraft");
    tokio::fs::create_dir_all(&game)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let profiles_path = game.join("launcher_profiles.json");
    if !tokio::fs::try_exists(&profiles_path)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?
    {
        tokio::fs::write(&profiles_path, b"{\"profiles\":{}}")
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let installer = game
        .join(".launcher-cache")
        .join("installers")
        .join(installer_name);
    if let Some(expected_sha1) = installer_sha1 {
        download_verified_file(
            &app,
            instance_id,
            &installer_url,
            &expected_sha1,
            None,
            &installer,
        )
        .await?;
    } else {
        download_file_with_local_hash(&app, instance_id, &installer_url, &installer).await?;
    }
    prefetch_installer_libraries(&app, instance_id, &installer, &game).await?;

    let logs = game.join("logs");
    tokio::fs::create_dir_all(&logs)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let installer_log = logs.join(format!("{loader_type}-installer.log"));
    tokio::fs::write(&installer_log, b"")
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let java_for_task = java.clone();
    let installer_for_task = installer.clone();
    let game_for_task = game.clone();
    let temp_for_task = game.join(".launcher-cache").join("tmp");
    tokio::fs::create_dir_all(&temp_for_task)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let log_for_task = installer_log.clone();
    let exit_status =
        tokio::task::spawn_blocking(move || -> Result<std::process::ExitStatus, LauncherError> {
            use std::io::Write as _;
            for attempt in 1..=3u32 {
                let mut stdout = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_for_task)
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
                let _ = writeln!(stdout, "\n=== Launcher installer attempt {attempt}/3 ===");
                let stderr = stdout
                    .try_clone()
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
                let mut child = Command::new(&java_for_task)
                    .arg(format!("-Djava.io.tmpdir={}", temp_for_task.display()))
                    .arg("-jar")
                    .arg(&installer_for_task)
                    .arg("--installClient")
                    .arg(&game_for_task)
                    .current_dir(&game_for_task)
                    .stdin(Stdio::null())
                    .stdout(Stdio::from(stdout))
                    .stderr(Stdio::from(stderr))
                    .spawn()
                    .map_err(|error| {
                        LauncherError::storage(format!("无法启动官方加载器安装器：{error}"))
                    })?;
                let deadline = std::time::Instant::now() + Duration::from_secs(15 * 60);
                loop {
                    if let Some(status) = child
                        .try_wait()
                        .map_err(|error| LauncherError::storage(error.to_string()))?
                    {
                        if status.success() || attempt == 3 {
                            return Ok(status);
                        }
                        std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
                        break;
                    }
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(LauncherError::storage(
                            "加载器安装超过 15 分钟，已安全终止。",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            unreachable!("installer retry loop always returns")
        })
        .await
        .map_err(|error| LauncherError::storage(format!("安装任务异常退出：{error}")))??;
    if !exit_status.success() {
        return Err(LauncherError::storage(format!(
            "官方加载器安装失败（退出码 {:?}），日志：{}",
            exit_status.code(),
            installer_log.display()
        )));
    }

    let profile = locate_java_loader_profile(&game, &game_version, &loader_type)?;
    let profile_libraries = profile
        .get("libraries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let base_path = game
        .join("versions")
        .join(&game_version)
        .join(format!("{game_version}.json"));
    let bytes = tokio::fs::read(&base_path)
        .await
        .map_err(|_| LauncherError::validation("基础版本元数据缺失。"))?;
    let mut effective: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    merge_loader_profile(&mut effective, &profile, profile_libraries)?;
    tokio::fs::write(
        game.join("versions")
            .join(&game_version)
            .join("launcher-effective.json"),
        serde_json::to_vec_pretty(&effective)
            .map_err(|error| LauncherError::storage(error.to_string()))?,
    )
    .await
    .map_err(|error| LauncherError::storage(error.to_string()))?;

    let connection = open_database(&app)?;
    connection
        .execute(
            "UPDATE instances SET loader_version=?1,status='ready' WHERE id=?2",
            params![loader_version, instance_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    connection.execute(
        "INSERT INTO installation_states(instance_id,component_kind,component_key,status) VALUES(?1,'loader',?2,'verified') ON CONFLICT(instance_id,component_kind,component_key) DO UPDATE SET status='verified'",
        params![instance_id, loader_version],
    ).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(Instance {
        id: instance_id,
        name,
        root_path,
        game_version,
        loader_type,
        memory_mb,
        status: "ready".into(),
        source,
    })
}

async fn sha1_file(path: &std::path::Path) -> Result<String, LauncherError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    instance_id: i64,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    job_id: Option<i64>,
    source_url: Option<String>,
    file_name: Option<String>,
    speed_bytes_per_second: Option<u64>,
    eta_seconds: Option<u64>,
}

fn create_download_job(
    app: &AppHandle,
    url: &str,
    target: &Path,
    resume_from: u64,
    total: Option<u64>,
    expected_sha1: &str,
) -> Result<i64, LauncherError> {
    let connection = open_database(app)?;
    let now = chrono_like_timestamp();
    connection
        .execute(
            "INSERT INTO download_jobs(source_url,target_path,progress_bytes,total_bytes,retry_count,expected_hash,status,error,recovery_action,created_at,started_at,updated_at,bytes_per_second,eta_seconds) VALUES(?1,?2,?3,?4,0,?5,'downloading',NULL,'重试下载',?6,?6,?6,0,NULL)",
            params![
                url,
                target.to_string_lossy(),
                resume_from as i64,
                total.map(|value| value as i64),
                expected_sha1,
                now
            ],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(connection.last_insert_rowid())
}

fn object_cache_path(expected_sha1: &str) -> Option<PathBuf> {
    if expected_sha1.len() < 4 {
        return None;
    }
    let directory = launcher_data_directory().ok()?;
    Some(
        directory
            .join("cache")
            .join("objects")
            .join("sha1")
            .join(&expected_sha1[..2])
            .join(&expected_sha1[2..]),
    )
}

async fn reuse_object_cache(
    target: &Path,
    expected_sha1: &str,
) -> Result<Option<u64>, LauncherError> {
    let Some(cached) = object_cache_path(expected_sha1) else {
        return Ok(None);
    };
    if !cached.is_file() {
        return Ok(None);
    }
    let observed = sha1_file(&cached).await?;
    if !observed.eq_ignore_ascii_case(expected_sha1) {
        return Ok(None);
    }
    let size = fs::metadata(&cached)
        .map(|metadata| metadata.len())
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    tokio::fs::copy(&cached, target)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(Some(size))
}

fn update_download_job_progress(
    app: &AppHandle,
    job_id: i64,
    downloaded: u64,
    total: Option<u64>,
    speed: u64,
    eta: Option<u64>,
) {
    let Ok(connection) = open_database(app) else {
        return;
    };
    let _ = connection.execute(
        "UPDATE download_jobs SET progress_bytes=?1,total_bytes=?2,updated_at=?3,bytes_per_second=?4,eta_seconds=?5 WHERE id=?6",
        params![
            downloaded as i64,
            total.map(|value| value as i64),
            chrono_like_timestamp(),
            speed as i64,
            eta.map(|value| value as i64),
            job_id
        ],
    );
}

fn finish_download_job(
    app: &AppHandle,
    job_id: i64,
    result: &Result<u64, LauncherError>,
    downloaded: u64,
    total: Option<u64>,
) {
    let Ok(connection) = open_database(app) else {
        return;
    };
    let now = chrono_like_timestamp();
    match result {
        Ok(size) => {
            let _ = connection.execute(
                "UPDATE download_jobs SET status='verified',progress_bytes=?1,total_bytes=?2,error=NULL,recovery_action=NULL,updated_at=?3,bytes_per_second=0,eta_seconds=NULL WHERE id=?4",
                params![
                    *size as i64,
                    total.map(|value| value as i64).or(Some(*size as i64)),
                    now,
                    job_id
                ],
            );
        }
        Err(error) => {
            let _ = connection.execute(
                "UPDATE download_jobs SET status='failed',progress_bytes=?1,total_bytes=?2,error=?3,recovery_action='重新下载',updated_at=?4,bytes_per_second=0,eta_seconds=NULL WHERE id=?5",
                params![
                    downloaded as i64,
                    total.map(|value| value as i64),
                    error.message,
                    now,
                    job_id
                ],
            );
        }
    }
}

fn download_cancel_flag() -> &'static AtomicBool {
    static CANCELLED: AtomicBool = AtomicBool::new(false);
    &CANCELLED
}

fn download_cancel_tokens() -> &'static DashMap<i64, CancellationToken> {
    static TOKENS: OnceLock<DashMap<i64, CancellationToken>> = OnceLock::new();
    TOKENS.get_or_init(DashMap::new)
}

fn job_speed_meters() -> &'static DashMap<i64, Mutex<download_perf::SpeedMeter>> {
    static METERS: OnceLock<DashMap<i64, Mutex<download_perf::SpeedMeter>>> = OnceLock::new();
    METERS.get_or_init(DashMap::new)
}

#[tauri::command]
fn cancel_active_downloads() {
    download_cancel_flag().store(true, Ordering::Release);
    download_cancel_tokens()
        .iter()
        .for_each(|entry| entry.cancel());
}

#[tauri::command]
fn cancel_download_job(job_id: i64) {
    download_cancel_tokens().entry(job_id).or_default().cancel();
}

fn download_job_cancelled(job_id: i64) -> bool {
    download_cancel_flag().load(Ordering::Acquire)
        || download_cancel_tokens()
            .get(&job_id)
            .is_some_and(|token| token.is_cancelled())
}

async fn send_download_request(
    client: &reqwest::Client,
    url: &reqwest::Url,
    resume_from: Option<u64>,
) -> Result<reqwest::Response, LauncherError> {
    let mut last_error = String::new();
    for attempt in 0..=3u32 {
        let mut request = client.get(url.clone());
        if url.host_str() == Some("www.curseforge.com") {
            request = request.header(reqwest::header::REFERER, "https://www.curseforge.com/");
        }
        if let Some(offset) = resume_from.filter(|offset| *offset > 0) {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        match request.send().await {
            Ok(response)
                if !(response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || response.status().is_server_error()
                    || response.status() == reqwest::StatusCode::NOT_FOUND) =>
            {
                download_perf::record_host_request(
                    url.host_str().unwrap_or("unknown"),
                    response.status().is_success() || response.status().is_redirection(),
                    0,
                );
                return Ok(response);
            }
            Ok(response) => {
                download_perf::record_host_request(url.host_str().unwrap_or("unknown"), false, 0);
                last_error = format!("HTTP {}", response.status());
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    break;
                }
                if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    if let Some(retry_after) = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok())
                    {
                        tokio::time::sleep(Duration::from_secs(retry_after.min(30))).await;
                        continue;
                    }
                }
            }
            Err(error) => {
                download_perf::record_host_request(url.host_str().unwrap_or("unknown"), false, 0);
                last_error = error.to_string();
            }
        }
        if attempt < 3 {
            tokio::time::sleep(download_perf::retry_delay(attempt)).await;
        }
    }
    Err(LauncherError::storage(format!(
        "下载在自动重试后仍失败：{last_error}"
    )))
}

async fn download_verified_file(
    app: &AppHandle,
    instance_id: i64,
    url: &str,
    expected_sha1: &str,
    expected_size: Option<u64>,
    target: &std::path::Path,
) -> Result<u64, LauncherError> {
    download_verified_file_with_progress(
        app,
        instance_id,
        url,
        expected_sha1,
        expected_size,
        target,
        true,
    )
    .await
}

async fn download_verified_file_parallel(
    url: &str,
    expected_sha1: &str,
    expected_size: u64,
    target: &std::path::Path,
) -> Result<u64, LauncherError> {
    if expected_size < 16 * 1024 * 1024 {
        return Err(LauncherError::validation("文件过小，不需要分段下载。"));
    }
    let url = validate_resource_url(url)?;
    let client = shared_download_client()?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let segments = 4u64;
    let chunk = expected_size.div_ceil(segments);
    let mut tasks = Vec::new();
    for index in 0..segments {
        let start = index * chunk;
        if start >= expected_size {
            break;
        }
        let end = (start + chunk - 1).min(expected_size - 1);
        let client = client.clone();
        let url = url.clone();
        let part_path = target.with_extension(format!("part{index}"));
        tasks.push(async move {
            let mut last_error = String::new();
            for attempt in 0..=2u32 {
                let mut request = client
                    .get(url.clone())
                    .header(reqwest::header::RANGE, format!("bytes={start}-{end}"));
                if url.host_str() == Some("www.curseforge.com") {
                    request =
                        request.header(reqwest::header::REFERER, "https://www.curseforge.com/");
                }
                match request.send().await {
                    Ok(response)
                        if response.status() == reqwest::StatusCode::PARTIAL_CONTENT
                            || response.status().is_success() =>
                    {
                        let mut file = tokio::fs::File::create(&part_path)
                            .await
                            .map_err(|error| LauncherError::storage(error.to_string()))?;
                        let mut stream = response.bytes_stream();
                        while let Some(chunk) = stream.next().await {
                            let chunk =
                                chunk.map_err(|error| LauncherError::storage(error.to_string()))?;
                            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                                .await
                                .map_err(|error| LauncherError::storage(error.to_string()))?;
                        }
                        tokio::io::AsyncWriteExt::flush(&mut file)
                            .await
                            .map_err(|error| LauncherError::storage(error.to_string()))?;
                        return Ok(());
                    }
                    Ok(response) => last_error = format!("HTTP {}", response.status()),
                    Err(error) => last_error = error.to_string(),
                }
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(300 * 2u64.pow(attempt))).await;
                }
            }
            Err(LauncherError::storage(format!(
                "分段下载失败：{last_error}"
            )))
        });
    }
    let results: Vec<Result<(), LauncherError>> = futures_util::stream::iter(tasks)
        .buffer_unordered(segments as usize)
        .collect()
        .await;
    if results.iter().any(Result::is_err) {
        for index in 0..segments {
            let _ = tokio::fs::remove_file(target.with_extension(format!("part{index}"))).await;
        }
        return Err(results
            .into_iter()
            .find_map(Result::err)
            .unwrap_or_else(|| LauncherError::storage("分段下载失败。")));
    }
    let mut output = tokio::fs::File::create(target)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    for index in 0..segments {
        let part_path = target.with_extension(format!("part{index}"));
        if tokio::fs::try_exists(&part_path).await.unwrap_or(false) {
            let mut part = tokio::fs::File::open(&part_path)
                .await
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            tokio::io::copy(&mut part, &mut output)
                .await
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut output)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    drop(output);
    let actual_size = tokio::fs::metadata(target)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?
        .len();
    if actual_size != expected_size || !sha1_file(target).await?.eq_ignore_ascii_case(expected_sha1)
    {
        let _ = tokio::fs::remove_file(target).await;
        return Err(LauncherError::validation("下载文件大小或 SHA-1 校验失败。"));
    }
    for index in 0..segments {
        let _ = tokio::fs::remove_file(target.with_extension(format!("part{index}"))).await;
    }
    Ok(actual_size)
}

async fn download_verified_file_with_progress(
    app: &AppHandle,
    instance_id: i64,
    url: &str,
    expected_sha1: &str,
    expected_size: Option<u64>,
    target: &std::path::Path,
    emit_file_progress: bool,
) -> Result<u64, LauncherError> {
    if !expected_sha1.is_empty() && expected_size.is_some_and(|size| size >= 16 * 1024 * 1024) {
        if let Ok(size) =
            download_verified_file_parallel(url, expected_sha1, expected_size.unwrap_or(0), target)
                .await
        {
            return Ok(size);
        }
    }
    let first = download_verified_file_attempt(
        app,
        instance_id,
        url,
        expected_sha1,
        expected_size,
        target,
        emit_file_progress,
        true,
    )
    .await;
    if first.is_err() {
        // 1) 先用同一地址重新开一个连接重试一次，缓解断流、超时和残留部分文件。
        if let Ok(size) = download_verified_file_attempt(
            app,
            instance_id,
            url,
            expected_sha1,
            expected_size,
            target,
            emit_file_progress,
            false,
        )
        .await
        {
            return Ok(size);
        }
        // 2) 仍失败时，自动改用 BMCLAPI 国内镜像重试一次（SHA-1 校验不变）。
        if let Ok(parsed) = validate_resource_url(url) {
            if let Some(mirror) = bmclapi_mirror_url(&parsed) {
                if let Ok(size) = download_verified_file_attempt(
                    app,
                    instance_id,
                    mirror.as_str(),
                    expected_sha1,
                    expected_size,
                    target,
                    emit_file_progress,
                    false,
                )
                .await
                {
                    return Ok(size);
                }
            }
        }
        return first;
    }
    first
}

async fn fetch_manifest_bytes(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<u8>, LauncherError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| LauncherError::storage(format!("获取官方版本清单失败：{error}")))?;
    let response = response
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("官方版本服务返回错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MANIFEST_BYTES as u64)
    {
        return Err(LauncherError::validation("版本清单超过安全大小限制。"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(LauncherError::validation("版本清单超过安全大小限制。"));
    }
    Ok(bytes.to_vec())
}

async fn fetch_version_details_from(
    client: &reqwest::Client,
    url: &reqwest::Url,
    expected_sha1: &str,
) -> Result<serde_json::Value, LauncherError> {
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| LauncherError::storage(format!("获取版本元数据失败：{error}")))?;
    let response = response
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("版本元数据服务返回错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_VERSION_JSON_BYTES as u64)
    {
        return Err(LauncherError::validation("版本元数据超过安全大小限制。"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if bytes.len() > MAX_VERSION_JSON_BYTES || !verify_sha1(&bytes, expected_sha1) {
        return Err(LauncherError::validation("版本元数据 SHA-1 校验失败。"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(format!("版本元数据格式无效：{error}")))
}

#[allow(clippy::too_many_arguments)]
async fn download_verified_file_attempt(
    app: &AppHandle,
    instance_id: i64,
    url: &str,
    expected_sha1: &str,
    expected_size: Option<u64>,
    target: &std::path::Path,
    emit_file_progress: bool,
    allow_resume: bool,
) -> Result<u64, LauncherError> {
    if !expected_sha1.is_empty()
        && (expected_sha1.len() != 40
            || !expected_sha1.chars().all(|value| value.is_ascii_hexdigit()))
    {
        return Err(LauncherError::validation("下载文件 SHA-1 无效。"));
    }
    let url = validate_resource_url(url)?;
    if !expected_sha1.is_empty() {
        if let Some(size) = reuse_object_cache(target, expected_sha1).await? {
            download_perf::record_network_bytes(0);
            return Ok(size);
        }
    }
    if expected_sha1.is_empty() {
        if let Some(size) = expected_size {
            if tokio::fs::try_exists(target)
                .await
                .map_err(|error| LauncherError::storage(error.to_string()))?
            {
                let existing = tokio::fs::metadata(target)
                    .await
                    .map_err(|error| LauncherError::storage(error.to_string()))?
                    .len();
                if existing == size {
                    return Ok(existing);
                }
            }
        }
    } else if tokio::fs::try_exists(target)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?
        && sha1_file(target).await?.eq_ignore_ascii_case(expected_sha1)
    {
        return Ok(tokio::fs::metadata(target)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .len());
    }
    // Content-addressed cache lets different instances and repeated installs reuse
    // an already verified file instead of downloading it again.
    let cache_target = if expected_sha1.is_empty() {
        None
    } else {
        launcher_data_directory().ok().map(|root| {
            root.join("cache")
                .join("sha1")
                .join(expected_sha1.to_ascii_lowercase())
        })
    };
    if let Some(cache_target) = cache_target.as_ref() {
        if tokio::fs::try_exists(cache_target).await.unwrap_or(false)
            && sha1_file(cache_target)
                .await?
                .eq_ignore_ascii_case(expected_sha1)
        {
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
            }
            tokio::fs::copy(cache_target, target)
                .await
                .map_err(|error| LauncherError::storage(format!("复用下载缓存失败：{error}")))?;
            return Ok(tokio::fs::metadata(target)
                .await
                .map_err(|error| LauncherError::storage(error.to_string()))?
                .len());
        }
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let part = target.with_extension("part");
    let client = shared_download_client()?;
    if !allow_resume {
        let _ = tokio::fs::remove_file(&part).await;
    }
    let mut resume_from = if allow_resume {
        tokio::fs::metadata(&part)
            .await
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    } else {
        0
    };
    if expected_size.is_some_and(|size| resume_from >= size) {
        let _ = tokio::fs::remove_file(&part).await;
        resume_from = 0;
    }
    let mut response = send_download_request(&client, &url, Some(resume_from)).await?;
    if resume_from > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        let _ = tokio::fs::remove_file(&part).await;
        resume_from = 0;
        response = send_download_request(&client, &url, None).await?;
    }
    let response = response
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("下载服务返回错误：{error}")))?;
    let _large_permit = if expected_size.is_some_and(|size| size >= 16 * 1024 * 1024) {
        Some(
            download_perf::download_concurrency()
                .large
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| LauncherError::storage("大文件并发控制异常。"))?,
        )
    } else {
        None
    };
    const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
    let total = expected_size.or_else(|| {
        response
            .content_length()
            .map(|remaining| remaining.saturating_add(resume_from))
    });
    if total.is_some_and(|size| size > MAX_DOWNLOAD_BYTES) {
        return Err(LauncherError::validation("下载文件超过安全大小限制。"));
    }
    let job_id = create_download_job(app, url.as_str(), target, resume_from, total, expected_sha1)?;
    let completed_bytes = Arc::new(AtomicU64::new(resume_from));
    let progress_bytes = completed_bytes.clone();
    let source_url = url.to_string();
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string);
    let download_result: Result<u64, LauncherError> = async move {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resume_from > 0)
            .truncate(resume_from == 0)
            .open(&part)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let mut stream = response.bytes_stream();
        let mut hasher = Sha1::new();
        if resume_from > 0 {
            let mut existing = tokio::fs::File::open(&part)
                .await
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            let mut existing_buffer = vec![0u8; 64 * 1024];
            loop {
                let count = existing
                    .read(&mut existing_buffer)
                    .await
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
                if count == 0 {
                    break;
                }
                hasher.update(&existing_buffer[..count]);
            }
        }
        let mut downloaded = resume_from;
        let mut last_recorded = resume_from;
        let mut last_emit = std::time::Instant::now();
        loop {
            let next = tokio::time::timeout(Duration::from_secs(180), stream.next())
                .await
                .map_err(|_| {
                    LauncherError::storage(
                        "下载连续 180 秒没有收到数据。请检查网络后重试；已下载的部分会保留。",
                    )
                })?;
            let Some(chunk) = next else { break };
            if download_job_cancelled(job_id) {
                file.flush()
                    .await
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
                return Err(LauncherError::storage(
                    "下载已取消；临时文件已保留，下次可断点继续。",
                ));
            }
            let chunk =
                chunk.map_err(|error| LauncherError::storage(format!("下载中断：{error}")))?;
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            progress_bytes.store(downloaded, Ordering::Relaxed);
            if downloaded > MAX_DOWNLOAD_BYTES {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(LauncherError::validation("下载文件超过安全大小限制。"));
            }
            hasher.update(&chunk);
            download_perf::record_network_bytes(chunk.len() as u64);
            file.write_all(&chunk)
                .await
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            if last_emit.elapsed() >= Duration::from_millis(250) {
                let delta = downloaded.saturating_sub(last_recorded);
                if let Some(mut meter) = job_speed_meters().get_mut(&job_id) {
                    if let Ok(inner) = meter.value_mut().get_mut() {
                        inner.record(delta);
                    }
                }
                let speed = job_speed_meters()
                    .get_mut(&job_id)
                    .and_then(|mut meter| {
                        meter
                            .value_mut()
                            .get_mut()
                            .ok()
                            .map(|inner| inner.bytes_per_second().round() as u64)
                    })
                    .unwrap_or(0);
                last_recorded = downloaded;
                let eta = total.and_then(|total| {
                    let remaining = total.saturating_sub(downloaded);
                    if speed > 0 {
                        Some((remaining as f64 / speed as f64).ceil() as u64)
                    } else {
                        None
                    }
                });
                update_download_job_progress(app, job_id, downloaded, total, speed, eta);
                if emit_file_progress && should_emit_download_progress() {
                    let _ = app.emit(
                        "download-progress",
                        DownloadProgress {
                            instance_id,
                            downloaded_bytes: downloaded,
                            total_bytes: total,
                            job_id: Some(job_id),
                            source_url: Some(source_url.clone()),
                            file_name: file_name.clone(),
                            speed_bytes_per_second: Some(speed),
                            eta_seconds: eta,
                        },
                    );
                }
                last_emit = std::time::Instant::now();
            }
        }
        file.flush()
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let hash_matches = expected_sha1.is_empty()
            || format!("{:x}", hasher.finalize()) == expected_sha1.to_ascii_lowercase();
        if expected_size.is_some_and(|size| size != downloaded) || !hash_matches {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(LauncherError::validation("下载文件大小或 SHA-1 校验失败。"));
        }
        if tokio::fs::try_exists(target).await.unwrap_or(false) {
            let backup = target.with_extension(format!("corrupt-{}", unique_timestamp()));
            tokio::fs::rename(target, backup)
                .await
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        tokio::fs::rename(&part, target)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if !expected_sha1.is_empty() {
            if let Some(object) = object_cache_path(expected_sha1) {
                if let Some(parent) = object.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                if !tokio::fs::try_exists(&object).await.unwrap_or(false) {
                    let _ = tokio::fs::copy(target, &object).await;
                }
            }
        }
        if let Some(cache_target) = cache_target {
            if let Some(parent) = cache_target.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            if !tokio::fs::try_exists(&cache_target).await.unwrap_or(false) {
                let _ = tokio::fs::copy(target, cache_target).await;
            }
        }
        Ok(downloaded)
    }
    .await;
    finish_download_job(
        app,
        job_id,
        &download_result,
        completed_bytes.load(Ordering::Relaxed),
        total,
    );
    download_result
}

fn emit_install_percent(app: &AppHandle, instance_id: i64, percent: u64) {
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            instance_id,
            downloaded_bytes: percent.min(100),
            total_bytes: Some(100),
            ..Default::default()
        },
    );
}

fn shared_download_client() -> Result<reqwest::Client, LauncherError> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36")
        .http1_only()
        .pool_max_idle_per_host(32)
        .build()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let _ = CLIENT.set(client.clone());
    Ok(CLIENT.get().cloned().unwrap_or(client))
}

fn quick_http_client() -> Result<reqwest::Client, LauncherError> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36")
        .http1_only()
        .build()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let _ = CLIENT.set(client.clone());
    Ok(CLIENT.get().cloned().unwrap_or(client))
}

fn should_emit_download_progress() -> bool {
    static LAST_EMIT: OnceLock<Mutex<std::time::Instant>> = OnceLock::new();
    let mutex =
        LAST_EMIT.get_or_init(|| Mutex::new(std::time::Instant::now() - Duration::from_secs(1)));
    let Ok(mut last) = mutex.lock() else {
        return false;
    };
    if last.elapsed() < Duration::from_millis(100) {
        return false;
    }
    *last = std::time::Instant::now();
    true
}

fn rule_matches_windows(rule: &serde_json::Value) -> bool {
    if rule
        .get("features")
        .and_then(|value| value.as_object())
        .is_some_and(|features| {
            features
                .values()
                .any(|expected| expected.as_bool() == Some(true))
        })
    {
        return false;
    }
    let Some(os) = rule.get("os") else {
        return true;
    };
    if os
        .get("name")
        .and_then(|value| value.as_str())
        .is_some_and(|name| name != "windows")
    {
        return false;
    }
    if os
        .get("arch")
        .and_then(|value| value.as_str())
        .is_some_and(|arch| !matches!(arch, "x86_64" | "amd64" | "x64"))
    {
        return false;
    }
    true
}

fn rules_allow_windows(value: &serde_json::Value) -> bool {
    let Some(rules) = value.get("rules").and_then(|rules| rules.as_array()) else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        if rule_matches_windows(rule) {
            allowed = rule.get("action").and_then(|action| action.as_str()) == Some("allow");
        }
    }
    allowed
}

fn download_fields<'a>(
    value: &'a serde_json::Value,
) -> Result<(&'a str, &'a str, u64), LauncherError> {
    let url = value
        .get("url")
        .and_then(|entry| entry.as_str())
        .ok_or_else(|| LauncherError::storage("下载描述缺少 URL。"))?;
    let sha1 = value
        .get("sha1")
        .and_then(|entry| entry.as_str())
        .ok_or_else(|| LauncherError::storage("下载描述缺少 SHA-1。"))?;
    let size = value
        .get("size")
        .and_then(|entry| entry.as_u64())
        .ok_or_else(|| LauncherError::storage("下载描述缺少文件大小。"))?;
    Ok((url, sha1, size))
}

fn safe_relative_download_path(value: &str) -> Result<PathBuf, LauncherError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(LauncherError::validation("下载目标路径无效。"));
    }
    Ok(path.to_path_buf())
}

fn extract_native_jar(archive_path: &Path, target: &Path) -> Result<(), LauncherError> {
    fs::create_dir_all(target).map_err(|error| LauncherError::storage(error.to_string()))?;
    let limits = fs_safe::ArchiveLimits {
        max_entries: 20_000,
        max_total_uncompressed: 2 * 1024 * 1024 * 1024,
        max_single_file: 512 * 1024 * 1024,
        ..fs_safe::ArchiveLimits::default()
    };
    let staging = target
        .join(".staging")
        .join(format!("native-{}", unique_timestamp()));
    fs_safe::extract_zip_securely(archive_path, &staging, &limits)?;
    for entry in walkdir::WalkDir::new(&staging)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(&staging) else {
            continue;
        };
        if relative
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("META-INF"))
        {
            continue;
        }
        let output = target.join(relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        fs::copy(entry.path(), &output)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

async fn install_vanilla_components(
    app: &AppHandle,
    instance_id: i64,
    root_path: &str,
    version: &str,
    details: &serde_json::Value,
    concurrency: usize,
) -> Result<u64, LauncherError> {
    let game = PathBuf::from(root_path).join(".minecraft");
    let mut total_downloaded = 0u64;
    emit_install_percent(app, instance_id, 2);
    let client = details
        .pointer("/downloads/client")
        .ok_or_else(|| LauncherError::storage("版本元数据缺少 client。"))?;
    let (client_url, client_sha1, client_size) = download_fields(client)?;
    total_downloaded += download_verified_file_with_progress(
        app,
        instance_id,
        client_url,
        client_sha1,
        Some(client_size),
        &game
            .join("versions")
            .join(version)
            .join(format!("{version}.jar")),
        false,
    )
    .await?;
    emit_install_percent(app, instance_id, 20);

    let libraries = details
        .get("libraries")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LauncherError::storage("版本元数据缺少 libraries。"))?;
    let natives_directory = game.join("versions").join(version).join("natives");
    type LibraryItem = (
        Option<(String, String, String, u64)>,
        Option<(String, String, String, u64)>,
    );
    let mut library_tasks = Vec::new();
    for library in libraries
        .iter()
        .filter(|library| rules_allow_windows(library))
    {
        let artifact = library
            .pointer("/downloads/artifact")
            .map(|artifact| {
                let path = artifact
                    .get("path")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| LauncherError::storage("Library 缺少路径。"))?
                    .to_string();
                let (url, sha1, size) = download_fields(artifact)?;
                Ok::<_, LauncherError>((path, url.to_string(), sha1.to_string(), size))
            })
            .transpose()?;
        let native = library
            .pointer("/natives/windows")
            .and_then(|value| value.as_str())
            .map(|native_template| {
                let classifier = native_template.replace("${arch}", "64");
                let native = library
                    .pointer(&format!("/downloads/classifiers/{classifier}"))
                    .ok_or_else(|| LauncherError::storage("Native library 缺少分类文件。"))?;
                let path = native
                    .get("path")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| LauncherError::storage("Native library 缺少路径。"))?
                    .to_string();
                let (url, sha1, size) = download_fields(native)?;
                Ok::<_, LauncherError>((path, url.to_string(), sha1.to_string(), size))
            })
            .transpose()?;
        library_tasks.push((artifact, native) as LibraryItem);
    }
    let library_count = library_tasks.len().max(1) as u64;
    let app_clone = app.clone();
    let game_clone = game.clone();
    let natives_clone = natives_directory.clone();
    let results =
        futures_util::stream::iter(library_tasks.into_iter().map(|(artifact, native)| {
            let app = app_clone.clone();
            let game = game_clone.clone();
            let natives = natives_clone.clone();
            async move {
                let _permit = download_perf::download_concurrency()
                    .library
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| LauncherError::storage("下载并发控制异常。"))?;
                let mut downloaded = 0u64;
                if let Some((path, url, sha1, size)) = artifact {
                    downloaded = downloaded.saturating_add(
                        download_verified_file_with_progress(
                            &app,
                            instance_id,
                            &url,
                            &sha1,
                            Some(size),
                            &game
                                .join("libraries")
                                .join(safe_relative_download_path(&path)?),
                            false,
                        )
                        .await?,
                    );
                }
                if let Some((path, url, sha1, size)) = native {
                    let target = game
                        .join("libraries")
                        .join(safe_relative_download_path(&path)?);
                    downloaded = downloaded.saturating_add(
                        download_verified_file_with_progress(
                            &app,
                            instance_id,
                            &url,
                            &sha1,
                            Some(size),
                            &target,
                            false,
                        )
                        .await?,
                    );
                    extract_native_jar(&target, &natives)?;
                }
                Ok::<u64, LauncherError>(downloaded)
            }
        }))
        .buffer_unordered(concurrency.clamp(1, 64))
        .collect::<Vec<_>>()
        .await;
    let mut processed_libraries = 0u64;
    for result in results {
        total_downloaded = total_downloaded.saturating_add(result?);
        processed_libraries += 1;
        emit_install_percent(
            app,
            instance_id,
            20 + (processed_libraries * 25 / library_count),
        );
    }

    // Some libraries do not include a Windows native. Count those after their artifact is ready.
    emit_install_percent(app, instance_id, 45);

    let asset_index = details
        .get("assetIndex")
        .ok_or_else(|| LauncherError::storage("版本元数据缺少 assetIndex。"))?;
    let asset_id = asset_index
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("资源索引缺少 id。"))?;
    validate_instance_field(asset_id, 128)?;
    let (index_url, index_sha1, index_size) = download_fields(asset_index)?;
    let index_target = game
        .join("assets")
        .join("indexes")
        .join(format!("{asset_id}.json"));
    total_downloaded += download_verified_file_with_progress(
        app,
        instance_id,
        index_url,
        index_sha1,
        Some(index_size),
        &index_target,
        false,
    )
    .await?;
    emit_install_percent(app, instance_id, 47);
    let index_bytes = tokio::fs::read(&index_target)
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if index_bytes.len() > 32 * 1024 * 1024 {
        return Err(LauncherError::validation("资源索引超过安全大小限制。"));
    }
    let index: serde_json::Value = serde_json::from_slice(&index_bytes)
        .map_err(|error| LauncherError::storage(format!("资源索引无效：{error}")))?;
    let objects = index
        .get("objects")
        .and_then(|value| value.as_object())
        .ok_or_else(|| LauncherError::storage("资源索引缺少 objects。"))?;
    if objects.len() > 100_000 {
        return Err(LauncherError::validation("资源索引条目过多。"));
    }
    let mut asset_tasks = Vec::with_capacity(objects.len());
    for object in objects.values() {
        let hash = object
            .get("hash")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LauncherError::storage("资源对象缺少 hash。"))?;
        if hash.len() != 40 || !hash.chars().all(|value| value.is_ascii_hexdigit()) {
            return Err(LauncherError::validation("资源对象 SHA-1 无效。"));
        }
        let size = object
            .get("size")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| LauncherError::storage("资源对象缺少大小。"))?;
        let prefix = &hash[..2];
        let url = format!("https://resources.download.minecraft.net/{prefix}/{hash}");
        asset_tasks.push((
            url,
            hash.to_string(),
            size,
            game.join("assets").join("objects").join(prefix).join(hash),
        ));
    }
    let asset_total = asset_tasks
        .iter()
        .map(|(_, _, size, _)| *size)
        .sum::<u64>()
        .max(1);
    let asset_done = Arc::new(AtomicU64::new(0));
    let results =
        futures_util::stream::iter(asset_tasks.into_iter().map(|(url, hash, size, target)| {
            let asset_done = Arc::clone(&asset_done);
            async move {
                let _permit = download_perf::download_concurrency()
                    .small
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| LauncherError::storage("下载并发控制异常。"))?;
                let result = download_verified_file_with_progress(
                    app,
                    instance_id,
                    &url,
                    &hash,
                    Some(size),
                    &target,
                    false,
                )
                .await;
                if result.is_ok() {
                    let done = asset_done
                        .fetch_add(size, Ordering::Relaxed)
                        .saturating_add(size);
                    emit_install_percent(app, instance_id, 47 + (done * 52 / asset_total));
                }
                result
            }
        }))
        .buffer_unordered(concurrency.clamp(1, 64))
        .collect::<Vec<_>>()
        .await;
    for result in results {
        total_downloaded = total_downloaded.saturating_add(result?);
    }
    emit_install_percent(app, instance_id, 100);
    Ok(total_downloaded)
}

fn copy_directory_contents(source: &Path, target: &Path) -> Result<(), LauncherError> {
    if !source.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(target).map_err(|error| LauncherError::storage(error.to_string()))?;
    for entry in fs::read_dir(source).map_err(|error| LauncherError::storage(error.to_string()))? {
        let entry = entry.map_err(|error| LauncherError::storage(error.to_string()))?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if from.is_dir() {
            copy_directory_contents(&from, &to)?;
        } else if !to.exists() {
            fs::copy(&from, &to).map_err(|error| LauncherError::storage(error.to_string()))?;
        }
    }
    Ok(())
}

fn backup_instance_worlds(instance_id: i64, game: &Path) -> Result<Option<PathBuf>, LauncherError> {
    let saves = game.join("saves");
    if !saves.is_dir()
        || fs::read_dir(&saves)
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .next()
            .is_none()
    {
        return Ok(None);
    }
    let backup_root = launcher_data_directory()?
        .join("backups")
        .join("instances")
        .join(instance_id.to_string());
    fs::create_dir_all(&backup_root).map_err(|error| LauncherError::storage(error.to_string()))?;
    let destination = backup_root.join(unique_timestamp().to_string());
    copy_directory_contents(&saves, &destination)?;
    let mut backups = fs::read_dir(&backup_root)
        .map_err(|error| LauncherError::storage(error.to_string()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    backups.sort_by_key(|entry| std::cmp::Reverse(entry.file_name()));
    for old in backups.into_iter().skip(5) {
        let path = old.path();
        if path.starts_with(&backup_root) {
            let _ = fs::remove_dir_all(path);
        }
    }
    Ok(Some(destination))
}

fn minecraft_source_candidates(app: &AppHandle) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(settings) = get_settings(app.clone()) {
        if let Some(configured) = settings
            .game_directory
            .filter(|path| !path.trim().is_empty())
        {
            let configured = PathBuf::from(configured);
            candidates.push(configured.clone());
            candidates.push(configured.join(".minecraft"));
        }
    }
    if let Some(app_data) = std::env::var_os("APPDATA") {
        candidates.push(PathBuf::from(app_data).join(".minecraft"));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        candidates.push(PathBuf::from(profile).join(".minecraft"));
    }
    let mut unique = HashSet::new();
    candidates.retain(|path| unique.insert(path.to_string_lossy().to_ascii_lowercase()));
    candidates
}

fn import_existing_minecraft_files(
    app: &AppHandle,
    root_path: &str,
    version: &str,
) -> Result<bool, LauncherError> {
    let target_game = PathBuf::from(root_path).join(".minecraft");
    for source_game in minecraft_source_candidates(app) {
        if !source_game.is_dir() || source_game == target_game {
            continue;
        }
        let source_version = source_game
            .join("versions")
            .join(version)
            .join(format!("{version}.jar"));
        let source_json = source_game
            .join("versions")
            .join(version)
            .join(format!("{version}.json"));
        if !source_version.is_file() || !source_json.is_file() {
            continue;
        }
        copy_directory_contents(
            &source_game.join("versions").join(version),
            &target_game.join("versions").join(version),
        )?;
        copy_directory_contents(
            &source_game.join("libraries"),
            &target_game.join("libraries"),
        )?;
        copy_directory_contents(&source_game.join("assets"), &target_game.join("assets"))?;
        return Ok(true);
    }
    Ok(false)
}

#[tauri::command]
async fn install_vanilla_client(
    app: AppHandle,
    instance_id: i64,
    version_url: String,
    version_sha1: String,
) -> Result<VanillaInstallPreview, LauncherError> {
    download_cancel_flag().store(false, Ordering::Release);
    let connection = open_database(&app)?;
    let (root_path, expected_version): (String, String) = connection
        .query_row(
            "SELECT root_path, game_version FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| LauncherError::storage(format!("实例不存在：{error}")))?;
    let created_at = chrono_like_timestamp();
    connection.execute("INSERT INTO download_jobs(source_url,target_path,status,created_at,recovery_action) VALUES(?1,?2,'downloading',?3,'重试下载')", params![version_url, root_path, created_at]).map_err(|error| LauncherError::storage(error.to_string()))?;
    let job_id = connection.last_insert_rowid();
    drop(connection);
    emit_install_percent(&app, instance_id, 1);
    let result: Result<(VanillaInstallPreview, u64, String), LauncherError> = async {
        let details = fetch_version_details(version_url, version_sha1).await?;
        let preview = install_preview_from_details(instance_id, &details)?;
        if preview.game_version != expected_version {
            return Err(LauncherError::validation(
                "官方返回的版本与当前游戏配置不一致。",
            ));
        }
        let client_sha1 = details
            .pointer("/downloads/client/sha1")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LauncherError::storage("官方版本信息缺少游戏文件校验值。"))?
            .to_string();
        let version_dir = PathBuf::from(&root_path)
            .join(".minecraft")
            .join("versions")
            .join(&expected_version);
        tokio::fs::create_dir_all(&version_dir)
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let version_json = serde_json::to_vec_pretty(&details)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        tokio::fs::write(
            version_dir.join(format!("{expected_version}.json")),
            version_json,
        )
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
        // PCL/官方启动器已有本体时直接导入到实例目录，后续 SHA-1 校验会逐项复用。
        let import_app = app.clone();
        let import_root = root_path.clone();
        let import_version = expected_version.clone();
        tokio::task::spawn_blocking(move || {
            import_existing_minecraft_files(&import_app, &import_root, &import_version)
        })
        .await
        .map_err(|error| LauncherError::storage(format!("读取已有游戏文件失败：{error}")))??;
        let concurrency = get_settings(app.clone())?.download_concurrency as usize;
        let size = install_vanilla_components(
            &app,
            instance_id,
            &root_path,
            &expected_version,
            &details,
            concurrency,
        )
        .await?;
        Ok((preview, size, client_sha1))
    }
    .await;
    let connection = open_database(&app)?;
    match result {
        Ok((preview, size, client_sha1)) => {
            connection.execute("UPDATE download_jobs SET status='verified',progress_bytes=?1,total_bytes=?1,expected_hash=?2 WHERE id=?3", params![size, client_sha1, job_id]).map_err(|error| LauncherError::storage(error.to_string()))?;
            connection.execute("INSERT INTO installation_states(instance_id,component_kind,component_key,hash,size_bytes,status) VALUES(?1,'game',?2,?3,?4,'verified') ON CONFLICT(instance_id,component_kind,component_key) DO UPDATE SET hash=excluded.hash,size_bytes=excluded.size_bytes,status='verified'", params![instance_id, expected_version, client_sha1, size]).map_err(|error| LauncherError::storage(error.to_string()))?;
            let loader: String = connection
                .query_row(
                    "SELECT loader_type FROM instances WHERE id=?1",
                    [instance_id],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| "vanilla".into());
            let loader_ready: i64 = connection.query_row("SELECT COUNT(*) FROM installation_states WHERE instance_id=?1 AND component_kind='loader' AND status='verified'", [instance_id], |row| row.get(0)).unwrap_or(0);
            let ready_status = if loader == "vanilla" || loader_ready > 0 {
                "ready"
            } else {
                "loader_missing"
            };
            connection
                .execute(
                    "UPDATE instances SET status=?1 WHERE id=?2",
                    params![ready_status, instance_id],
                )
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            Ok(preview)
        }
        Err(error) => {
            let _ = connection.execute(
                "UPDATE download_jobs SET status='failed',error=?1,recovery_action='重新下载' WHERE id=?2",
                params![error.message, job_id],
            );
            let _ = connection.execute(
                "UPDATE instances SET status='missing' WHERE id=?1 AND status!='ready'",
                [instance_id],
            );
            Err(error)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaunchResult {
    process_id: u32,
    log_path: String,
}

fn analyze_crash_log(path: &Path) -> (String, String, String) {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lower = text.to_ascii_lowercase();
    if lower.contains("outofmemoryerror") {
        return (
            "可能内存不足".into(),
            "high".into(),
            "降低高占用模组数量或在实例设置中适度增加内存。".into(),
        );
    }
    if lower.contains("unsupportedclassversionerror") {
        return (
            "Java 主版本不兼容".into(),
            "high".into(),
            "为该 Minecraft 版本选择要求的 64 位 Java。".into(),
        );
    }
    if lower.contains("mod resolution encountered")
        || lower.contains("requires version")
        || lower.contains("missing mandatory dependencies")
    {
        return (
            "可能缺少模组依赖或版本不匹配".into(),
            "medium".into(),
            "检查模组页的依赖警告、Minecraft 版本和加载器。".into(),
        );
    }
    if lower.contains("mixin") {
        return (
            "可能是 Mixin 或模组冲突".into(),
            "low".into(),
            "查看日志中最先失败的模组，并逐个停用近期加入的模组。".into(),
        );
    }
    (
        "原因未确定".into(),
        "low".into(),
        "导出诊断报告并查看完整游戏日志。".into(),
    )
}

fn argument_values(value: &serde_json::Value) -> Vec<String> {
    if let Some(text) = value.as_str() {
        return vec![text.to_string()];
    }
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    if !rules_allow_windows(value) {
        return Vec::new();
    }
    match object.get("value") {
        Some(value) if value.is_string() => vec![value.as_str().unwrap_or_default().to_string()],
        Some(value) if value.is_array() => value
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn substitute_argument(
    mut value: String,
    replacements: &[(&str, &str)],
) -> Result<String, LauncherError> {
    for (key, replacement) in replacements {
        value = value.replace(key, replacement);
    }
    if value.contains("${") {
        return Err(LauncherError::validation(format!(
            "无法解析启动参数：{value}"
        )));
    }
    Ok(value)
}

fn build_vanilla_launch_arguments(
    details: &serde_json::Value,
    game: &Path,
    version: &str,
    player_name: &str,
    player_uuid: &str,
    access_token: &str,
    user_type: &str,
    xuid: &str,
    memory_mb: i64,
) -> Result<Vec<String>, LauncherError> {
    let libraries = details
        .get("libraries")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LauncherError::storage("版本元数据缺少 libraries。"))?;
    let mut classpath = Vec::new();
    for library in libraries
        .iter()
        .filter(|library| rules_allow_windows(library))
    {
        let artifact_path = library
            .pointer("/downloads/artifact/path")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| {
                library
                    .get("name")
                    .and_then(|value| value.as_str())
                    .and_then(|coordinate| maven_artifact_path(coordinate).ok())
            });
        if let Some(path) = artifact_path {
            classpath.push(
                game.join("libraries")
                    .join(safe_relative_download_path(&path)?)
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
    classpath.push(
        game.join("versions")
            .join(version)
            .join(format!("{version}.jar"))
            .to_string_lossy()
            .to_string(),
    );
    let classpath = classpath.join(";");
    let game_path = game.to_string_lossy().to_string();
    let assets_root = game.join("assets").to_string_lossy().to_string();
    let libraries_path = game.join("libraries").to_string_lossy().to_string();
    let natives_path = game
        .join("versions")
        .join(version)
        .join("natives")
        .to_string_lossy()
        .to_string();
    let asset_index = details
        .pointer("/assetIndex/id")
        .and_then(|value| value.as_str())
        .unwrap_or(version);
    let version_type = details
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("release");
    let replacements = [
        ("${auth_player_name}", player_name),
        ("${version_name}", version),
        ("${game_directory}", game_path.as_str()),
        ("${assets_root}", assets_root.as_str()),
        ("${assets_index_name}", asset_index),
        ("${auth_uuid}", player_uuid),
        ("${auth_access_token}", access_token),
        ("${user_type}", user_type),
        ("${version_type}", version_type),
        ("${natives_directory}", natives_path.as_str()),
        ("${launcher_name}", "SH启动器"),
        ("${launcher_version}", "0.1.0"),
        ("${classpath}", classpath.as_str()),
        ("${classpath_separator}", ";"),
        ("${library_directory}", libraries_path.as_str()),
        ("${resolution_width}", "1280"),
        ("${resolution_height}", "720"),
        ("${auth_xuid}", xuid),
        ("${clientid}", ""),
    ];
    let java_temp = game.join(".launcher-cache").join("tmp");
    fs::create_dir_all(&java_temp).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut arguments = vec![
        format!("-Xmx{memory_mb}M"),
        "-XX:+UnlockExperimentalVMOptions".into(),
        "-XX:+UseG1GC".into(),
        "-XX:+ParallelRefProcEnabled".into(),
        "-XX:MaxGCPauseMillis=200".into(),
        "-XX:+DisableExplicitGC".into(),
        "-XX:G1NewSizePercent=30".into(),
        "-XX:G1MaxNewSizePercent=40".into(),
        "-XX:G1ReservePercent=20".into(),
        "-XX:InitiatingHeapOccupancyPercent=15".into(),
        "-XX:SurvivorRatio=32".into(),
        "-XX:MaxTenuringThreshold=1".into(),
        "-XX:+PerfDisableSharedMem".into(),
        "-Dfile.encoding=UTF-8".into(),
        format!("-Djava.io.tmpdir={}", java_temp.display()),
    ];
    if let Some(jvm) = details
        .pointer("/arguments/jvm")
        .and_then(|value| value.as_array())
    {
        for entry in jvm {
            for value in argument_values(entry) {
                arguments.push(substitute_argument(value, &replacements)?);
            }
        }
    } else {
        arguments.extend([
            format!("-Djava.library.path={natives_path}"),
            "-cp".into(),
            classpath.clone(),
        ]);
    }
    arguments.push(
        details
            .get("mainClass")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LauncherError::storage("版本元数据缺少 mainClass。"))?
            .to_string(),
    );
    if let Some(game_arguments) = details
        .pointer("/arguments/game")
        .and_then(|value| value.as_array())
    {
        for entry in game_arguments {
            for value in argument_values(entry) {
                arguments.push(substitute_argument(value, &replacements)?);
            }
        }
    } else if let Some(legacy) = details
        .get("minecraftArguments")
        .and_then(|value| value.as_str())
    {
        for value in tokenize_arguments(legacy) {
            arguments.push(substitute_argument(value.to_string(), &replacements)?);
        }
    } else {
        return Err(LauncherError::storage("版本元数据缺少游戏参数。"));
    }
    Ok(arguments)
}

/// shell-like 参数分词：支持双引号、单引号、转义和连续空白，
/// 每个参数独立传给 `Command::arg`，不拼 shell 字符串。
fn tokenize_arguments(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    while let Some(character) = chars.next() {
        match quote {
            Some(active_quote) => {
                if character == active_quote {
                    quote = None;
                } else if character == '\\' && active_quote == '"' && chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    current.push(character);
                }
            }
            None => match character {
                '"' | '\'' => {
                    quote = Some(character);
                    in_token = true;
                }
                value if value.is_whitespace() => {
                    if in_token {
                        tokens.push(std::mem::take(&mut current));
                        in_token = false;
                    }
                }
                value => {
                    current.push(value);
                    in_token = true;
                }
            },
        }
    }
    if in_token || !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn append_server_join_arguments(arguments: &mut Vec<String>, address: &str, port: u16) {
    arguments.push("--server".into());
    arguments.push(address.to_string());
    arguments.push("--port".into());
    arguments.push(port.to_string());
}

#[tauri::command]
async fn launch_instance(
    app: AppHandle,
    instance_id: i64,
    account_id: i64,
    java_path: String,
    force: Option<bool>,
    server_address: Option<String>,
    server_port: Option<u16>,
    server_id: Option<i64>,
) -> Result<LaunchResult, LauncherError> {
    let _ = app.emit(
        "game-preparing",
        serde_json::json!({ "instanceId": instance_id }),
    );
    let connection = open_database(&app)?;
    let (root_path, version, loader, memory_mb, status): (String, String, String, i64, String) = connection.query_row(
        "SELECT root_path, game_version, loader_type, memory_mb, status FROM instances WHERE id=?1", [instance_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).map_err(|_| LauncherError::validation("实例不存在。"))?;
    if !matches!(
        loader.as_str(),
        "vanilla" | "fabric" | "quilt" | "forge" | "neoforge"
    ) {
        return Err(LauncherError::validation("实例加载器类型无效。"));
    }
    if status != "ready" {
        return Err(LauncherError::validation("实例尚未完成安装或校验。"));
    }
    let force = force.unwrap_or(false);
    let server_address = server_address
        .map(|value| validate_server_address(&value))
        .transpose()?;
    let server_port = if server_address.is_some() {
        Some(validate_server_port(server_port.unwrap_or(25565))?)
    } else {
        None
    };
    // 启动不再联网补齐前置：只做本地快速校验，缺前置由前端弹窗让用户选择处理，启动不被网络拖慢
    if !force {
        validate_instance_mods(&root_path, &version, &loader)?;
    }
    let (player_name, account_type, secret_ref): (String, String, Option<String>) = connection
        .query_row(
            "SELECT display_name, account_type, credential_ref FROM accounts WHERE id=?1",
            [account_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| LauncherError::validation("请选择有效的账户。"))?;
    drop(connection);
    let (player_uuid, access_token, user_type, xuid, authlib_javaagent) = if account_type
        == "MICROSOFT"
    {
        let secret_ref = secret_ref
            .ok_or_else(|| LauncherError::validation("Microsoft 凭据不存在，请重新登录。"))?;
        let entry = keyring::Entry::new("SH启动器", &secret_ref)
            .map_err(|error| LauncherError::storage(format!("无法读取 Windows 凭据：{error}")))?;
        let secret = entry.get_password().map_err(|error| {
            LauncherError::validation(format!("Microsoft 登录已失效，请重新登录：{error}"))
        })?;
        let value: serde_json::Value = serde_json::from_str(&secret)
            .map_err(|_| LauncherError::validation("Microsoft 凭据格式无效，请重新登录。"))?;
        let refresh_token = value
            .get("refreshToken")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let client_id = get_settings(app.clone())?
            .microsoft_client_id
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| LauncherError::validation("Microsoft Client ID 未配置，请重新登录。"))?;
        let refreshed = auth::refresh(&client_id, refresh_token)
            .await
            .map_err(LauncherError::validation)?;
        let updated_secret = serde_json::json!({
            "refreshToken": refreshed.refresh_token,
            "accessToken": refreshed.access_token,
            "uuid": refreshed.profile.uuid,
            "xuid": refreshed.profile.xuid,
        });
        entry
            .set_password(&updated_secret.to_string())
            .map_err(|error| LauncherError::storage(format!("无法更新 Microsoft 凭据：{error}")))?;
        (
            updated_secret
                .get("uuid")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            updated_secret
                .get("accessToken")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            "msa".to_string(),
            updated_secret
                .get("xuid")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            None,
        )
    } else if account_type == "EXTERNAL" {
        let secret_ref = secret_ref
            .ok_or_else(|| LauncherError::validation("外置登录凭据不存在，请重新登录。"))?;
        let entry = keyring::Entry::new("SH启动器", &secret_ref)
            .map_err(|error| LauncherError::storage(format!("无法读取 Windows 凭据：{error}")))?;
        let secret = entry.get_password().map_err(|error| {
            LauncherError::validation(format!("外置登录已失效，请重新登录：{error}"))
        })?;
        let value: serde_json::Value = serde_json::from_str(&secret)
            .map_err(|_| LauncherError::validation("外置登录凭据格式无效，请重新登录。"))?;
        let access_token = value
            .get("accessToken")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let client_token = value
            .get("clientToken")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let api_root = value
            .get("apiRoot")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LauncherError::validation("外置登录凭据缺少服务器地址，请重新登录。"))?
            .to_string();
        let uuid = value
            .get("uuid")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let mut refreshed_token = access_token;
        if let Ok(Some((new_token, new_client))) =
            refresh_external_token(&api_root, &refreshed_token, &client_token).await
        {
            refreshed_token = new_token;
            let updated = serde_json::json!({
                "accessToken": refreshed_token,
                "clientToken": new_client,
                "uuid": uuid,
                "apiRoot": api_root,
            });
            let _ = entry.set_password(&updated.to_string());
        }
        let jar = ensure_authlib_injector().await?;
        (
            uuid,
            refreshed_token,
            "legacy".to_string(),
            String::new(),
            Some(format!("-javaagent:{}={}", jar.display(), api_root)),
        )
    } else {
        (
            minecraft_offline_uuid(&player_name).to_string(),
            "0".to_string(),
            "legacy".to_string(),
            String::new(),
            None,
        )
    };
    let java = PathBuf::from(java_path);
    if !java.is_file()
        || java
            .file_name()
            .and_then(|value| value.to_str())
            .is_none_or(|name| !name.eq_ignore_ascii_case("java.exe"))
    {
        return Err(LauncherError::validation("Java 可执行文件无效。"));
    }
    let game = PathBuf::from(&root_path).join(".minecraft");
    let version_directory = game.join("versions").join(&version);
    let metadata_path = if loader == "vanilla" {
        version_directory.join(format!("{version}.json"))
    } else {
        version_directory.join("launcher-effective.json")
    };
    let metadata = fs::read(&metadata_path)
        .map_err(|_| LauncherError::validation("本地版本元数据缺失，请重新安装实例。"))?;
    if metadata.len() > MAX_VERSION_JSON_BYTES {
        return Err(LauncherError::validation("本地版本元数据超过安全限制。"));
    }
    let details: serde_json::Value = serde_json::from_slice(&metadata)
        .map_err(|error| LauncherError::storage(format!("本地版本元数据无效：{error}")))?;
    let runtime = inspect_java_runtime(&java)?;
    if !runtime.is_64_bit {
        return Err(LauncherError::validation("游戏仅允许使用 64 位 Java。"));
    }
    if let Some(required) = details
        .pointer("/javaVersion/majorVersion")
        .and_then(|value| value.as_u64())
    {
        if runtime.major_version != Some(required as u32) {
            return Err(LauncherError::validation(format!(
                "此 Minecraft 版本要求 Java {required}，当前选择的是 Java {}。请在设置中切换或安装匹配版本。",
                runtime
                    .major_version
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "未知".into())
            )));
        }
    }
    let mut arguments = build_vanilla_launch_arguments(
        &details,
        &game,
        &version,
        &player_name,
        &player_uuid,
        &access_token,
        &user_type,
        &xuid,
        memory_mb,
    )?;
    if let Some(javaagent) = authlib_javaagent {
        arguments.insert(0, javaagent);
    }
    if let Some(address) = &server_address {
        append_server_join_arguments(&mut arguments, address, server_port.unwrap_or(25565));
    }
    if get_settings(app.clone())?.backup_worlds_before_launch {
        let _ = backup_instance_worlds(instance_id, &game)?;
    }
    let logs = game.join("logs");
    fs::create_dir_all(&logs).map_err(|error| LauncherError::storage(error.to_string()))?;
    let log_path = logs.join(format!("launcher-{}.log", unique_timestamp()));
    let stdout =
        fs::File::create(&log_path).map_err(|error| LauncherError::storage(error.to_string()))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut child = Command::new(&java)
        .args(&arguments)
        .current_dir(&game)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| LauncherError::storage(format!("启动 Java 失败：{error}")))?;
    let process_id = child.id();
    running_games()
        .lock()
        .map_err(|_| LauncherError::storage("无法保存游戏运行状态。"))?
        .insert(instance_id, process_id);
    let _ = app.emit(
        "game-running",
        serde_json::json!({ "instanceId": instance_id, "processId": process_id }),
    );
    let started_at = chrono_like_timestamp();
    let connection = open_database(&app)?;
    connection
        .execute(
            "INSERT INTO play_history(instance_id,started_at) VALUES(?1,?2)",
            params![instance_id, started_at],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let history_id = connection.last_insert_rowid();
    connection
        .execute(
            "UPDATE instances SET last_played=?1 WHERE id=?2",
            params![started_at, instance_id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if let Some(server_id) = server_id {
        let _ = connection.execute(
            "UPDATE servers SET last_connected_at=?1 WHERE id=?2",
            params![started_at, server_id],
        );
    }
    let db_path = database_path(&app)?;
    let watcher_log_path = log_path.clone();
    let watcher_app = app.clone();
    std::thread::spawn(move || {
        let exit_code = child.wait().ok().and_then(|status| status.code());
        if let Ok(mut games) = running_games().lock() {
            games.remove(&instance_id);
        }
        if let Ok(connection) = Connection::open(db_path) {
            let _ = connection.execute(
                "UPDATE play_history SET ended_at=?1,exit_code=?2 WHERE id=?3",
                params![chrono_like_timestamp(), exit_code, history_id],
            );
            if exit_code.is_some_and(|code| code != 0) {
                let (cause, confidence, suggestion) = analyze_crash_log(&watcher_log_path);
                let _ = connection.execute("INSERT INTO crash_reports(instance_id,occurred_at,exit_code,log_path,suspected_cause,confidence,suggestion) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![instance_id,chrono_like_timestamp(),exit_code,watcher_log_path.to_string_lossy().to_string(),cause,confidence,suggestion]);
            }
        }
        let event_name = if exit_code.is_some_and(|code| code != 0) {
            "game-crashed"
        } else {
            "game-exited"
        };
        let _ = watcher_app.emit(
            event_name,
            serde_json::json!({ "instanceId": instance_id, "exitCode": exit_code }),
        );
        // “启动后关闭启动器”：UI 只是隐藏窗口，supervisor（本进程）活到游戏退出后
        // 才真正退出，保证 play_history / crash report / game-exited 不丢失。
        if get_settings(watcher_app.clone())
            .map(|settings| settings.close_launcher_after_game_start)
            .unwrap_or(false)
        {
            watcher_app.exit(0);
        }
    });
    Ok(LaunchResult {
        process_id,
        log_path: log_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
async fn fetch_version_details(
    url: String,
    expected_sha1: String,
) -> Result<serde_json::Value, LauncherError> {
    if expected_sha1.len() != 40 || !expected_sha1.chars().all(|value| value.is_ascii_hexdigit()) {
        return Err(LauncherError::validation("版本元数据 SHA-1 无效。"));
    }
    let parsed = validate_metadata_url(&url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("SHLauncher/0.1")
        .build()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    match fetch_version_details_from(&client, &parsed, &expected_sha1).await {
        Ok(value) => Ok(value),
        Err(primary_error) => {
            if let Some(mirror) = bmclapi_mirror_url(&parsed) {
                if let Ok(value) =
                    fetch_version_details_from(&client, &mirror, &expected_sha1).await
                {
                    return Ok(value);
                }
            }
            Err(primary_error)
        }
    }
}

#[tauri::command]
async fn fetch_remote_changelog() -> Result<Vec<serde_json::Value>, LauncherError> {
    let url =
        "https://cdn.jsdelivr.net/gh/Bantanxiaon/minecraft-java-launcher@main/docs/changelog.json";
    let parsed =
        reqwest::Url::parse(url).map_err(|error| LauncherError::storage(error.to_string()))?;
    if parsed.host_str() != Some("cdn.jsdelivr.net") {
        return Err(LauncherError::validation("更新日志地址不受信任。"));
    }
    let client = shared_download_client()?;
    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|error| LauncherError::storage(format!("获取更新日志失败：{error}")))?;
    let response = response
        .error_for_status()
        .map_err(|error| LauncherError::storage(format!("更新日志服务返回错误：{error}")))?;
    if response
        .content_length()
        .is_some_and(|length| length > 1024 * 1024)
    {
        return Err(LauncherError::validation("更新日志超过安全限制。"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(format!("更新日志内容无效：{error}")))?;
    Ok(value.as_array().cloned().unwrap_or_default())
}

#[tauri::command]
async fn fetch_version_manifest(include_snapshots: bool) -> Result<VersionManifest, LauncherError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("SHLauncher/0.1")
        .build()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let bytes = match fetch_manifest_bytes(&client, VERSION_MANIFEST_URL).await {
        Ok(bytes) => bytes,
        Err(primary_error) => {
            const BMCLAPI_MANIFEST: &str =
                "https://bmclapi2.bangbang93.com/mc/game/version_manifest.json";
            match fetch_manifest_bytes(&client, BMCLAPI_MANIFEST).await {
                Ok(bytes) => bytes,
                Err(_) => return Err(primary_error),
            }
        }
    };
    let mut manifest = parse_version_manifest(&bytes)?;
    if !include_snapshots {
        manifest
            .versions
            .retain(|version| version.version_type == "release");
    }
    Ok(manifest)
}

pub(crate) fn chrono_like_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
pub(crate) fn unique_timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            if let Ok(connection) = open_database(_app.handle()) {
                let _ = recover_interrupted_download_jobs(&connection);
            }
            #[cfg(debug_assertions)]
            {
                let app = _app;
                let install_version = std::env::var("LAUNCHER_E2E_VERSION").ok();
                let launch_version = std::env::var("LAUNCHER_E2E_LAUNCH_VERSION").ok();
                let loader_type = std::env::var("LAUNCHER_E2E_LOADER").ok();
                if install_version.is_some() || launch_version.is_some() || loader_type.is_some() {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                    let handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        let report_name;
                        let result = if let Some(loader) = loader_type {
                            report_name = format!("acceptance-loader-{loader}.json");
                            match (
                                std::env::var("LAUNCHER_E2E_GAME_VERSION"),
                                std::env::var("LAUNCHER_E2E_JAVA"),
                            ) {
                                (Ok(game_version), Ok(java)) => {
                                    acceptance::run_loader_install_acceptance(
                                        handle.clone(),
                                        game_version,
                                        loader,
                                        java,
                                    )
                                    .await
                                }
                                _ => Err(LauncherError::validation(
                                    "加载器验收缺少游戏版本或 Java 路径。",
                                )),
                            }
                        } else if let Some(game_version) = launch_version {
                            let loader = std::env::var("LAUNCHER_E2E_LAUNCH_LOADER")
                                .unwrap_or_else(|_| "vanilla".into());
                            report_name = format!("acceptance-launch-{loader}.json");
                            match std::env::var("LAUNCHER_E2E_JAVA") {
                                Ok(java) => {
                                    acceptance::run_vanilla_launch_acceptance(
                                        handle.clone(),
                                        game_version,
                                        java,
                                        loader,
                                    )
                                    .await
                                }
                                Err(_) => Err(LauncherError::validation(
                                    "启动验收缺少 LAUNCHER_E2E_JAVA。",
                                )),
                            }
                        } else {
                            report_name = "acceptance-install.json".into();
                            acceptance::run_vanilla_install_acceptance(
                                handle.clone(),
                                install_version.unwrap_or_default(),
                            )
                            .await
                        };
                        let (report, exit_code) = match result {
                            Ok(report) => (report, 0),
                            Err(error) => (
                                serde_json::json!({
                                    "status":"failed",
                                    "code":error.code,
                                    "message":error.message,
                                    "completedAt":chrono_like_timestamp()
                                }),
                                2,
                            ),
                        };
                        if let Ok(root) = launcher_data_directory() {
                            let _ = fs::write(
                                root.join(report_name),
                                serde_json::to_vec_pretty(&report).unwrap_or_default(),
                            );
                        }
                        handle.exit(exit_code);
                    });
                }
            }
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Folder {
                        path: PathBuf::from(r"D:\MinecraftLauncherData\logs"),
                        file_name: Some("launcher".into()),
                    },
                )])
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            build_info,
            instance_health,
            exit_launcher,
            hide_launcher_window,
            terminate_game,
            list_accounts,
            create_offline_account,
            login_microsoft,
            login_external,
            microsoft_login_available,
            remove_account,
            list_servers,
            add_server,
            update_server,
            remove_server,
            ping_server,
            list_modpack_archives,
            record_modpack_archive,
            remove_modpack_archive,
            list_instances,
            boot_health_check,
            create_vanilla_instance,
            create_instance_profile,
            rename_instance,
            update_instance_memory,
            clone_instance,
            delete_instance_to_backup,
            fetch_version_manifest,
            fetch_version_details,
            fetch_remote_changelog,
            detect_java_runtimes,
            install_managed_java,
            get_settings,
            save_settings,
            preview_vanilla_install,
            install_vanilla_client,
            cancel_active_downloads,
            cancel_download_job,
            download_perf::download_diagnostics,
            launch_instance,
            multiplayer::multiplayer_prepare,
            multiplayer::multiplayer_start,
            multiplayer::multiplayer_stop,
            multiplayer::multiplayer_state,
            repair_missing_mod_dependencies,
            list_loader_versions,
            install_profile_loader,
            install_java_loader,
            inspect_mod_jar,
            inspect_modpack,
            search_modrinth_projects,
            search_curseforge_projects,
            translate_search_text,
            install_modrinth_mod,
            install_curseforge_url,
            install_curseforge_project,
            download_curseforge_modpack,
            check_mod_updates,
            update_modrinth_mod,
            install_modrinth_modpack,
            import_modrinth_pack,
            import_local_pack,
            import_mmc_pack,
            import_override_pack,
            list_content_items,
            install_mod,
            set_mod_enabled,
            remove_mod_to_backup,
            install_content_archive,
            set_content_enabled,
            remove_content_to_backup,
            import_world,
            backup_world,
            duplicate_world,
            remove_world_to_backup,
            delete_world_permanently,
            remove_incompatible_mods,
            clean_launcher_cache,
            storage::get_storage_overview,
            storage::build_safe_cleanup_plan,
            storage::execute_cleanup_plan,
            storage::list_deleted_instances,
            storage::restore_deleted_instance,
            storage::permanently_delete_instance_backup,
            storage::list_staging_operations,
            storage::cleanup_staging_operation,
            content_reconcile::reconcile_scan,
            content_reconcile::reconcile_apply,
            list_removed_backups,
            restore_removed_backup,
            exports::export_instance_modpack,
            exports::export_world,
            system::open_instance_directory,
            diagnostics::list_download_jobs,
            diagnostics::list_crash_reports,
            diagnostics::list_game_logs,
            diagnostics::read_game_log,
            diagnostics::export_diagnostic_report
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub(crate) async fn multiplayer_launch(
    app: AppHandle,
    instance_id: i64,
    account_id: i64,
    java_path: String,
) -> Result<LaunchResult, LauncherError> {
    launch_instance(
        app,
        instance_id,
        account_id,
        java_path,
        Some(true),
        None,
        None,
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmclapi_mirror_maps_official_urls() {
        let manifest =
            reqwest::Url::parse("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
                .unwrap();
        assert_eq!(
            bmclapi_mirror_url(&manifest).unwrap().as_str(),
            "https://bmclapi2.bangbang93.com/mc/game/version_manifest.json"
        );
        let details =
            reqwest::Url::parse("https://piston-meta.mojang.com/v1/packages/abc/1.21.1.json")
                .unwrap();
        assert_eq!(
            bmclapi_mirror_url(&details).unwrap().as_str(),
            "https://bmclapi2.bangbang93.com/version/1.21.1/json"
        );
        let client =
            reqwest::Url::parse("https://piston-data.mojang.com/v1/objects/abcdef0123/client.jar")
                .unwrap();
        assert_eq!(
            bmclapi_mirror_url(&client).unwrap().as_str(),
            "https://bmclapi2.bangbang93.com/v1/objects/abcdef0123/client.jar"
        );
        let asset =
            reqwest::Url::parse("https://resources.download.minecraft.net/ab/abcdef1234").unwrap();
        assert_eq!(
            bmclapi_mirror_url(&asset).unwrap().as_str(),
            "https://bmclapi2.bangbang93.com/assets/ab/abcdef1234"
        );
        let library =
            reqwest::Url::parse("https://libraries.minecraft.net/com/example/lib.jar").unwrap();
        assert_eq!(
            bmclapi_mirror_url(&library).unwrap().as_str(),
            "https://bmclapi2.bangbang93.com/maven/com/example/lib.jar"
        );
        let forge =
            reqwest::Url::parse("https://maven.minecraftforge.net/net/minecraftforge/forge.jar")
                .unwrap();
        assert_eq!(
            bmclapi_mirror_url(&forge).unwrap().as_str(),
            "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge.jar"
        );
        assert!(
            bmclapi_mirror_url(&reqwest::Url::parse("https://api.modrinth.com/v2/x").unwrap())
                .is_none()
        );
        assert!(bmclapi_mirror_url(
            &reqwest::Url::parse("https://bmclapi2.bangbang93.com/version/1.21.1/json").unwrap()
        )
        .is_none());
    }

    #[test]
    fn world_delete_target_stays_inside_saves() {
        let root = std::env::temp_dir().join(format!("sh-world-delete-{}", unique_timestamp()));
        let saves = root.join("saves");
        let world = saves.join("my-world");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("level.dat"), b"level").unwrap();
        assert_eq!(
            validated_world_delete_target(&saves, "my-world").unwrap(),
            world.canonicalize().unwrap()
        );
        assert!(validated_world_delete_target(&saves, "..\\escape").is_err());
        assert!(validated_world_delete_target(&saves, "..").is_err());
        assert!(validated_world_delete_target(&saves, "missing-world").is_ok());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn curseforge_matcher_resolves_real_pack_dependencies() {
        let cases = [
            (
                "iceandfire",
                "Ice and Fire: Dragons",
                "ice-and-fire-dragons",
            ),
            ("ftblibrary", "FTB Library", "ftb-library"),
            ("cupboard", "Cupboard", "cupboard"),
            ("octolib", "OctoLib", "octolib"),
            ("slashblade", "Slashblade", "slashblade"),
            ("structure_gel", "Structure Gel", "structure-gel"),
            ("tacz", "Tacz", "tacz"),
            (
                "irons_spellbooks",
                "Iron's Spells 'n Spellbooks",
                "irons-spells-n-spellbooks",
            ),
            ("alexscaves", "Alex's Caves", "alexs-caves"),
        ];
        for (dep, name, slug) in cases {
            let normalized_dep = normalize_curseforge_key(dep);
            let normalized_name = normalize_curseforge_key(name);
            let normalized_slug = normalize_curseforge_key(slug);
            let score =
                curseforge_match_score(&normalized_dep, &normalized_name, &normalized_slug, "");
            assert!(
                score >= 60,
                "依赖 {dep} 匹配分数过低：{score}（name={normalized_name}, slug={normalized_slug}）"
            );
        }
    }

    #[test]
    fn boot_mods_scan_is_quiet_for_missing_folders() {
        let vanilla = scan_boot_mods(1, "C:/definitely/missing/instance", "vanilla", "1.21.1");
        assert_eq!(vanilla.mod_count, 0);
        assert!(vanilla.missing_dependencies.is_empty());

        let fabric = scan_boot_mods(2, "C:/definitely/missing/instance", "fabric", "1.21.1");
        assert_eq!(fabric.mod_count, 0);
        assert!(fabric.missing_dependencies.is_empty());
    }

    #[test]
    #[ignore = "calls the live Modrinth API"]
    fn modrinth_search_end_to_end() {
        let projects = tauri::async_runtime::block_on(search_modrinth_projects(
            "sodium".into(),
            "mod".into(),
            Some("1.21.1".into()),
            Some("fabric".into()),
        ))
        .expect("Modrinth search should succeed");
        assert!(!projects.is_empty());
        assert!(projects.iter().all(|project| project.project_type == "mod"));
        let project = projects.first().expect("at least one project");
        let (url, sha1, filename, size) = tauri::async_runtime::block_on(modrinth_primary_file(
            &project.project_id,
            Some("1.21.1"),
            Some("fabric"),
            ".jar",
        ))
        .expect("compatible primary file should resolve");
        assert!(url.starts_with("https://cdn.modrinth.com/"));
        assert_eq!(sha1.len(), 40);
        assert!(filename.ends_with(".jar"));
        assert!(size > 0);
    }
    use std::io::Write;

    fn test_temp_path(name: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-temp");
        fs::create_dir_all(&root).unwrap();
        root.join(format!("{name}-{}", unique_timestamp()))
    }
    #[test]
    fn profile_validation_accepts_expected_names() {
        assert!(validate_profile_name("Alex_123").is_ok());
    }
    #[test]
    fn profile_validation_rejects_unsafe_names() {
        for value in ["ab", "name-with-dash", "名字", "this_name_is_far_too_long"] {
            assert!(validate_profile_name(value).is_err());
        }
    }
    #[test]
    fn migration_creates_required_tables() {
        let mut connection = Connection::open_in_memory().unwrap();
        run_migrations(&mut connection).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM migrations WHERE version=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let server_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='servers'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(server_count, 1);
        let archive_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='modpack_archives'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(archive_count, 1);
        connection
            .execute(
                "INSERT INTO accounts (account_type, display_name, created_at) VALUES ('EXTERNAL', 'Alex', '1')",
                [],
            )
            .unwrap();
    }
    #[test]
    fn interrupted_downloads_become_retryable_on_startup() {
        let mut connection = Connection::open_in_memory().unwrap();
        run_migrations(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO download_jobs(source_url,target_path,status,created_at,recovery_action)
                 VALUES('https://example.invalid/file','D:\\test','downloading','1','重试下载')",
                [],
            )
            .unwrap();

        assert_eq!(recover_interrupted_download_jobs(&connection).unwrap(), 1);
        let (status, error, action): (String, String, String) = connection
            .query_row(
                "SELECT status,error,recovery_action FROM download_jobs WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "failed");
        assert!(error.contains("意外中断"));
        assert_eq!(action, "重新下载");
    }
    #[test]
    fn instance_field_rejects_path_components() {
        assert!(validate_instance_field("safe-name", 64).is_ok());
        assert!(validate_instance_field("../escape", 64).is_err());
    }
    #[test]
    fn parses_official_manifest_shape() {
        let json = br#"{"latest":{"release":"1.21.1","snapshot":"24w01a"},"versions":[{"id":"1.21.1","type":"release","url":"https://piston-meta.mojang.com/version.json","sha1":"abc","complianceLevel":1}]}"#;
        let manifest = parse_version_manifest(json).unwrap();
        assert_eq!(manifest.latest.release, "1.21.1");
        assert_eq!(manifest.versions[0].version_type, "release");
    }
    #[test]
    fn verifies_sha1_digest() {
        assert!(verify_sha1(
            b"abc",
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        ));
        assert!(!verify_sha1(
            b"abc",
            "0000000000000000000000000000000000000000"
        ));
    }
    #[test]
    fn metadata_url_allows_only_official_https_hosts() {
        assert!(validate_metadata_url("https://piston-meta.mojang.com/v1/version.json").is_ok());
        assert!(validate_metadata_url("http://piston-meta.mojang.com/v1/version.json").is_err());
        assert!(validate_metadata_url("https://example.com/version.json").is_err());
    }
    #[test]
    fn parses_java_properties_and_major_versions() {
        let output = "    java.vendor = Eclipse Adoptium\n    java.version = 21.0.4\n    sun.arch.data.model = 64";
        assert_eq!(
            property_from_java_output(output, "java.vendor").as_deref(),
            Some("Eclipse Adoptium")
        );
        assert_eq!(java_major_version("1.8.0_412"), Some(8));
        assert_eq!(java_major_version("21.0.4"), Some(21));
    }
    #[test]
    fn validates_launcher_settings() {
        assert!(validate_settings(&LauncherSettings::default()).is_ok());
        let invalid = LauncherSettings {
            download_concurrency: 0,
            ..LauncherSettings::default()
        };
        assert!(validate_settings(&invalid).is_err());
    }
    #[test]
    fn builds_vanilla_install_preview() {
        let details = serde_json::json!({"id":"1.21.1","downloads":{"client":{"size":100}},"libraries":[{"downloads":{"artifact":{"size":25}}},{"downloads":{"artifact":{"size":30}}}],"javaVersion":{"majorVersion":21},"mainClass":"net.minecraft.client.main.Main"});
        let preview = install_preview_from_details(7, &details).unwrap();
        assert_eq!(preview.library_count, 2);
        assert_eq!(preview.library_bytes, 55);
        assert_eq!(preview.java_major_version, Some(21));
    }
    #[test]
    fn resource_url_allows_only_official_https_hosts() {
        assert!(validate_resource_url("https://piston-data.mojang.com/v1/client.jar").is_ok());
        assert!(validate_resource_url("https://resources.download.minecraft.net/ab/hash").is_ok());
        assert!(validate_resource_url("https://maven.minecraftforge.net/a/b.jar").is_ok());
        assert!(validate_resource_url(" https://maven.minecraftforge.net/a/b.jar\n").is_ok());
        assert!(validate_resource_url("https://maven.neoforged.net/releases/a/b.jar").is_ok());
        assert!(validate_resource_url("https://example.com/client.jar").is_err());
    }

    #[test]
    #[ignore = "reads a Forge/NeoForge installer supplied by the test environment"]
    fn parses_external_loader_installer_profile() {
        let installer = std::env::var("LAUNCHER_TEST_LOADER_INSTALLER")
            .expect("LAUNCHER_TEST_LOADER_INSTALLER is required");
        let libraries = installer_profile_libraries(Path::new(&installer)).unwrap();
        assert!(!libraries.is_empty());
        for (path, url, sha1, _) in libraries {
            safe_relative_download_path(&path).unwrap();
            validate_resource_url(&url).unwrap();
            assert!(!url.chars().any(char::is_whitespace));
            if let Some(hash) = sha1 {
                assert_eq!(hash, hash.trim());
            }
        }
    }

    #[test]
    fn neoforge_prefix_supports_old_and_new_game_versions() {
        assert_eq!(neoforge_game_prefix("1.21.1").unwrap(), "21.1.");
        assert_eq!(neoforge_game_prefix("26.1").unwrap(), "26.1.");
        assert!(neoforge_game_prefix("release").is_err());
    }
    #[test]
    #[ignore = "downloads and extracts the current official managed OpenJDK"]
    fn managed_java_21_end_to_end() {
        let runtime = tauri::async_runtime::block_on(install_managed_java(21)).unwrap();
        assert_eq!(runtime.major_version, Some(21));
        assert!(runtime.is_64_bit);
        assert!(Path::new(&runtime.path).starts_with(r"D:\MinecraftLauncherData\runtimes"));
    }
    #[test]
    fn inspects_fabric_mod_descriptor() {
        let path = test_temp_path("fabric").with_extension("jar");
        let file = fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(br#"{"id":"example","name":"Example Mod","version":"1.0.0"}"#)
            .unwrap();
        writer.finish().unwrap();
        let inspection = inspect_mod_jar_path(&path).unwrap();
        assert_eq!(inspection.loader_type, "fabric");
        assert_eq!(inspection.mod_id.as_deref(), Some("example"));
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn forge_descriptor_enforces_minecraft_range() {
        let path = test_temp_path("forge-range").with_extension("jar");
        let file = fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(
                "META-INF/mods.toml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer
            .write_all(
                br#"modLoader="javafml"
loaderVersion="[47,)"
[[mods]]
modId="example"
version="1.0.0"
displayName="Example Forge Mod"
[[dependencies.example]]
modId="minecraft"
mandatory=true
versionRange="[1.20,1.21)"
ordering="NONE"
side="BOTH"
"#,
            )
            .unwrap();
        writer.finish().unwrap();
        let inspection = inspect_mod_jar_path(&path).unwrap();
        assert_eq!(inspection.loader_type, "forge");
        assert_eq!(inspection.game_version_requirements, ["[1.20,1.21)"]);
        assert!(ensure_game_version_compatible("1.20.1", &inspection).is_ok());
        assert!(ensure_game_version_compatible("26.2", &inspection).is_err());
        fs::remove_file(path).unwrap();
    }
    #[test]
    #[ignore = "reads a mod JAR supplied by the test environment"]
    fn external_mod_rejects_incompatible_game_version() {
        let source = std::env::var("LAUNCHER_TEST_MOD_JAR").expect("mod JAR path is required");
        let game_version =
            std::env::var("LAUNCHER_TEST_GAME_VERSION").expect("game version is required");
        let temporary = test_temp_path("external-mod").with_extension("jar");
        fs::copy(source, &temporary).unwrap();
        let inspection = inspect_mod_jar_path(&temporary).unwrap();
        assert!(ensure_game_version_compatible(&game_version, &inspection).is_err());
        fs::remove_file(temporary).unwrap();
    }
    #[test]
    fn rejects_jar_path_traversal() {
        let path = test_temp_path("slip").with_extension("jar");
        let file = fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("../evil.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"unsafe").unwrap();
        writer.finish().unwrap();
        assert!(inspect_mod_jar_path(&path).is_err());
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn loader_compatibility_is_strict() {
        assert!(ensure_loader_compatible("fabric", "fabric").is_ok());
        assert!(ensure_loader_compatible("vanilla", "fabric").is_err());
        assert!(ensure_loader_compatible("forge", "neoforge").is_err());
        assert!(ensure_loader_compatible("quilt", "unknown").is_err());
    }

    #[test]
    fn minecraft_version_ranges_reject_wrong_game_versions() {
        assert!(game_version_matches("[1.20,1.21)", "1.20.1"));
        assert!(!game_version_matches("[1.20,1.21)", "26.2"));
        assert!(game_version_matches(">=1.20.1 <1.21", "1.20.6"));
        assert!(!game_version_matches(">=1.20.1 <1.21", "1.21"));
        assert!(game_version_matches("1.20.x", "1.20.4"));
        assert!(game_version_matches("[26.2,)", "26.2"));
    }
    #[test]
    fn jar_file_name_cannot_escape_instance() {
        assert_eq!(
            safe_jar_file_name(Path::new("example.jar")).unwrap(),
            "example.jar"
        );
        assert!(safe_jar_file_name(Path::new("not-a-mod.zip")).is_err());
        assert!(safe_jar_file_name(Path::new("../escape.jar")).is_ok());
        assert_eq!(
            safe_jar_file_name(Path::new("../escape.jar")).unwrap(),
            "escape.jar"
        );
    }
    #[test]
    fn windows_rules_follow_last_matching_action() {
        let allowed = serde_json::json!({"rules":[{"action":"disallow"},{"action":"allow","os":{"name":"windows"}}]});
        let linux_only = serde_json::json!({"rules":[{"action":"allow","os":{"name":"linux"}}]});
        let feature_only =
            serde_json::json!({"rules":[{"action":"allow","features":{"is_demo_user":true}}]});
        assert!(rules_allow_windows(&allowed));
        assert!(!rules_allow_windows(&linux_only));
        assert!(!rules_allow_windows(&feature_only));
    }
    #[test]
    fn download_paths_are_confined() {
        assert!(safe_relative_download_path("org/example/library.jar").is_ok());
        assert!(safe_relative_download_path("../escape.jar").is_err());
        assert!(safe_relative_download_path("C:\\escape.jar").is_err());
    }
    #[test]
    fn inspects_modrinth_pack_manifest() {
        let path = test_temp_path("pack").with_extension("mrpack");
        let file = fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(
                "modrinth.index.json",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(br#"{"name":"Pack","versionId":"1","dependencies":{"minecraft":"1.21.1","fabric-loader":"0.16.0"},"files":[{"path":"mods/example.jar"}]}"#).unwrap();
        writer.finish().unwrap();
        let inspection = inspect_modpack_path(&path).unwrap();
        assert_eq!(inspection.format, "modrinth");
        assert_eq!(inspection.loader_type.as_deref(), Some("fabric"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    #[ignore = "需要用户提供的真实整合包路径"]
    fn inspects_real_curseforge_pack() {
        let path =
            std::env::var("LAUNCHER_TEST_MODPACK").expect("LAUNCHER_TEST_MODPACK is required");
        let started = std::time::Instant::now();
        let inspection = inspect_modpack_path(Path::new(&path)).expect("真实整合包应能通过检查");
        println!(
            "format={} name={:?} version={:?} mc={:?} loader={:?} mods={} overrides={} elapsed_ms={}",
            inspection.format,
            inspection.name,
            inspection.version,
            inspection.game_version,
            inspection.loader_type,
            inspection.mod_count,
            inspection.override_count,
            started.elapsed().as_millis()
        );
        assert_eq!(inspection.format, "curseforge");
        assert_eq!(inspection.loader_type.as_deref(), Some("forge"));
        assert_eq!(inspection.game_version.as_deref(), Some("1.20.1"));
        assert!(inspection.mod_count >= 200, "模组数量异常");
        assert!(
            started.elapsed().as_secs() < 60,
            "整合包检查耗时过长，影响流畅性"
        );
    }

    #[test]
    #[ignore = "联网测试 CurseForge 公开下载"]
    fn curseforge_download_endpoint_live() {
        tauri::async_runtime::block_on(async {
            let client = shared_download_client().expect("client");
            let (name, size) = curseforge_file_info(&client, 264231, 5633453)
                .await
                .expect("file info");
            println!("file={name} size={size}");
            assert!(size > 0);
            let url = reqwest::Url::parse(
                "https://www.curseforge.com/api/v1/mods/264231/files/5633453/download",
            )
            .expect("url");
            let response = send_download_request(&client, &url, None)
                .await
                .expect("download request");
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let bytes = response.bytes().await.expect("body");
            println!("downloaded {} bytes", bytes.len());
            assert!(bytes.len() > 1000);
        });
    }
    #[test]
    fn validates_resource_and_shader_archive_structure() {
        let resource = test_temp_path("resourcepack").with_extension("zip");
        let file = fs::File::create(&resource).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(br#"{"pack":{"pack_format":34}}"#).unwrap();
        writer.finish().unwrap();
        assert!(inspect_content_archive(&resource, "resourcepack").is_ok());
        assert!(inspect_content_archive(&resource, "shaderpack").is_err());
        fs::remove_file(resource).unwrap();

        let shader = test_temp_path("shaderpack").with_extension("zip");
        let file = fs::File::create(&shader).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(
                "shaders/program.fsh",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"void main() {}").unwrap();
        writer.finish().unwrap();
        assert!(inspect_content_archive(&shader, "shaderpack").is_ok());
        fs::remove_file(shader).unwrap();
    }
    #[test]
    fn locates_world_root_and_rejects_ambiguous_folder() {
        let root = test_temp_path("world");
        let world = root.join("My World");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("level.dat"), b"level").unwrap();
        assert_eq!(locate_world_directory(&root).unwrap(), world);
        let second = root.join("Second");
        fs::create_dir_all(&second).unwrap();
        fs::write(second.join("level.dat"), b"level").unwrap();
        assert!(locate_world_directory(&root).is_err());
        assert!(root.starts_with(PathBuf::from(env!("CARGO_MANIFEST_DIR"))));
        fs::remove_dir_all(root).unwrap();
    }

    fn write_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    fn forge_jar_bytes(mod_id: &str, version: &str) -> Vec<u8> {
        let toml = format!(
            "modLoader=\"javafml\"\nloaderVersion=\"[47,)\"\n[[mods]]\nmodId=\"{mod_id}\"\nversion=\"{version}\"\ndisplayName=\"{mod_id}\"\n[[dependencies.{mod_id}]]\nmodId=\"minecraft\"\nmandatory=true\nversionRange=\"[1.20,1.21)\"\n"
        );
        let mut buffer = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut buffer);
        writer
            .start_file(
                "META-INF/mods.toml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(toml.as_bytes()).unwrap();
        writer.finish().unwrap();
        buffer.into_inner()
    }

    #[test]
    fn synthetic_packs_are_detected_in_all_formats() {
        let jar = forge_jar_bytes("example", "1.0.0");
        let curseforge_entries: &[(&str, &[u8])] = &[
            (
                "manifest.json",
                br#"{"minecraft":{"version":"1.20.1","modLoaders":[{"id":"forge-47.4.22"}]},"files":[{"projectID":586095,"fileID":7756316,"required":true}]}"#,
            ),
            ("overrides/mods/example.jar", &jar),
        ];
        let modrinth_entries: &[(&str, &[u8])] = &[
            (
                "modrinth.index.json",
                br#"{"dependencies":{"minecraft":"1.20.1","forge":"47.4.22"},"files":[{"path":"mods/example.jar","hashes":{"sha1":"abc"},"fileSize":1,"downloads":[]}]}"#,
            ),
            ("mods/example.jar", &jar),
        ];
        let hmcl_entries: &[(&str, &[u8])] = &[
            (
                "modpack.json",
                br#"{"name":"HMCL Test","gameVersion":"1.20.1","addons":[{"id":"forge","version":"47.4.22"}]}"#,
            ),
            ("minecraft/mods/example.jar", &jar),
        ];
        let mmc_entries: &[(&str, &[u8])] = &[
            (
                "mmc-pack.json",
                br#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.minecraftforge","version":"47.4.22"}]}"#,
            ),
            (".minecraft/mods/example.jar", &jar),
        ];
        let mcbbs_entries: &[(&str, &[u8])] = &[
            (
                "mcbbs.packmeta",
                br#"{"minecraft":{"version":"1.20.1","modLoaders":[{"id":"forge-47.4.22"}]}}"#,
            ),
            ("overrides/mods/example.jar", &jar),
        ];
        let cases: Vec<(&str, &[(&str, &[u8])], &str)> = vec![
            ("curseforge", curseforge_entries, "curseforge"),
            ("modrinth", modrinth_entries, "modrinth"),
            ("hmcl", hmcl_entries, "hmcl"),
            ("mmc", mmc_entries, "mmc"),
            ("mcbbs", mcbbs_entries, "mcbbs"),
        ];
        for (name, entries, expected_format) in cases {
            let path = test_temp_path(name).with_extension("zip");
            write_test_zip(&path, entries);
            let inspection = inspect_modpack_path(&path)
                .unwrap_or_else(|error| panic!("{name} 应能通过检查：{}", error.message));
            assert_eq!(inspection.format, expected_format, "{name} 格式识别错误");
            assert_eq!(
                inspection.game_version.as_deref(),
                Some("1.20.1"),
                "{name} 游戏版本错误"
            );
            assert_eq!(
                inspection.loader_type.as_deref(),
                Some("forge"),
                "{name} 加载器错误"
            );
            assert!(inspection.mod_count >= 1, "{name} 模组数量错误");
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn curseforge_dependency_resolution_prefers_mod_id() {
        let files = vec![
            serde_json::json!({"projectId":1567856,"fileId":8211489,"fileName":"goety-apostle-fix-1.5.0.jar","modId":"goetyfix"}),
            serde_json::json!({"projectId":586095,"fileId":7756316,"fileName":"goety-2.5.49.3.jar","modId":"goety"}),
            serde_json::json!({"projectId":1236817,"fileId":7638178,"fileName":"GoetyRevelation-2.3.1.jar","modId":"goety_revelation"}),
            serde_json::json!({"projectId":402818,"fileId":6118401,"fileName":"patchouli-1.20.1-81-FORGE.jar","modId":"patchouli"}),
        ];
        assert_eq!(
            best_curseforge_match(&files, "goety"),
            Some((586095, 7756316))
        );
        assert_eq!(
            best_curseforge_match(&files, "goetyfix"),
            Some((1567856, 8211489))
        );
        assert_eq!(
            best_curseforge_match(&files, "goety_revelation"),
            Some((1236817, 7638178))
        );
        assert_eq!(
            best_curseforge_match(&files, "patchouli"),
            Some((402818, 6118401))
        );
    }

    #[test]
    fn curseforge_dependency_resolution_uses_file_name_when_mod_id_missing() {
        let files = vec![
            serde_json::json!({"projectId":586095,"fileId":7756316,"fileName":"goety-2.5.49.3.jar","modId":""}),
            serde_json::json!({"projectId":402818,"fileId":6118401,"fileName":"patchouli-1.20.1-81-FORGE.jar","modId":""}),
        ];
        assert_eq!(
            best_curseforge_match(&files, "goety"),
            Some((586095, 7756316))
        );
        assert_eq!(
            best_curseforge_match(&files, "patchouli"),
            Some((402818, 6118401))
        );
        assert_eq!(best_curseforge_match(&files, "iceandfire"), None);
    }

    #[test]
    fn server_address_rejects_embedded_port() {
        assert!(validate_server_address("play.example.com:25565").is_err());
        assert!(validate_server_address("play.example.com").is_ok());
        assert!(validate_server_address("192.168.1.10").is_ok());
        assert!(validate_server_address("[2001:db8::1]").is_ok());
        assert!(validate_server_address("2001:db8::1").is_ok());
        assert!(validate_server_address("bad path").is_err());
    }

    #[test]
    fn server_port_rejects_zero() {
        assert!(validate_server_port(0).is_err());
        assert!(validate_server_port(25565).is_ok());
    }

    #[test]
    fn authlib_api_root_allows_https_and_localhost_http() {
        assert!(normalize_authlib_api_root("https://littleskin.cn/api/yggdrasil").is_ok());
        assert!(normalize_authlib_api_root("http://localhost:8080").is_ok());
        assert!(normalize_authlib_api_root("http://127.0.0.1/api").is_ok());
        assert!(normalize_authlib_api_root("http://example.com/api").is_err());
        assert!(normalize_authlib_api_root("ftp://example.com").is_err());
    }

    #[test]
    fn server_join_arguments_are_appended_in_order() {
        let mut arguments = vec!["-Xmx4096M".to_string()];
        append_server_join_arguments(&mut arguments, "play.example.com", 25565);
        assert_eq!(
            arguments,
            vec![
                "-Xmx4096M",
                "--server",
                "play.example.com",
                "--port",
                "25565"
            ]
        );
    }

    #[test]
    fn ping_server_reports_local_reachable_and_refused() {
        tauri::async_runtime::block_on(async {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let reachable = ping_server("127.0.0.1".into(), port).await.unwrap();
            assert!(reachable.reachable, "本机监听端口应可连接");
            drop(listener);
            let refused = ping_server("127.0.0.1".into(), port).await.unwrap();
            assert!(!refused.reachable, "已关闭端口应连接失败");
        });
    }

    #[test]
    #[ignore = "联网下载 authlib-injector 真实组件"]
    fn authlib_injector_download_live() {
        tauri::async_runtime::block_on(async {
            let path = ensure_authlib_injector().await.expect("应能下载并校验");
            let metadata = std::fs::metadata(&path).expect("文件应存在");
            assert!(metadata.len() >= AUTHLIB_INJECTOR_MIN_BYTES);
        });
    }

    #[test]
    #[ignore = "联网测试 CurseForge 搜索代理"]
    fn curseforge_search_proxy_live() {
        tauri::async_runtime::block_on(async {
            let projects = search_curseforge_projects("create".into(), "mod".into(), None, None)
                .await
                .expect("应能通过代理搜索 CurseForge");
            assert!(!projects.is_empty(), "应返回搜索结果");
            assert!(
                projects
                    .iter()
                    .all(|project| project.source == "curseforge"),
                "来源应标记为 curseforge"
            );
        });
    }

    #[test]
    fn missing_dependencies_reports_kotlinforforge_when_absent() {
        let installed: HashSet<String> = HashSet::new();
        let missing = missing_dependencies(
            ["kotlinforforge", "patchouli"].into_iter(),
            &installed,
            false,
        );
        assert!(
            missing.contains("kotlinforforge"),
            "缺少 kotlinforforge 时必须报出"
        );
        assert!(missing.contains("patchouli"));

        let with_kotlin = missing_dependencies(
            ["kotlinforforge", "patchouli"].into_iter(),
            &installed,
            true,
        );
        assert!(
            !with_kotlin.contains("kotlinforforge"),
            "存在 kotlinforforge 文件时不应再报缺失"
        );
        assert!(with_kotlin.contains("patchouli"));
    }

    #[test]
    fn resolver_rejects_ambiguous_same_slug_projects() {
        let hits = vec![
            serde_json::json!({"project_id": "AAA", "slug": "bookshelf", "title": "Bookshelf"}),
            serde_json::json!({"project_id": "BBB", "slug": "bookshelf", "title": "Bookshelf"}),
        ];
        let matches = exact_candidate_matches("bookshelf", &hits);
        assert_eq!(
            matches.len(),
            2,
            "同名多项目必须返回 AMBIGUOUS 候选而非自动安装第一条"
        );
    }

    #[test]
    fn resolver_unknown_mod_without_exact_match_returns_no_candidates() {
        // “第 11 个未知 Mod”：不在别名表，modId 与任何 provider slug 都不同。
        let hits = vec![
            serde_json::json!({"project_id": "uy4Cnpcm", "slug": "bookshelf-lib", "title": "Bookshelf-Lib"}),
            serde_json::json!({"project_id": "1OE8wbN0", "slug": "prism-lib", "title": "Prism-Lib"}),
            serde_json::json!({"project_id": "SzzJttH8", "slug": "timeless-and-classics-zero", "title": "Timeless and Classics Zero"}),
        ];
        let matches = exact_candidate_matches("totally_unknown_mod_xyz", &hits);
        assert!(matches.is_empty(), "未知 modId 不得被静默解析");
    }

    #[test]
    fn modrinth_identity_normalization_is_stable() {
        assert_eq!(
            normalize_modrinth_identity("Iron's Spells 'n Spellbooks"),
            "ironsspellsnspellbooks"
        );
        assert_eq!(
            normalize_modrinth_identity("Timeless and Classics Zero"),
            "timelessandclassicszero"
        );
        assert_eq!(
            normalize_modrinth_identity("irons_spellbooks"),
            "irons-spellbooks"
        );
    }

    #[test]
    fn download_retry_does_not_repeat_not_found() {
        use std::io::{Read as _, Write as _};
        tauri::async_runtime::block_on(async {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counter = requests.clone();
            let handle = std::thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let mut buffer = [0u8; 1024];
                    let _ = stream.read(&mut buffer);
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            });
            let url = reqwest::Url::parse(&format!("http://127.0.0.1:{port}/missing")).unwrap();
            let client = quick_http_client().unwrap();
            let result = send_download_request(&client, &url, None).await;
            assert!(result.is_err(), "404 应返回失败");
            let _ = handle.join();
            assert_eq!(
                requests.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "404 不得反复重试"
            );
        });
    }

    #[test]
    fn kotlinforforge_detected_by_file_name() {
        let dir = std::env::temp_dir().join(format!("sh-kff-test-{}", unique_timestamp()));
        let mods = dir.join("mods");
        fs::create_dir_all(&mods).unwrap();
        fs::write(mods.join("kotlinforforge-4.12.0-all.jar"), b"x").unwrap();
        assert!(has_kotlinforforge_file(&mods));
        assert!(!has_kotlinforforge_file(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_uuid_matches_java_name_uuid_from_bytes() {
        // 与 Java 交叉验证的固定值：
        // UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes(StandardCharsets.UTF_8))
        let expected = uuid::Uuid::parse_str("b50ad385-829d-3141-a216-7e7d7539ba7f").unwrap();
        assert_eq!(minecraft_offline_uuid("Notch"), expected);
        assert_ne!(
            minecraft_offline_uuid("Notch"),
            minecraft_offline_uuid("Alex")
        );
    }

    #[test]
    fn legacy_arguments_tokenize_quotes_and_spaces() {
        let tokens = tokenize_arguments(
            r#"--username Alex --gameDir "C:\Users\张三\My Games\world" --demo "quoted \"arg\"" 'single'"#,
        );
        assert_eq!(
            tokens,
            vec![
                "--username",
                "Alex",
                "--gameDir",
                r"C:\Users\张三\My Games\world",
                "--demo",
                r#"quoted "arg""#,
                "single",
            ]
        );
    }

    #[test]
    fn duplicate_display_names_do_not_overwrite_account_identity() {
        let mut connection = Connection::open_in_memory().unwrap();
        run_migrations(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO accounts (account_type, minecraft_uuid, display_name, legacy_offline_uuid, created_at)
                 VALUES ('OFFLINE', ?1, 'Alex', ?2, '1')",
                params![minecraft_offline_uuid("Alex").to_string(), legacy_offline_uuid("Alex")],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO accounts (account_type, minecraft_uuid, display_name, legacy_offline_uuid, created_at)
                 VALUES ('EXTERNAL', ?1, 'Alex', NULL, '2')",
                params![uuid::Uuid::new_v4().to_string()],
            )
            .expect("同名但身份不同的账户应可共存，display_name 不再是唯一约束");
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
