use super::{chrono_like_timestamp, launcher_data_directory, open_database, LauncherError};
use serde::Serialize;
use std::{fs, path::PathBuf, time::UNIX_EPOCH};
use tauri::AppHandle;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadJobView {
    id: i64,
    source_url: String,
    target_path: String,
    progress_bytes: i64,
    total_bytes: Option<i64>,
    retry_count: i64,
    status: String,
    error: Option<String>,
    recovery_action: Option<String>,
    expected_hash: Option<String>,
    created_at: String,
    started_at: Option<String>,
    updated_at: Option<String>,
    bytes_per_second: i64,
    eta_seconds: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CrashReportView {
    id: i64,
    instance_id: i64,
    occurred_at: String,
    exit_code: Option<i64>,
    log_path: String,
    suspected_cause: String,
    confidence: String,
    suggestion: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GameLogView {
    instance_id: i64,
    instance_name: String,
    file_name: String,
    size: u64,
    modified_at: u64,
}

#[tauri::command]
pub(crate) fn list_game_logs(app: AppHandle) -> Result<Vec<GameLogView>, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection
        .prepare("SELECT id,name,root_path FROM instances")
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut logs = Vec::new();
    for row in rows {
        let (instance_id, instance_name, root) =
            row.map_err(|error| LauncherError::storage(error.to_string()))?;
        let directory = PathBuf::from(root).join(".minecraft").join("logs");
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !path.is_file() || !(file_name.ends_with(".log") || file_name.ends_with(".txt")) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            logs.push(GameLogView {
                instance_id,
                instance_name: instance_name.clone(),
                file_name: file_name.to_string(),
                size: metadata.len(),
                modified_at: metadata
                    .modified()
                    .ok()
                    .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                    .map(|value| value.as_secs())
                    .unwrap_or(0),
            });
        }
    }
    logs.sort_by_key(|item| std::cmp::Reverse(item.modified_at));
    logs.truncate(40);
    Ok(logs)
}

fn redact_log_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if lower.contains("access_token")
                || lower.contains("refresh_token")
                || lower.contains("authorization: bearer")
                || lower.contains("sessionid")
            {
                "[已隐藏可能包含登录凭据的日志行]".to_string()
            } else {
                redact_path(line.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tauri::command]
pub(crate) fn read_game_log(
    app: AppHandle,
    instance_id: i64,
    file_name: String,
    level: Option<String>,
    query: Option<String>,
) -> Result<String, LauncherError> {
    if file_name.is_empty()
        || file_name.contains(['/', '\\'])
        || !(file_name.ends_with(".log") || file_name.ends_with(".txt"))
    {
        return Err(LauncherError::validation("日志文件名无效。"));
    }
    let connection = open_database(&app)?;
    let root: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))?;
    let path = PathBuf::from(root)
        .join(".minecraft")
        .join("logs")
        .join(file_name);
    let bytes =
        fs::read(path).map_err(|error| LauncherError::storage(format!("无法读取日志：{error}")))?;
    let start = bytes.len().saturating_sub(512 * 1024);
    let text = String::from_utf8_lossy(&bytes[start..]);
    let level = level.unwrap_or_default().to_ascii_lowercase();
    let query = query.unwrap_or_default().to_ascii_lowercase();
    let filtered = text
        .lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            (level.is_empty() || level == "all" || lower.contains(&format!("[{}]", level)))
                && (query.is_empty() || lower.contains(&query))
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(redact_log_text(&filtered))
}

#[tauri::command]
pub(crate) fn list_download_jobs(app: AppHandle) -> Result<Vec<DownloadJobView>, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection.prepare("SELECT id,source_url,target_path,progress_bytes,total_bytes,retry_count,status,error,recovery_action,expected_hash,created_at,started_at,updated_at,bytes_per_second,eta_seconds FROM download_jobs ORDER BY id DESC LIMIT 100").map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok(DownloadJobView {
                id: row.get(0)?,
                source_url: row.get(1)?,
                target_path: row.get(2)?,
                progress_bytes: row.get(3)?,
                total_bytes: row.get(4)?,
                retry_count: row.get(5)?,
                status: row.get(6)?,
                error: row.get(7)?,
                recovery_action: row.get(8)?,
                expected_hash: row.get(9)?,
                created_at: row.get(10)?,
                started_at: row.get(11)?,
                updated_at: row.get(12)?,
                bytes_per_second: row.get(13)?,
                eta_seconds: row.get(14)?,
            })
        })
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| LauncherError::storage(error.to_string()))
}

#[tauri::command]
pub(crate) fn list_crash_reports(app: AppHandle) -> Result<Vec<CrashReportView>, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection.prepare("SELECT id,instance_id,occurred_at,exit_code,log_path,suspected_cause,confidence,suggestion FROM crash_reports ORDER BY id DESC LIMIT 50").map_err(|error| LauncherError::storage(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok(CrashReportView {
                id: row.get(0)?,
                instance_id: row.get(1)?,
                occurred_at: row.get(2)?,
                exit_code: row.get(3)?,
                log_path: redact_path(row.get::<_, String>(4)?),
                suspected_cause: row.get(5)?,
                confidence: row.get(6)?,
                suggestion: row.get(7)?,
            })
        })
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| LauncherError::storage(error.to_string()))
}

fn redact_path(value: String) -> String {
    let normalized = value.replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    if let Some(start) = lower.find("c:\\users\\") {
        let rest = &normalized[start + 9..];
        if let Some(separator) = rest.find('\\') {
            return format!(
                "{}%USERPROFILE%\\{}",
                &normalized[..start],
                &rest[separator + 1..]
            );
        }
    }
    normalized
}

#[tauri::command]
pub(crate) fn export_diagnostic_report(app: AppHandle) -> Result<String, LauncherError> {
    let connection = open_database(&app)?;
    let instance_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM instances", [], |row| row.get(0))
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let failed_downloads: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM download_jobs WHERE status='failed'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let crash_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM crash_reports", [], |row| row.get(0))
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let java = crate::detect_java_runtimes_cached(&app).into_iter().map(|runtime| serde_json::json!({"vendor":runtime.vendor,"version":runtime.version,"majorVersion":runtime.major_version,"architecture":runtime.architecture,"path":redact_path(runtime.path)})).collect::<Vec<_>>();
    let report = serde_json::json!({"generatedAt":chrono_like_timestamp(),"launcherVersion":"0.1.0","platform":"windows-x64","dataDirectory":"D:\\MinecraftLauncherData","instanceCount":instance_count,"failedDownloadCount":failed_downloads,"crashCount":crash_count,"javaRuntimes":java,"note":"账户名称、凭据和访问令牌未包含在报告中。"});
    let directory = launcher_data_directory()?.join("reports");
    std::fs::create_dir_all(&directory)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let path = directory.join(format!("diagnostic-{}.json", chrono_like_timestamp()));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report)
            .map_err(|error| LauncherError::storage(error.to_string()))?,
    )
    .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(path.to_string_lossy().to_string())
}
