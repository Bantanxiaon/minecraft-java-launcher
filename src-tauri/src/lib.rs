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
};
#[cfg(debug_assertions)]
use tauri::Manager;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(debug_assertions)]
mod acceptance;
mod auth;
mod diagnostics;
mod exports;
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

fn launcher_data_directory() -> Result<PathBuf, LauncherError> {
    let directory = std::env::var_os("MINECRAFT_LAUNCHER_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\MinecraftLauncherData"));
    if !directory.is_absolute() || directory.components().next().is_none_or(|component| !matches!(component, Component::Prefix(prefix) if prefix.as_os_str().to_string_lossy().eq_ignore_ascii_case("D:"))) {
        return Err(LauncherError::validation("启动器数据目录必须是 D 盘绝对路径。"));
    }
    fs::create_dir_all(&directory).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(directory)
}

fn database_path(_app: &AppHandle) -> Result<PathBuf, LauncherError> {
    let directory = launcher_data_directory()?;
    Ok(directory.join("launcher.sqlite3"))
}

fn run_migrations(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch("PRAGMA foreign_keys = ON;
    CREATE TABLE IF NOT EXISTS migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS accounts (id INTEGER PRIMARY KEY, account_type TEXT NOT NULL CHECK(account_type IN ('OFFLINE','MICROSOFT')), display_name TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL, last_used_at TEXT, safe_secret_ref TEXT);
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
        connection.execute(
            "ALTER TABLE download_jobs ADD COLUMN started_at TEXT",
            [],
        )?;
    }
    if !existing.iter().any(|name| name == "updated_at") {
        connection.execute(
            "ALTER TABLE download_jobs ADD COLUMN updated_at TEXT",
            [],
        )?;
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
    Ok(())
}

pub(crate) fn open_database(app: &AppHandle) -> Result<Connection, LauncherError> {
    let connection = Connection::open(database_path(app)?)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    run_migrations(&connection).map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(connection)
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
    connection.execute("INSERT INTO accounts (account_type, display_name, created_at, last_used_at) VALUES ('OFFLINE', ?1, ?2, ?2)", params![display_name, created_at]).map_err(|error| LauncherError::storage(error.to_string()))?;
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
            "INSERT INTO accounts (account_type, display_name, created_at, last_used_at) VALUES ('MICROSOFT', ?1, ?2, ?2) ON CONFLICT(display_name) DO UPDATE SET account_type='MICROSOFT', last_used_at=excluded.last_used_at",
            params![profile_name, created_at],
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

#[tauri::command]
fn remove_account(app: AppHandle, account_id: i64) -> Result<(), LauncherError> {
    let connection = open_database(&app)?;
    let secret_ref: Option<String> = connection
        .query_row(
            "SELECT safe_secret_ref FROM accounts WHERE id=?1",
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
        let next = tokio::time::timeout(Duration::from_secs(60), stream.next())
            .await
            .map_err(|_| {
                LauncherError::storage(
                    "Java 下载连续 60 秒没有收到数据。请检查网络后重试；已下载的部分会保留。",
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
    let file =
        fs::File::open(archive_path).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| LauncherError::validation(format!("Java ZIP 无效：{error}")))?;
    if archive.len() > 100_000 {
        return Err(LauncherError::validation("Java ZIP 条目数超过安全限制。"));
    }
    fs::create_dir_all(destination).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut expanded = 0u64;
    let mut java_path = None;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        let relative = safe_relative_download_path(entry.name())?;
        expanded = expanded.saturating_add(entry.size());
        if expanded > 2 * 1024 * 1024 * 1024
            || (entry.compressed_size() > 0 && entry.size() / entry.compressed_size().max(1) > 200)
        {
            return Err(LauncherError::validation("Java ZIP 解压规模异常。"));
        }
        let output = destination.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        let mut output_file =
            fs::File::create(&output).map_err(|error| LauncherError::storage(error.to_string()))?;
        std::io::copy(&mut entry, &mut output_file)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if output
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("java.exe"))
            && output
                .parent()
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("bin"))
        {
            java_path = Some(output);
        }
    }
    java_path.ok_or_else(|| LauncherError::validation("Java ZIP 中未找到 bin/java.exe。"))
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

fn running_games() -> &'static Mutex<HashMap<i64, u32>> {
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
    if requirement.is_empty() || matches!(requirement, "*" | "${minecraft_version}") {
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
                let first = value
                    .get("mods")
                    .and_then(|value| value.as_array())
                    .and_then(|mods| mods.first());
                loader_type = loader.into();
                mod_id = first
                    .and_then(|value| value.get("modId"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                display_name = first
                    .and_then(|value| value.get("displayName"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                version = first
                    .and_then(|value| value.get("version"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                if let Some(id) = mod_id.as_deref() {
                    if let Some(items) = value
                        .get("dependencies")
                        .and_then(|entry| entry.get(id))
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
    let installed_ids = inspections
        .iter()
        .filter_map(|inspection| inspection.mod_id.as_deref())
        .map(|id| id.to_ascii_lowercase())
        .collect::<HashSet<_>>();
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
    let missing = inspections
        .iter()
        .flat_map(|inspection| inspection.dependencies.iter())
        .map(|id| id.to_ascii_lowercase())
        .filter(|id| !provided.contains(&id.as_str()) && !installed_ids.contains(id))
        .collect::<std::collections::BTreeSet<_>>();
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
    unresolved_remote_files: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnlineProject {
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
    let response = send_download_request(&shared_download_client()?, &url, None)
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
    Ok(hits
        .iter()
        .filter_map(|hit| {
            Some(OnlineProject {
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
        .collect())
}

async fn modrinth_primary_file(
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
    extension: &str,
) -> Result<(String, String, String, u64), LauncherError> {
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
    let versions = versions
        .as_array()
        .ok_or_else(|| LauncherError::storage("Modrinth 版本结果无效。"))?;
    let selected = versions
        .iter()
        .find(|value| value.get("version_type").and_then(|value| value.as_str()) == Some("release"))
        .or_else(|| versions.first())
        .ok_or_else(|| LauncherError::validation("没有找到与实例兼容的 Modrinth 文件。"))?;
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
    ))
}

async fn modrinth_compatible_version(
    project_id: &str,
    game_version: &str,
    loader: &str,
) -> Result<serde_json::Value, LauncherError> {
    validate_modrinth_project_id(project_id)?;
    let mut url = reqwest::Url::parse(&format!(
        "https://api.modrinth.com/v2/project/{project_id}/version"
    ))
    .map_err(|error| LauncherError::storage(error.to_string()))?;
    url.query_pairs_mut()
        .append_pair(
            "game_versions",
            &serde_json::to_string(&[game_version]).unwrap_or_default(),
        )
        .append_pair(
            "loaders",
            &serde_json::to_string(&[loader]).unwrap_or_default(),
        );
    let versions = fetch_modrinth_json(url).await?;
    let versions = versions
        .as_array()
        .ok_or_else(|| LauncherError::storage("Modrinth 版本结果无效。"))?;
    versions
        .iter()
        .find(|value| value.get("version_type").and_then(|value| value.as_str()) == Some("release"))
        .or_else(|| versions.first())
        .cloned()
        .ok_or_else(|| {
            LauncherError::validation(format!("{project_id} 没有与当前实例兼容的版本。"))
        })
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
            return Err(LauncherError::validation(format!(
                "检测到 Modrinth 循环依赖：{project_id}"
            )));
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
        object.insert("modrinthSha1".into(), serde_json::Value::String(sha1));
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
        let url = reqwest::Url::parse(&format!(
            "https://api.modrinth.com/v2/project/{candidate}"
        ))
        .map_err(|error| LauncherError::storage(error.to_string()))?;
        if let Ok(value) = fetch_modrinth_json(url).await {
            if value
                .get("project_type")
                .and_then(|value| value.as_str())
                == Some("mod")
            {
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
        for hit in hits {
            let slug = hit.get("slug").and_then(|value| value.as_str());
            let project_id = hit.get("project_id").and_then(|value| value.as_str());
            let title = hit.get("title").and_then(|value| value.as_str());
            let title_matches = title.is_some_and(|value| {
                value.eq_ignore_ascii_case(input)
                    || value
                        .replace([' ', '_'], "-")
                        .eq_ignore_ascii_case(candidate)
            });
            if slug.is_some_and(|value| value.eq_ignore_ascii_case(candidate))
                || project_id.is_some_and(|value| value.eq_ignore_ascii_case(input))
                || title_matches
            {
                if let Some(project_id) = project_id {
                    return Ok(project_id.to_string());
                }
            }
        }
    }
    Err(LauncherError::validation(format!(
        "没有在 Modrinth 找到前置模组“{input}”。请确认模组来源，或手动安装该前置模组。"
    )))
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
    let mut installed_ids = inspections
        .iter()
        .filter_map(|inspection| inspection.mod_id.as_deref())
        .map(|id| id.to_ascii_lowercase())
        .collect::<HashSet<_>>();
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
    let mut missing = BTreeSet::new();
    for inspection in &inspections {
        for dependency in &inspection.dependencies {
            let dependency = dependency.to_ascii_lowercase();
            if !provided.iter().any(|item| *item == dependency)
                && !installed_ids.contains(&dependency)
            {
                missing.insert(dependency);
            }
        }
    }
    let mut failures = Vec::new();
    for missing_id in missing {
        let project_id = match resolve_modrinth_project_id(&missing_id).await {
            Ok(project_id) => project_id,
            Err(error) => {
                failures.push(format!("{missing_id}：{}", error.message));
                continue;
            }
        };
        match install_modrinth_mod(app.clone(), instance_id, project_id).await {
            Ok(item) => {
                if let Some(metadata_json) = item.metadata_json.as_deref() {
                    if let Ok(metadata) =
                        serde_json::from_str::<serde_json::Value>(metadata_json)
                    {
                        if let Some(mod_id) = metadata
                            .get("modId")
                            .and_then(|value| value.as_str())
                        {
                            installed_ids.insert(mod_id.to_ascii_lowercase());
                        }
                    }
                }
            }
            Err(error) if error.message.contains("已存在相同 Mod ID") => {
                if !installed_ids.contains(&missing_id) {
                    failures.push(format!("{missing_id}：{}", error.message));
                }
            }
            Err(error) => failures.push(format!("{missing_id}：{}", error.message)),
        }
    }
    if !failures.is_empty() {
        return Err(LauncherError::validation(format!(
            "自动补齐前置模组失败：\n- {}\n请检查网络后重试，或手动安装这些前置模组。",
            failures.join("\n- ")
        )));
    }
    Ok(())
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
    fs::rename(&old_path, &backup)
        .map_err(|error| LauncherError::storage(format!("备份旧模组失败：{error}")))?;
    if let Err(error) = fs::rename(&staged, &destination) {
        let _ = fs::rename(&backup, &old_path);
        return Err(LauncherError::storage(format!("替换模组失败：{error}")));
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
        let _ = fs::remove_file(&destination);
        let _ = fs::rename(&backup, &old_path);
        return Err(LauncherError::storage(error.to_string()));
    }
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
    let file = fs::File::open(source).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| LauncherError::validation(error.to_string()))?;
    let mut count = 0usize;
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
        let relative = normalized
            .strip_prefix("overrides/")
            .or_else(|| normalized.strip_prefix("client-overrides/"));
        let Some(relative) = relative else { continue };
        if relative.is_empty() {
            continue;
        }
        let output = pack_target_path(game, relative)?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        let temporary = output.with_extension(format!("part-{}", unique_timestamp()));
        let mut file = fs::File::create(&temporary)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        std::io::copy(&mut entry, &mut file)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if output.exists() {
            move_pack_collision_to_backup(game, &output)?;
        }
        fs::rename(temporary, output).map_err(|error| LauncherError::storage(error.to_string()))?;
        count += 1;
    }
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

#[tauri::command]
fn import_local_pack(
    app: AppHandle,
    instance_id: i64,
    source_path: String,
) -> Result<ImportedLocalPack, LauncherError> {
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
    let game = PathBuf::from(root_path).join(".minecraft");
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
                return Err(error);
            }
        };
        if output.exists() {
            move_pack_collision_to_backup(&game, &output)?;
        }
        fs::rename(&temporary, &output)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if let Some(info) = mod_info {
            let metadata = serde_json::to_string(&info)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            connection.execute("INSERT OR IGNORE INTO content_items(instance_id,kind,file_name,hash,metadata_json,enabled,source,installed_at) VALUES(?1,'mod',?2,?3,?4,1,'local-pack',?5)", params![instance_id, output.file_name().and_then(|value| value.to_str()).unwrap_or("mod.jar"), info.sha256, metadata, chrono_like_timestamp()]).map_err(|error| LauncherError::storage(error.to_string()))?;
            imported_mods += 1;
        }
        imported_files += 1;
    }
    let unresolved_remote_files = if inspection.format == "curseforge" {
        inspection.mod_count.saturating_sub(imported_mods)
    } else {
        0
    };
    Ok(ImportedLocalPack {
        instance_id,
        imported_files,
        imported_mods,
        unresolved_remote_files,
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
    let provided_by_loader = [
        "minecraft",
        "java",
        "fabricloader",
        "fabric-loader",
        "quilt_loader",
        "quilt-loader",
        "forge",
        "neoforge",
    ];
    let missing = inspection
        .dependencies
        .iter()
        .filter(|id| {
            !provided_by_loader.contains(&id.as_str()) && !installed_ids.contains(id.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        inspection.warnings.push(format!(
            "尚未检测到必需依赖：{}。安装后启动前请补齐。",
            missing.join(", ")
        ));
    }

    let mods_directory = PathBuf::from(root_path).join(".minecraft").join("mods");
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
    let file = fs::File::open(source).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| LauncherError::validation(format!("存档 ZIP 无效：{error}")))?;
    if archive.len() > 200_000 {
        return Err(LauncherError::validation("存档 ZIP 条目过多。"));
    }
    let mut roots = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        safe_relative_download_path(entry.name())?;
        let normalized = entry.name().replace('\\', "/");
        if normalized == "level.dat" {
            roots.push(String::new());
        } else if let Some(root) = normalized.strip_suffix("/level.dat") {
            roots.push(format!("{root}/"));
        }
    }
    roots.sort_by_key(String::len);
    roots.dedup();
    let root = roots
        .first()
        .ok_or_else(|| LauncherError::validation("ZIP 中未找到 level.dat。"))?
        .clone();
    let mut count = 0usize;
    let mut total = 0u64;
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
            return Err(LauncherError::validation("存档 ZIP 包含符号链接。"));
        }
        let normalized = entry.name().replace('\\', "/");
        let Some(relative) = normalized.strip_prefix(&root) else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        let output = destination.join(safe_relative_download_path(relative)?);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        total = total.saturating_add(entry.size());
        count += 1;
        if total > 20 * 1024 * 1024 * 1024 {
            return Err(LauncherError::validation("存档解压后超过 20 GB。"));
        }
        let mut file =
            fs::File::create(output).map_err(|error| LauncherError::storage(error.to_string()))?;
        std::io::copy(&mut entry, &mut file)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    Ok(count)
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
fn clone_instance(
    app: AppHandle,
    instance_id: i64,
    name: String,
) -> Result<Instance, LauncherError> {
    validate_instance_field(name.trim(), 64)?;
    let connection = open_database(&app)?;
    let (source_root, version, loader, loader_version, status): (String,String,String,Option<String>,String) = connection.query_row(
        "SELECT root_path,game_version,loader_type,loader_version,status FROM instances WHERE id=?1",
        [instance_id],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
    ).map_err(|_| LauncherError::validation("实例不存在。"))?;
    drop(connection);
    let mut cloned = create_instance_profile(app.clone(), name, version, loader)?;
    let source_game = PathBuf::from(source_root).join(".minecraft");
    let target_game = PathBuf::from(&cloned.root_path).join(".minecraft");
    for directory in [
        "mods",
        "config",
        "saves",
        "resourcepacks",
        "shaderpacks",
        "versions",
    ] {
        copy_directory_contents(&source_game.join(directory), &target_game.join(directory))?;
    }
    let connection = open_database(&app)?;
    connection
        .execute(
            "UPDATE instances SET loader_version=?1,status=?2,source='clone' WHERE id=?3",
            params![loader_version, status, cloned.id],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    cloned.status = status;
    cloned.source = "clone".into();
    Ok(cloned)
}

#[tauri::command]
fn delete_instance_to_backup(
    app: AppHandle,
    instance_id: i64,
) -> Result<RemovedContent, LauncherError> {
    let connection = open_database(&app)?;
    let root: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
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
    if root.exists() {
        fs::rename(&root, &destination)
            .map_err(|error| LauncherError::storage(format!("移动实例备份失败：{error}")))?;
    }
    connection
        .execute("DELETE FROM instances WHERE id=?1", [instance_id])
        .map_err(|error| LauncherError::storage(error.to_string()))?;
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
        Some("piston-meta.mojang.com") | Some("launchermeta.mojang.com")
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
    );
    if url.scheme() != "https" || !allowed_host {
        return Err(LauncherError::validation(
            "仅允许 Minecraft 官方 HTTPS 下载来源。",
        ));
    }
    Ok(url)
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

#[tauri::command]
fn cancel_active_downloads() {
    download_cancel_flag().store(true, Ordering::Release);
}

async fn send_download_request(
    client: &reqwest::Client,
    url: &reqwest::Url,
    resume_from: Option<u64>,
) -> Result<reqwest::Response, LauncherError> {
    let mut last_error = String::new();
    for attempt in 0..=3u32 {
        let mut request = client.get(url.clone());
        if let Some(offset) = resume_from.filter(|offset| *offset > 0) {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        match request.send().await {
            Ok(response)
                if !(response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || response.status().is_server_error()) =>
            {
                return Ok(response);
            }
            Ok(response) => {
                last_error = format!("HTTP {}", response.status());
            }
            Err(error) => {
                last_error = error.to_string();
            }
        }
        if attempt < 3 {
            tokio::time::sleep(Duration::from_millis(300 * 2u64.pow(attempt))).await;
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

async fn download_verified_file_with_progress(
    app: &AppHandle,
    instance_id: i64,
    url: &str,
    expected_sha1: &str,
    expected_size: Option<u64>,
    target: &std::path::Path,
    emit_file_progress: bool,
) -> Result<u64, LauncherError> {
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
    if first
        .as_ref()
        .is_err_and(|error| error.message == "下载文件大小或 SHA-1 校验失败。")
    {
        return download_verified_file_attempt(
            app,
            instance_id,
            url,
            expected_sha1,
            expected_size,
            target,
            emit_file_progress,
            false,
        )
        .await;
    }
    first
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
    if expected_sha1.len() != 40 || !expected_sha1.chars().all(|value| value.is_ascii_hexdigit()) {
        return Err(LauncherError::validation("下载文件 SHA-1 无效。"));
    }
    let url = validate_resource_url(url)?;
    if tokio::fs::try_exists(target)
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
    let cache_target = launcher_data_directory().ok().map(|root| {
        root.join("cache")
            .join("sha1")
            .join(expected_sha1.to_ascii_lowercase())
    });
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
    let started = std::time::Instant::now();
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
        let mut last_emit = std::time::Instant::now();
        loop {
            let next = tokio::time::timeout(Duration::from_secs(60), stream.next())
                .await
                .map_err(|_| {
                    LauncherError::storage(
                        "下载连续 60 秒没有收到数据。请检查网络后重试；已下载的部分会保留。",
                    )
                })?;
            let Some(chunk) = next else { break };
            if download_cancel_flag().load(Ordering::Acquire) {
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
            file.write_all(&chunk)
                .await
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            if last_emit.elapsed() >= Duration::from_millis(250) {
                let elapsed = started.elapsed().as_secs_f64().max(0.001);
                let speed =
                    (downloaded.saturating_sub(resume_from) as f64 / elapsed).round() as u64;
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
        if expected_size.is_some_and(|size| size != downloaded)
            || format!("{:x}", hasher.finalize()) != expected_sha1.to_ascii_lowercase()
        {
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
        .user_agent("SHLauncher/0.1")
        .pool_max_idle_per_host(32)
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
    let file =
        fs::File::open(archive_path).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| LauncherError::validation(format!("Native JAR 无效：{error}")))?;
    if archive.len() > 20_000 {
        return Err(LauncherError::validation("Native JAR 条目过多。"));
    }
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        if entry.is_dir() || entry.name().to_ascii_uppercase().starts_with("META-INF/") {
            continue;
        }
        let relative = safe_relative_download_path(entry.name())?;
        let output = target.join(relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        let mut output_file =
            fs::File::create(output).map_err(|error| LauncherError::storage(error.to_string()))?;
        std::io::copy(&mut entry, &mut output_file)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
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
    let allowed_library_count = libraries
        .iter()
        .filter(|library| rules_allow_windows(library))
        .count()
        .max(1);
    let mut processed_libraries = 0usize;
    for library in libraries
        .iter()
        .filter(|library| rules_allow_windows(library))
    {
        if let Some(artifact) = library.pointer("/downloads/artifact") {
            let path = artifact
                .get("path")
                .and_then(|value| value.as_str())
                .ok_or_else(|| LauncherError::storage("Library 缺少路径。"))?;
            let (url, sha1, size) = download_fields(artifact)?;
            total_downloaded += download_verified_file_with_progress(
                app,
                instance_id,
                url,
                sha1,
                Some(size),
                &game
                    .join("libraries")
                    .join(safe_relative_download_path(path)?),
                false,
            )
            .await?;
        }
        let Some(native_template) = library
            .pointer("/natives/windows")
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        let classifier = native_template.replace("${arch}", "64");
        let Some(native) = library.pointer(&format!("/downloads/classifiers/{classifier}")) else {
            continue;
        };
        let path = native
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| LauncherError::storage("Native library 缺少路径。"))?;
        let (url, sha1, size) = download_fields(native)?;
        let target = game
            .join("libraries")
            .join(safe_relative_download_path(path)?);
        total_downloaded += download_verified_file_with_progress(
            app,
            instance_id,
            url,
            sha1,
            Some(size),
            &target,
            false,
        )
        .await?;
        extract_native_jar(&target, &natives_directory)?;
        processed_libraries += 1;
        emit_install_percent(
            app,
            instance_id,
            20 + (processed_libraries as u64 * 25 / allowed_library_count as u64),
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
struct LaunchResult {
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
        for value in legacy.split_whitespace() {
            arguments.push(substitute_argument(value.to_string(), &replacements)?);
        }
    } else {
        return Err(LauncherError::storage("版本元数据缺少游戏参数。"));
    }
    Ok(arguments)
}

#[tauri::command]
async fn launch_instance(
    app: AppHandle,
    instance_id: i64,
    account_id: i64,
    java_path: String,
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
    auto_install_missing_mod_dependencies(&app, instance_id, &root_path, &loader).await?;
    validate_instance_mods(&root_path, &version, &loader)?;
    let (player_name, account_type, secret_ref): (String, String, Option<String>) = connection
        .query_row(
            "SELECT display_name, account_type, safe_secret_ref FROM accounts WHERE id=?1",
            [account_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| LauncherError::validation("请选择有效的账户。"))?;
    drop(connection);
    let (player_uuid, access_token, user_type, xuid) = if account_type == "MICROSOFT" {
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
        )
    } else {
        let digest = format!(
            "{:x}",
            Sha256::digest(format!("OfflinePlayer:{player_name}").as_bytes())
        );
        (
            digest[..32].to_string(),
            "0".to_string(),
            "legacy".to_string(),
            String::new(),
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
    let arguments = build_vanilla_launch_arguments(
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
    let url = validate_metadata_url(&url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("SHLauncher/0.1")
        .build()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| LauncherError::storage(format!("获取版本元数据失败：{error}")))?
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
    if bytes.len() > MAX_VERSION_JSON_BYTES || !verify_sha1(&bytes, &expected_sha1) {
        return Err(LauncherError::validation("版本元数据 SHA-1 校验失败。"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| LauncherError::storage(format!("版本元数据格式无效：{error}")))
}

#[tauri::command]
async fn fetch_version_manifest(include_snapshots: bool) -> Result<VersionManifest, LauncherError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("SHLauncher/0.1")
        .build()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let response = client
        .get(VERSION_MANIFEST_URL)
        .send()
        .await
        .map_err(|error| LauncherError::storage(format!("获取官方版本清单失败：{error}")))?
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
    let mut manifest = parse_version_manifest(&bytes)?;
    if !include_snapshots {
        manifest
            .versions
            .retain(|version| version.version_type == "release");
    }
    Ok(manifest)
}

fn chrono_like_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
fn unique_timestamp() -> u128 {
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
            exit_launcher,
            terminate_game,
            list_accounts,
            create_offline_account,
            login_microsoft,
            microsoft_login_available,
            remove_account,
            list_instances,
            create_vanilla_instance,
            create_instance_profile,
            rename_instance,
            clone_instance,
            delete_instance_to_backup,
            fetch_version_manifest,
            fetch_version_details,
            detect_java_runtimes,
            install_managed_java,
            get_settings,
            save_settings,
            preview_vanilla_install,
            install_vanilla_client,
            cancel_active_downloads,
            launch_instance,
            list_loader_versions,
            install_profile_loader,
            install_java_loader,
            inspect_mod_jar,
            inspect_modpack,
            search_modrinth_projects,
            install_modrinth_mod,
            check_mod_updates,
            update_modrinth_mod,
            install_modrinth_modpack,
            import_modrinth_pack,
            import_local_pack,
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let connection = Connection::open_in_memory().unwrap();
        run_migrations(&connection).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM migrations WHERE version=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
    #[test]
    fn interrupted_downloads_become_retryable_on_startup() {
        let connection = Connection::open_in_memory().unwrap();
        run_migrations(&connection).unwrap();
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
}
