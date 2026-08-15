//! 一键免费联机 V1：基于 e4mc（公开联机 Mod）作为受管理内容。
//! 不自行实现 NAT/STUN/TURN/P2P，只负责精确安装、启动与日志识别。

use crate::{
    chrono_like_timestamp, install_managed_mod, multiplayer_launch, open_database, stop_game,
    LauncherError,
};
use dashmap::DashMap;
use regex::Regex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

const E4MC_MODRINTH_PROJECT_ID: &str = "qANg5Jrr";

static ROOM_STATES: OnceLock<DashMap<i64, RoomInfo>> = OnceLock::new();
static ROOM_CANCELS: OnceLock<DashMap<i64, CancellationToken>> = OnceLock::new();

fn room_states() -> &'static DashMap<i64, RoomInfo> {
    ROOM_STATES.get_or_init(DashMap::new)
}

fn room_cancels() -> &'static DashMap<i64, CancellationToken> {
    ROOM_CANCELS.get_or_init(DashMap::new)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomInfo {
    pub instance_id: i64,
    pub state: String,
    pub address: Option<String>,
}

fn lan_port_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)Local game hosted on port\s+(\d{1,5})").expect("valid lan regex")
    })
}

fn e4mc_address_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)e4mc(?:[ -]link)?\s*[:=]\s*([a-z0-9.-]+)").expect("valid e4mc regex")
    })
}

pub fn parse_lan_port(line: &str) -> Option<u16> {
    lan_port_pattern()
        .captures(line)?
        .get(1)?
        .as_str()
        .parse()
        .ok()
        .filter(|port| *port > 0)
}

pub fn parse_e4mc_address(line: &str) -> Option<String> {
    let address = e4mc_address_pattern()
        .captures(line)?
        .get(1)?
        .as_str()
        .trim()
        .to_string();
    (!address.is_empty()).then_some(address)
}

async fn ensure_e4mc(app: &AppHandle, instance_id: i64) -> Result<(), LauncherError> {
    let connection = open_database(app)?;
    let (root_path, game_version, loader): (String, String, String) = connection
        .query_row(
            "SELECT root_path, game_version, loader_type FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))?;
    let mods = PathBuf::from(&root_path).join(".minecraft").join("mods");
    let installed = std::fs::read_dir(&mods)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("e4mc")
        });
    if installed {
        return Ok(());
    }
    if loader == "vanilla" {
        return Err(LauncherError::validation(
            "原版实例暂不支持一键联机，请使用模组实例。",
        ));
    }
    if game_version != "1.20.1" && game_version != "1.20.4" && game_version != "1.21.1" {
        return Err(LauncherError::validation(
            "一键联机暂支持 Minecraft 1.20.1 / 1.20.4 / 1.21.1。",
        ));
    }
    // 精确 provider identity，禁止模糊搜索。
    let item =
        install_managed_mod(app.clone(), instance_id, E4MC_MODRINTH_PROJECT_ID.into()).await?;
    let installed_path = mods.join(&item.file_name);
    connection.execute(
        "INSERT INTO managed_content(id, instance_id, kind, provider, project_id, version_id, file_sha1, file_sha256, installed_path, installed_by_launcher, created_at)
         VALUES(?1, ?2, 'MULTIPLAYER_HELPER', 'modrinth', ?3, NULL, NULL, ?4, ?5, 1, ?6)
         ON CONFLICT(id) DO UPDATE SET file_sha256=excluded.file_sha256, installed_path=excluded.installed_path",
        rusqlite::params![
            format!("e4mc-{instance_id}"),
            instance_id,
            E4MC_MODRINTH_PROJECT_ID,
            item.hash,
            installed_path.to_string_lossy(),
            chrono_like_timestamp()
        ],
    )
    .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}

fn watch_game_log(log_path: String, instance_id: i64, app: AppHandle) {
    let cancel = room_cancels()
        .entry(instance_id)
        .or_default()
        .clone();
    let _ = std::thread::Builder::new()
        .name(format!("sh-multiplayer-watch-{instance_id}"))
        .spawn(move || {
            let mut file = std::fs::File::open(&log_path).ok();
            use std::io::{BufRead, BufReader, Seek, SeekFrom};
            if let Some(file) = file.as_mut() {
                let _ = file.seek(SeekFrom::End(0));
            }
            let mut reader = file.map(BufReader::new);
            loop {
                if cancel.is_cancelled() {
                    break;
                }
                let mut line = String::new();
                let mut advanced = false;
                if let Some(reader) = reader.as_mut() {
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                        Ok(_) => {
                            advanced = true;
                        }
                        Err(_) => break,
                    }
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                if !advanced {
                    continue;
                }
                let mut update = None;
                if let Some(address) = parse_e4mc_address(&line) {
                    update = Some(RoomInfo {
                        instance_id,
                        state: "READY".into(),
                        address: Some(address),
                    });
                } else if let Some(port) = parse_lan_port(&line) {
                    update = Some(RoomInfo {
                        instance_id,
                        state: "READY".into(),
                        address: Some(format!("localhost:{port}")),
                    });
                }
                if let Some(info) = update {
                    room_states().insert(instance_id, info.clone());
                    let _ = app.emit("multiplayer-state", &info);
                }
            }
            room_states().insert(
                instance_id,
                RoomInfo {
                    instance_id,
                    state: "CLOSED".into(),
                    address: None,
                },
            );
            let _ = app.emit(
                "multiplayer-state",
                RoomInfo {
                    instance_id,
                    state: "CLOSED".into(),
                    address: None,
                },
            );
        });
}

#[tauri::command]
pub async fn multiplayer_prepare(
    app: AppHandle,
    instance_id: i64,
) -> Result<String, LauncherError> {
    ensure_e4mc(&app, instance_id).await?;
    Ok("联机组件已就绪，开始游戏后进入世界选择“对局域网开放”即可获得邀请地址。".into())
}

#[tauri::command]
pub async fn multiplayer_start(
    app: AppHandle,
    instance_id: i64,
    account_id: i64,
    java_path: String,
) -> Result<RoomInfo, LauncherError> {
    ensure_e4mc(&app, instance_id).await?;
    room_states().insert(
        instance_id,
        RoomInfo {
            instance_id,
            state: "PREPARING".into(),
            address: None,
        },
    );
    let launched = multiplayer_launch(app.clone(), instance_id, account_id, java_path).await?;
    room_states().insert(
        instance_id,
        RoomInfo {
            instance_id,
            state: "WAITING_FOR_LAN".into(),
            address: None,
        },
    );
    watch_game_log(launched.log_path, instance_id, app.clone());
    Ok(room_states()
        .get(&instance_id)
        .map(|entry| entry.clone())
        .unwrap_or(RoomInfo {
            instance_id,
            state: "WAITING_FOR_LAN".into(),
            address: None,
        }))
}

#[tauri::command]
pub async fn multiplayer_stop(app: AppHandle, instance_id: i64) -> Result<RoomInfo, LauncherError> {
    if let Some(cancel) = room_cancels().get(&instance_id) {
        cancel.cancel();
    }
    room_cancels().remove(&instance_id);
    let _ = stop_game(instance_id);
    let info = RoomInfo {
        instance_id,
        state: "STOPPED".into(),
        address: None,
    };
    room_states().insert(instance_id, info.clone());
    let _ = app.emit("multiplayer-state", &info);
    Ok(info)
}

#[tauri::command]
pub fn multiplayer_state(instance_id: i64) -> RoomInfo {
    room_states()
        .get(&instance_id)
        .map(|entry| entry.clone())
        .unwrap_or(RoomInfo {
            instance_id,
            state: "IDLE".into(),
            address: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vanilla_lan_port() {
        assert_eq!(
            parse_lan_port("[10:00:00] [Server thread/INFO]: Local game hosted on port 52913"),
            Some(52913)
        );
        assert_eq!(parse_lan_port("no lan here"), None);
    }

    #[test]
    fn parses_e4mc_endpoint_lines() {
        assert_eq!(
            parse_e4mc_address("[e4mc] e4mc link: play.example.e4mc.link"),
            Some("play.example.e4mc.link".into())
        );
        assert_eq!(
            parse_e4mc_address("e4mc-link: abc123.e4mc.link"),
            Some("abc123.e4mc.link".into())
        );
        assert_eq!(parse_e4mc_address("normal log line"), None);
    }
}
