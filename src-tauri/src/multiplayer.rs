//! 一键免费联机：基于 e4mc（Modrinth qANg5Jrr）作为受管理 Provider。
//!
//! 原则：
//! - 不自行实现 NAT / STUN / TURN / ICE / P2P / QUIC 隧道栈；
//! - 只负责精确身份安装、会话生命周期、状态机与真实日志识别；
//! - 只有可信的 `*.e4mc.link` 公网域名才进入 READY，localhost 永远不会成为邀请地址。

use crate::{
    chrono_like_timestamp, download_verified_file, fetch_modrinth_json, inspect_jar_identity,
    install_mod_from_cache, jar_supports_game_version, launcher_data_directory,
    multiplayer_join_launch, multiplayer_launch, open_database, sha256_file_sync, stop_game,
    validate_resource_url, LauncherError,
};
use dashmap::DashMap;
use regex::Regex;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::future::Future;
use std::io::{self, Read, Write};
use std::net::{
    IpAddr, Shutdown, SocketAddr, TcpListener as StdTcpListener, TcpStream as StdTcpStream,
    ToSocketAddrs,
};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const E4MC_PROVIDER_ID: &str = "e4mc";
pub const E4MC_MODRINTH_PROJECT_ID: &str = "qANg5Jrr";
pub const E4MC_PUBLIC_SUFFIX: &str = ".e4mc.link";
pub const E4MC_ADDRESS_MAX_LEN: usize = 253;
/// LAN 开放后仍拿不到公网地址的软提示窗口（秒）。只提示，不自动失败、不自动关闭。
pub const PUBLIC_ENDPOINT_NOTICE_SECS: u64 = 60;

pub const ERR_HELPER_INSTALL_FAILED: &str = "HELPER_INSTALL_FAILED";
pub const ERR_HELPER_INCOMPATIBLE: &str = "HELPER_INCOMPATIBLE";
pub const ERR_GAME_START_FAILED: &str = "GAME_START_FAILED";
/// §35 规定“等待用户开放局域网”不设自动超时，此码保留用于诊断分类，不主动触发。
#[allow(dead_code)]
pub const ERR_WAITING_FOR_LAN_TIMEOUT: &str = "WAITING_FOR_LAN_TIMEOUT";
pub const ERR_TUNNEL_CONNECT_FAILED: &str = "TUNNEL_CONNECT_FAILED";
pub const ERR_PUBLIC_ADDRESS_NOT_FOUND: &str = "PUBLIC_ADDRESS_NOT_FOUND";
pub const ERR_PROVIDER_UNAVAILABLE: &str = "PROVIDER_UNAVAILABLE";
pub const ERR_NETWORK_ERROR: &str = "NETWORK_ERROR";
pub const ERR_SESSION_CLOSED: &str = "SESSION_CLOSED";

static SESSIONS: OnceLock<Arc<DashMap<String, MultiplayerSession>>> = OnceLock::new();
static SESSION_CANCELS: OnceLock<DashMap<String, CancellationToken>> = OnceLock::new();
static INSTANCE_SESSIONS: OnceLock<DashMap<i64, String>> = OnceLock::new();
static E4MC: OnceLock<E4mcProvider> = OnceLock::new();

fn sessions_map() -> &'static DashMap<String, MultiplayerSession> {
    SESSIONS.get_or_init(|| Arc::new(DashMap::new())).as_ref()
}

fn sessions_handle() -> Arc<DashMap<String, MultiplayerSession>> {
    Arc::clone(SESSIONS.get_or_init(|| Arc::new(DashMap::new())))
}

fn session_cancels_map() -> &'static DashMap<String, CancellationToken> {
    SESSION_CANCELS.get_or_init(DashMap::new)
}

fn instance_sessions_map() -> &'static DashMap<i64, String> {
    INSTANCE_SESSIONS.get_or_init(DashMap::new)
}

static JOIN_SHIMS: OnceLock<DashMap<i64, JoinRelayShim>> = OnceLock::new();

fn join_shims_map() -> &'static DashMap<i64, JoinRelayShim> {
    JOIN_SHIMS.get_or_init(DashMap::new)
}

pub fn provider() -> &'static E4mcProvider {
    E4MC.get_or_init(|| E4mcProvider)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MultiplayerState {
    Idle,
    Preparing,
    InstallingHelper,
    GameStarting,
    WaitingForLan,
    LanOpened,
    WaitingForTunnel,
    Ready,
    Reconnecting,
    Stopping,
    Closed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomInfo {
    pub session_id: Option<String>,
    pub instance_id: i64,
    pub state: MultiplayerState,
    pub lan_port: Option<u16>,
    pub public_address: Option<String>,
    pub provider: Option<String>,
    pub helper_version: Option<String>,
    pub error_code: Option<String>,
    pub user_message: Option<String>,
    pub technical_message: Option<String>,
    pub started_at: Option<i64>,
    pub reconnect_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderEvent {
    LanOpened(u16),
    TunnelConnecting,
    PublicAddressReady(String),
    RelayConnected(String),
    RelayDisconnected,
    ProviderError(String),
}

#[derive(Debug, Clone)]
pub struct ResolvedProviderVersion {
    pub version_id: String,
    pub version_number: String,
    pub file_name: String,
    pub file_url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedMultiplayerHelper {
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
    pub version_number: Option<String>,
    pub file_sha256: String,
    pub installed_path: PathBuf,
    pub installed_by_launcher: bool,
    pub game_version: String,
    pub loader: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperStatus {
    Ready(ManagedMultiplayerHelper),
    /// 用户自装且兼容的 e4mc：复用但绝不接管文件所有权。
    ReadyUserJar(ManagedMultiplayerHelper),
    NeedsInstall(String),
    UserConflict(Vec<String>),
}

#[derive(Debug, Clone)]
struct SessionEvent {
    at: i64,
    kind: String,
}

#[derive(Debug, Clone)]
pub struct MultiplayerSession {
    pub id: String,
    pub instance_id: i64,
    pub provider: String,
    pub game_version: String,
    pub loader: String,
    pub state: MultiplayerState,
    pub lan_port: Option<u16>,
    pub public_address: Option<String>,
    pub helper_version: Option<String>,
    pub error_code: Option<String>,
    pub user_message: Option<String>,
    pub technical_message: Option<String>,
    pub started_at: i64,
    pub game_pid: Option<u32>,
    pub reconnect_count: u32,
    events: Vec<SessionEvent>,
    lan_opened_at: Option<Instant>,
    lan_notice_sent: bool,
}

impl MultiplayerSession {
    fn new(
        id: String,
        instance_id: i64,
        game_version: String,
        loader: String,
        started_at: i64,
    ) -> Self {
        Self {
            id,
            instance_id,
            provider: E4MC_PROVIDER_ID.to_string(),
            game_version,
            loader,
            state: MultiplayerState::Preparing,
            lan_port: None,
            public_address: None,
            helper_version: None,
            error_code: None,
            user_message: None,
            technical_message: None,
            started_at,
            game_pid: None,
            reconnect_count: 0,
            events: Vec::new(),
            lan_opened_at: None,
            lan_notice_sent: false,
        }
    }

    fn record_event(&mut self, kind: &str) {
        self.events.push(SessionEvent {
            at: chrono_like_timestamp().parse().unwrap_or(0),
            kind: kind.to_string(),
        });
        if self.events.len() > 200 {
            let overflow = self.events.len() - 200;
            self.events.drain(0..overflow);
        }
    }
}

/// Provider 抽象：当前只实现 e4mc，未来可扩 Playit / DirectIPv6，但本轮不开发 fallback。
pub trait MultiplayerProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn project_id(&self) -> &'static str;
    fn public_suffixes(&self) -> &'static [&'static str];
    fn accepts_public_address(&self, value: &str) -> bool;
    fn resolve_version<'a>(
        &'a self,
        game_version: &'a str,
        loader: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedProviderVersion, LauncherError>> + Send + 'a>>;
    fn parse_log_line(&self, line: &str) -> Option<ProviderEvent>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct E4mcProvider;

impl MultiplayerProvider for E4mcProvider {
    fn id(&self) -> &'static str {
        E4MC_PROVIDER_ID
    }

    fn project_id(&self) -> &'static str {
        E4MC_MODRINTH_PROJECT_ID
    }

    fn public_suffixes(&self) -> &'static [&'static str] {
        &[E4MC_PUBLIC_SUFFIX]
    }

    fn accepts_public_address(&self, value: &str) -> bool {
        validate_public_address_with_suffixes(value, self.public_suffixes())
    }

    fn resolve_version<'a>(
        &'a self,
        game_version: &'a str,
        loader: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedProviderVersion, LauncherError>> + Send + 'a>>
    {
        Box::pin(resolve_e4mc_version(game_version, loader))
    }

    fn parse_log_line(&self, line: &str) -> Option<ProviderEvent> {
        parse_e4mc_log_line(line)
    }
}

/// 严格校验 e4mc 公网地址：只接受 `label.e4mc.link`，拒绝端口、路径、反斜杠与非 ASCII。
pub fn validate_e4mc_public_address(value: &str) -> bool {
    validate_public_address_with_suffixes(value, &[E4MC_PUBLIC_SUFFIX])
}

fn validate_public_address_with_suffixes(value: &str, suffixes: &[&str]) -> bool {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .trim_end_matches('.')
        .to_string();
    if normalized.is_empty()
        || normalized.len() > E4MC_ADDRESS_MAX_LEN
        || !normalized.is_ascii()
        || normalized.contains('/')
        || normalized.contains('\\')
        || normalized.contains(':')
        || normalized.contains(char::is_whitespace)
    {
        return false;
    }
    let Some(label) = suffixes
        .iter()
        .find_map(|suffix| normalized.strip_suffix(suffix))
    else {
        return false;
    };
    if label.is_empty() || label.len() > 200 {
        return false;
    }
    label
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !label.starts_with(['.', '-'])
        && !label.ends_with('.')
        && !label.ends_with('-')
        && !label.contains("..")
        && label.chars().any(|c| c.is_ascii_alphanumeric())
}

fn lan_port_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(?i)local game hosted on port\s+(\d{1,5})",
            r"已开启本地局域网服务器[，,]\s*端口[:：]?\s*(\d{1,5})",
            r"已将本地游戏在端口\s*(\d{1,5})\s*上开放",
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("valid lan regex"))
        .collect()
    })
}

fn domain_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(?i)domain assigned[:：]\s*([^\s\[\]]+)",
            r"(?i)local game hosted on domain\s*\[([^\]]+)\]",
            r"将本地游戏托管在域名\s*\[([^\]]+)\]",
            // 旧协议 fallback：仅兜底，仍然要过严格地址校验。
            r"(?i)e4mc(?:[ -]?link)?\s*[:=]\s*([^\s\[\]]+)",
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("valid domain regex"))
        .collect()
    })
}

fn relay_connected_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)using relay\s+(\S+)").expect("valid relay regex"))
}

fn tunnel_markers() -> &'static [&'static str] {
    &[
        "broker req:",
        "broker resp:",
        "relaymap req:",
        "relaymap resp:",
        "control channel open:",
        "probing capabilities",
        "control channel write complete",
        "notified server of our ticket",
        "starting dialtoneambientsession",
    ]
}

/// 日志解析：所有 message 文本均取自上游 e4mc 源码的 logger 调用（见 multiplayer_fixtures/README.md）。
/// 未知行一律返回 None，绝不改变状态。
fn parse_e4mc_log_line(line: &str) -> Option<ProviderEvent> {
    if let Some(port) = lan_port_patterns().iter().find_map(|pattern| {
        pattern
            .captures(line)
            .and_then(|captures| captures.get(1))
            .and_then(|value| value.as_str().parse::<u16>().ok())
            .filter(|port| *port > 0)
    }) {
        return Some(ProviderEvent::LanOpened(port));
    }
    if let Some(domain) = domain_patterns().iter().find_map(|pattern| {
        pattern
            .captures(line)
            .and_then(|captures| captures.get(1))
            .map(|value| value.as_str().trim().to_string())
            .filter(|value| !value.is_empty())
    }) {
        return Some(ProviderEvent::PublicAddressReady(domain));
    }
    let lower = line.to_ascii_lowercase();
    if lower.contains("error in e4mc")
        || lower.contains("an error occurred in e4mc")
        || lower.contains("motw check failed")
        || lower.contains("poison pill active")
    {
        let detail: String = line.trim().chars().take(300).collect();
        return Some(ProviderEvent::ProviderError(detail));
    }
    if lower.contains("no longer publicly hosted")
        || lower.contains("not publicly hosted")
        || line.contains("不再公开托管本地游戏")
    {
        return Some(ProviderEvent::RelayDisconnected);
    }
    if let Some(captures) = relay_connected_pattern().captures(line) {
        let relay = captures
            .get(1)
            .map(|value| value.as_str().trim().to_string())
            .unwrap_or_default();
        return Some(ProviderEvent::RelayConnected(relay));
    }
    if tunnel_markers().iter().any(|marker| lower.contains(marker)) {
        return Some(ProviderEvent::TunnelConnecting);
    }
    None
}

/// 状态迁移：只有明确的 Provider 事件才改变状态；公网地址必须过严格校验。
/// 返回有变化时的最新快照，供事件推送使用。
fn apply_event(session: &mut MultiplayerSession, event: &ProviderEvent) -> Option<RoomInfo> {
    match event {
        ProviderEvent::LanOpened(port) => {
            if matches!(
                session.state,
                MultiplayerState::WaitingForLan
                    | MultiplayerState::WaitingForTunnel
                    | MultiplayerState::LanOpened
                    | MultiplayerState::Reconnecting
                    | MultiplayerState::Error
            ) && (session.lan_port != Some(*port)
                || session.state != MultiplayerState::LanOpened)
            {
                session.lan_port = Some(*port);
                session.state = MultiplayerState::LanOpened;
                session.lan_opened_at = Some(Instant::now());
                session.lan_notice_sent = false;
                session.record_event("LAN_OPENED");
                return Some(snapshot(session));
            }
        }
        ProviderEvent::PublicAddressReady(candidate) => {
            if !provider().accepts_public_address(candidate) {
                return None;
            }
            if matches!(
                session.state,
                MultiplayerState::Closed | MultiplayerState::Stopping
            ) {
                return None;
            }
            if session.public_address.as_deref() == Some(candidate.as_str()) {
                return None;
            }
            session.public_address = Some(candidate.clone());
            session.state = MultiplayerState::Ready;
            session.error_code = None;
            session.user_message = None;
            session.technical_message = None;
            session.lan_notice_sent = true;
            session.record_event("PUBLIC_ADDRESS_READY");
            return Some(snapshot(session));
        }
        ProviderEvent::TunnelConnecting => {
            if matches!(
                session.state,
                MultiplayerState::WaitingForLan
                    | MultiplayerState::LanOpened
                    | MultiplayerState::Reconnecting
            ) {
                session.state = MultiplayerState::WaitingForTunnel;
                session.record_event("TUNNEL_CONNECTING");
                return Some(snapshot(session));
            }
        }
        ProviderEvent::RelayConnected(_) => {
            if matches!(
                session.state,
                MultiplayerState::WaitingForLan
                    | MultiplayerState::LanOpened
                    | MultiplayerState::Reconnecting
            ) {
                session.state = MultiplayerState::WaitingForTunnel;
                session.record_event("RELAY_CONNECTED");
                return Some(snapshot(session));
            }
        }
        ProviderEvent::RelayDisconnected => {
            if session.state == MultiplayerState::Ready {
                session.state = MultiplayerState::Reconnecting;
                session.reconnect_count += 1;
                if session.reconnect_count >= 3 {
                    session.state = MultiplayerState::Error;
                    session.error_code = Some(ERR_TUNNEL_CONNECT_FAILED.to_string());
                    session.user_message = Some(
                        "联机连接多次中断，未能恢复。请结束联机后重试，或检查网络。".to_string(),
                    );
                    session.record_event("TUNNEL_CONNECT_FAILED");
                    return Some(snapshot(session));
                }
                session.user_message = Some(
                    "联机连接中断，正在等待恢复；若长时间未恢复，可结束联机后重试。".to_string(),
                );
                session.record_event("RELAY_DISCONNECTED");
                return Some(snapshot(session));
            }
            if matches!(
                session.state,
                MultiplayerState::WaitingForTunnel | MultiplayerState::LanOpened
            ) {
                session.reconnect_count += 1;
                if session.reconnect_count >= 3 {
                    session.state = MultiplayerState::Error;
                    session.error_code = Some(ERR_TUNNEL_CONNECT_FAILED.to_string());
                    session.user_message = Some(
                        "联机连接多次中断，未能恢复。请结束联机后重试，或检查网络。".to_string(),
                    );
                    session.record_event("TUNNEL_CONNECT_FAILED");
                    return Some(snapshot(session));
                }
                session.record_event("RELAY_DISCONNECTED");
            }
        }
        ProviderEvent::ProviderError(detail) => {
            if matches!(
                session.state,
                MultiplayerState::Closed | MultiplayerState::Stopping
            ) {
                return None;
            }
            session.state = MultiplayerState::Error;
            session.error_code = Some(ERR_PROVIDER_UNAVAILABLE.to_string());
            session.user_message =
                Some("e4mc 联机服务出现异常，暂时无法建立公网联机。".to_string());
            session.technical_message = Some(detail.clone());
            session.record_event("PROVIDER_ERROR");
            return Some(snapshot(session));
        }
    }
    None
}

/// LAN 开放后 60 秒仍无公网地址：只提示、不失败、不关闭。
fn apply_notice(session: &mut MultiplayerSession) -> Option<RoomInfo> {
    if session.lan_notice_sent {
        return None;
    }
    if !matches!(
        session.state,
        MultiplayerState::LanOpened | MultiplayerState::WaitingForTunnel
    ) {
        return None;
    }
    let elapsed = session.lan_opened_at.map(|instant| instant.elapsed());
    if !elapsed.is_some_and(|duration| duration >= Duration::from_secs(PUBLIC_ENDPOINT_NOTICE_SECS))
    {
        return None;
    }
    session.lan_notice_sent = true;
    session.error_code = Some(ERR_PUBLIC_ADDRESS_NOT_FOUND.to_string());
    session.user_message = Some(
        "世界已开放，但暂时还没拿到公网地址（可能网络较慢）。可继续等待，或稍后重试。".to_string(),
    );
    Some(snapshot(session))
}

fn snapshot(session: &MultiplayerSession) -> RoomInfo {
    RoomInfo {
        session_id: Some(session.id.clone()),
        instance_id: session.instance_id,
        state: session.state,
        lan_port: session.lan_port,
        public_address: session.public_address.clone(),
        provider: Some(session.provider.clone()),
        helper_version: session.helper_version.clone(),
        error_code: session.error_code.clone(),
        user_message: session.user_message.clone(),
        technical_message: session.technical_message.clone(),
        started_at: Some(session.started_at),
        reconnect_count: session.reconnect_count,
    }
}

fn transition(
    sessions: &DashMap<String, MultiplayerSession>,
    session_id: &str,
    event: &ProviderEvent,
) -> Option<RoomInfo> {
    sessions
        .get_mut(session_id)
        .and_then(|mut guard| apply_event(&mut guard, event))
}

/// 日志尾随：等待文件出现、处理 truncate/rotate/recreate、按字节读取并做 lossy UTF-8。
struct LogTailer {
    path: PathBuf,
    file: Option<std::fs::File>,
    offset: u64,
    buffer: Vec<u8>,
    poll: Duration,
}

impl LogTailer {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            file: None,
            offset: 0,
            buffer: Vec::new(),
            poll: Duration::from_millis(200),
        }
    }

    fn next_line(&mut self) -> Option<String> {
        use std::io::{Read, Seek, SeekFrom};
        if self.file.is_none() {
            match std::fs::File::open(&self.path) {
                Ok(opened) => {
                    self.file = Some(opened);
                    self.offset = 0;
                }
                Err(_) => {
                    std::thread::sleep(self.poll);
                    return None;
                }
            }
        }
        // 先消费缓冲区内已经完整的行：即使文件不再增长，也必须把已读内容全部交付，
        // 否则游戏停止写日志后，缓冲的最后几行（含 e4mc 域名）会被永远卡住。
        if let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=position).collect();
            let text = String::from_utf8_lossy(&line);
            return Some(text.trim_end_matches(['\r', '\n']).to_string());
        }
        match std::fs::metadata(&self.path) {
            Ok(meta) => {
                let len = meta.len();
                if len < self.offset {
                    // truncate 或日志被重新创建：从头再读。
                    self.file = None;
                    self.offset = 0;
                    self.buffer.clear();
                    return None;
                }
                if len == self.offset {
                    std::thread::sleep(self.poll);
                    return None;
                }
            }
            Err(_) => {
                // 日志被移动/删除：稍后重新打开。
                self.file = None;
                self.offset = 0;
                self.buffer.clear();
                std::thread::sleep(self.poll);
                return None;
            }
        }
        let want = 65_536usize;
        let mut chunk = vec![0u8; want];
        let file = self.file.as_mut().expect("file opened above");
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            self.file = None;
            self.offset = 0;
            self.buffer.clear();
            return None;
        }
        match file.read(&mut chunk) {
            Ok(0) => {
                std::thread::sleep(self.poll);
                None
            }
            Ok(read) => {
                self.offset += read as u64;
                self.buffer.extend_from_slice(&chunk[..read]);
                if self.buffer.len() > 4 * 1024 * 1024 {
                    self.buffer.clear();
                }
                None
            }
            Err(_) => {
                self.file = None;
                self.offset = 0;
                self.buffer.clear();
                None
            }
        }
    }
}

/// 日志监听线程核心：只处理属于本 session 的事件；session 被移除后立即失效。
fn spawn_log_watcher(
    session_id: String,
    sessions: Arc<DashMap<String, MultiplayerSession>>,
    log_path: String,
    cancel: CancellationToken,
    on_update: impl Fn(RoomInfo) + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name(format!("sh-multiplayer-watch-{session_id}"))
        .spawn(move || {
            eprintln!("[e2e-watch] watcher started session={session_id} log={log_path}");
            let mut tail = LogTailer::new(PathBuf::from(log_path));
            let mut last_notice_check = Instant::now();
            loop {
                if cancel.is_cancelled() {
                    break;
                }
                if let Some(line) = tail.next_line() {
                    if let Some(event) = provider().parse_log_line(&line) {
                        eprintln!(
                            "[e2e-watch] session={} event={event:?} line={}",
                            session_id,
                            line.chars().take(160).collect::<String>()
                        );
                        let update = transition(&sessions, &session_id, &event);
                        if let Some(info) = update {
                            eprintln!(
                                "[e2e-watch] session={} state={:?} address={:?}",
                                session_id, info.state, info.public_address
                            );
                            on_update(info);
                        }
                    }
                } else if last_notice_check.elapsed() >= Duration::from_secs(2) {
                    last_notice_check = Instant::now();
                    let notice = sessions
                        .get_mut(&session_id)
                        .and_then(|mut guard| apply_notice(&mut guard));
                    if let Some(info) = notice {
                        on_update(info);
                    }
                }
            }
        })
        .expect("spawn multiplayer log watcher")
}

fn watch_game_log(session_id: String, log_path: String, cancel: CancellationToken, app: AppHandle) {
    spawn_log_watcher(
        session_id,
        sessions_handle(),
        log_path,
        cancel,
        move |info| {
            let _ = app.emit("multiplayer-state", &info);
        },
    );
}

// ---------------------------------------------------------------------------
// 受管理 helper：身份识别、动态版本、安装与对账
// ---------------------------------------------------------------------------

fn instance_identity(app: &AppHandle, instance_id: i64) -> Result<(String, String), LauncherError> {
    let connection = open_database(app)?;
    connection
        .query_row(
            "SELECT game_version, loader_type FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))
}

fn instance_root(app: &AppHandle, instance_id: i64) -> Result<String, LauncherError> {
    let connection = open_database(app)?;
    connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))
}

fn is_vanilla_loader(loader: &str) -> bool {
    loader.eq_ignore_ascii_case("vanilla")
}

fn loader_compatible(instance_loader: &str, mod_loader: &str) -> bool {
    let instance_loader = instance_loader.to_ascii_lowercase();
    let mod_loader = mod_loader.to_ascii_lowercase();
    match instance_loader.as_str() {
        "fabric" => mod_loader == "fabric",
        "quilt" => matches!(mod_loader.as_str(), "fabric" | "quilt"),
        "forge" => mod_loader == "forge",
        "neoforge" => mod_loader == "neoforge",
        _ => false,
    }
}

/// 依据 managed_content / content_provenance 判断 helper 是否可信，绝不通过文件名猜测。
fn verify_managed_helper(
    connection: &rusqlite::Connection,
    instance_id: i64,
    game_version: &str,
    loader: &str,
    mods_dir: &Path,
) -> Result<HelperStatus, LauncherError> {
    let row: Option<(String, String, Option<String>, String, String, bool)> = connection
        .query_row(
            "SELECT project_id, version_id, version_number, file_sha256, installed_path, installed_by_launcher
             FROM managed_content
             WHERE instance_id=?1 AND kind='MULTIPLAYER_HELPER'",
            [instance_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get::<_, i64>(5)? != 0,
                ))
            },
        )
        .optional()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if let Some((
        project_id,
        version_id,
        version_number,
        file_sha256,
        installed_path,
        by_launcher,
    )) = row
    {
        if project_id != E4MC_MODRINTH_PROJECT_ID {
            return Ok(HelperStatus::NeedsInstall(
                "联机组件记录身份不一致，将重新安装。".to_string(),
            ));
        }
        let path = PathBuf::from(&installed_path);
        let hash_ok = path.is_file()
            && sha256_file_sync(&path)
                .map(|hash| hash.eq_ignore_ascii_case(&file_sha256))
                .unwrap_or(false);
        if hash_ok {
            return Ok(HelperStatus::Ready(ManagedMultiplayerHelper {
                provider: E4MC_PROVIDER_ID.to_string(),
                project_id,
                version_id,
                version_number,
                file_sha256,
                installed_path: path,
                installed_by_launcher: by_launcher,
                game_version: game_version.to_string(),
                loader: loader.to_string(),
            }));
        }
        let reason = if path.is_file() {
            "联机组件文件已被修改，将重新安装。"
        } else {
            "联机组件文件缺失，将重新安装。"
        };
        return Ok(HelperStatus::NeedsInstall(reason.to_string()));
    }
    // 没有受管理记录：识别用户自装的 e4mc（按身份 / 哈希 / 加载器 / 游戏版本判断）。
    let mut conflicts = Vec::new();
    let mut adopt: Option<(PathBuf, String)> = None;
    if let Ok(entries) = std::fs::read_dir(mods_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .and_then(|value| value.to_str())
                .is_none_or(|value| !value.eq_ignore_ascii_case("jar"))
            {
                continue;
            }
            let Ok((mod_id, mod_loader)) = inspect_jar_identity(&path) else {
                continue;
            };
            if !mod_id.eq_ignore_ascii_case("e4mc") {
                continue;
            }
            if loader_compatible(loader, &mod_loader)
                && jar_supports_game_version(&path, game_version)
            {
                if let Ok(hash) = sha256_file_sync(&path) {
                    if adopt.is_none() {
                        adopt = Some((path, hash));
                    }
                }
            } else {
                conflicts.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    if let Some((path, hash)) = adopt {
        return Ok(HelperStatus::ReadyUserJar(ManagedMultiplayerHelper {
            provider: E4MC_PROVIDER_ID.to_string(),
            project_id: E4MC_MODRINTH_PROJECT_ID.to_string(),
            version_id: "user-managed".to_string(),
            version_number: None,
            file_sha256: hash,
            installed_path: path,
            installed_by_launcher: false,
            game_version: game_version.to_string(),
            loader: loader.to_string(),
        }));
    }
    if !conflicts.is_empty() {
        return Ok(HelperStatus::UserConflict(conflicts));
    }
    Ok(HelperStatus::NeedsInstall("尚未安装联机组件。".to_string()))
}

/// 只按当前游戏版本 + 加载器做严格匹配，禁止猜 slug、禁止放宽到错误加载器。
fn select_strict_version<'a>(
    versions: &'a [serde_json::Value],
    game_version: &str,
    loader: &str,
) -> Option<&'a serde_json::Value> {
    let loader = loader.to_ascii_lowercase();
    versions
        .iter()
        .filter(|version| {
            let loaders_ok = version
                .get("loaders")
                .and_then(|value| value.as_array())
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item.as_str()
                            .is_some_and(|name| name.eq_ignore_ascii_case(&loader))
                    })
                });
            let games_ok = version
                .get("game_versions")
                .and_then(|value| value.as_array())
                .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(game_version)));
            loaders_ok && games_ok
        })
        .max_by(|left, right| {
            let left_release =
                left.get("version_type").and_then(|value| value.as_str()) == Some("release");
            let right_release =
                right.get("version_type").and_then(|value| value.as_str()) == Some("release");
            let left_date = left
                .get("date_published")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let right_date = right
                .get("date_published")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let left_number = left
                .get("version_number")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let right_number = right
                .get("version_number")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            left_release
                .cmp(&right_release)
                .then(left_date.cmp(right_date))
                .then(left_number.cmp(right_number))
        })
}

fn incompatible_message(
    versions: &[serde_json::Value],
    game_version: &str,
    loader: &str,
) -> String {
    let mut loaders = std::collections::BTreeSet::new();
    let mut games = std::collections::BTreeSet::new();
    for version in versions {
        if let Some(items) = version.get("loaders").and_then(|value| value.as_array()) {
            for item in items.iter().filter_map(|value| value.as_str()) {
                loaders.insert(item.to_string());
            }
        }
        if let Some(items) = version
            .get("game_versions")
            .and_then(|value| value.as_array())
        {
            for item in items.iter().filter_map(|value| value.as_str()) {
                games.insert(item.to_string());
            }
        }
    }
    let loader_list = loaders
        .iter()
        .take(8)
        .cloned()
        .collect::<Vec<_>>()
        .join(" / ");
    let game_list = games
        .iter()
        .take(16)
        .cloned()
        .collect::<Vec<_>>()
        .join("、");
    let game_suffix = if games.len() > 16 { "…" } else { "" };
    format!(
        "当前 Minecraft {game_version} + {loader} 暂无可用 e4mc 版本。e4mc 支持加载器：{loader_list}；支持版本：{game_list}{game_suffix}。"
    )
}

async fn resolve_e4mc_version(
    game_version: &str,
    loader: &str,
) -> Result<ResolvedProviderVersion, LauncherError> {
    let url = reqwest::Url::parse(&format!(
        "https://api.modrinth.com/v2/project/{E4MC_MODRINTH_PROJECT_ID}/version"
    ))
    .map_err(|error| LauncherError::storage(error.to_string()))?;
    let value = fetch_modrinth_json(url).await?;
    let versions = value.as_array().cloned().unwrap_or_default();
    let selected = select_strict_version(&versions, game_version, loader).ok_or_else(|| {
        LauncherError::classified(
            ERR_HELPER_INCOMPATIBLE,
            incompatible_message(&versions, game_version, loader),
            false,
        )
    })?;
    parse_resolved_version(selected)
}

fn parse_resolved_version(
    version: &serde_json::Value,
) -> Result<ResolvedProviderVersion, LauncherError> {
    let version_id = version
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("Modrinth 版本缺少 id。"))?
        .to_string();
    let version_number = version
        .get("version_number")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let files = version
        .get("files")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LauncherError::storage("Modrinth 版本缺少文件列表。"))?;
    let is_jar = |file: &&serde_json::Value| {
        file.get("filename")
            .and_then(|value| value.as_str())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".jar"))
    };
    let file = files
        .iter()
        .filter(is_jar)
        .find(|file| file.get("primary").and_then(|value| value.as_bool()) == Some(true))
        .or_else(|| files.iter().find(is_jar))
        .ok_or_else(|| {
            LauncherError::classified(
                ERR_HELPER_INSTALL_FAILED,
                "e4mc 版本没有可安装的主文件。",
                false,
            )
        })?;
    let file_name = file
        .get("filename")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("Modrinth 文件缺少文件名。"))?
        .to_string();
    let file_url = file
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("Modrinth 文件缺少下载地址。"))?
        .to_string();
    validate_resource_url(&file_url).map_err(|error| {
        LauncherError::classified(ERR_HELPER_INSTALL_FAILED, error.error_message(), false)
    })?;
    let sha1 = file
        .pointer("/hashes/sha1")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("Modrinth 文件缺少 SHA-1。"))?
        .to_string();
    let size = file
        .get("size")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| LauncherError::storage("Modrinth 文件缺少大小。"))?;
    Ok(ResolvedProviderVersion {
        version_id,
        version_number,
        file_name,
        file_url,
        sha1,
        size,
    })
}

fn adopt_managed_helper(
    connection: &rusqlite::Connection,
    instance_id: i64,
    helper: &ManagedMultiplayerHelper,
) -> Result<(), LauncherError> {
    connection
        .execute(
            "INSERT INTO managed_content(id, instance_id, kind, provider, project_id, version_id, version_number, file_sha1, file_sha256, installed_path, installed_by_launcher, created_at)
             VALUES(?1, ?2, 'MULTIPLAYER_HELPER', 'modrinth', ?3, 'user-managed', NULL, NULL, ?4, ?5, 0, ?6)
             ON CONFLICT(id) DO UPDATE SET
                instance_id=excluded.instance_id,
                provider='modrinth',
                project_id=excluded.project_id,
                version_id=excluded.version_id,
                version_number=NULL,
                file_sha1=NULL,
                file_sha256=excluded.file_sha256,
                installed_path=excluded.installed_path,
                installed_by_launcher=0,
                created_at=excluded.created_at",
            rusqlite::params![
                format!("e4mc-{instance_id}"),
                instance_id,
                E4MC_MODRINTH_PROJECT_ID,
                helper.file_sha256,
                helper.installed_path.to_string_lossy(),
                chrono_like_timestamp()
            ],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}

/// 下载 → 校验 → 安装 → 写 provenance + managed_content（事务化，失败不留下半个 jar）。
async fn install_e4mc(
    app: &AppHandle,
    instance_id: i64,
    game_version: &str,
    loader: &str,
    mods_dir: &Path,
    token: &CancellationToken,
) -> Result<ManagedMultiplayerHelper, LauncherError> {
    if token.is_cancelled() {
        return Err(session_closed_error());
    }
    let resolved = provider()
        .resolve_version(game_version, loader)
        .await
        .map_err(|error| {
            if error.error_code() == ERR_HELPER_INCOMPATIBLE {
                error
            } else {
                LauncherError::classified(
                    ERR_NETWORK_ERROR,
                    format!("无法连接 e4mc 版本服务：{}", error.error_message()),
                    true,
                )
            }
        })?;
    let cache = launcher_data_directory()?
        .join("cache")
        .join("multiplayer")
        .join(format!("{}-{}", &resolved.sha1[..12], resolved.file_name));
    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    download_verified_file(
        app,
        instance_id,
        &resolved.file_url,
        &resolved.sha1,
        Some(resolved.size),
        &cache,
    )
    .await
    .map_err(|error| {
        LauncherError::classified(
            ERR_HELPER_INSTALL_FAILED,
            format!("联机组件下载失败：{}", error.error_message()),
            true,
        )
    })?;
    if token.is_cancelled() {
        return Err(session_closed_error());
    }
    let item = install_mod_from_cache(
        app.clone(),
        instance_id,
        cache.to_string_lossy().to_string(),
    )
    .map_err(|error| {
        LauncherError::classified(
            ERR_HELPER_INSTALL_FAILED,
            format!("联机组件安装失败：{}", error.error_message()),
            true,
        )
    })?;
    let installed_path = mods_dir.join(&item.file_name);
    let file_sha256 = sha256_file_sync(&installed_path).map_err(|error| {
        LauncherError::classified(
            ERR_HELPER_INSTALL_FAILED,
            format!("联机组件校验失败：{}", error.error_message()),
            true,
        )
    })?;
    let mut connection = open_database(app)?;
    let transaction = connection
        .transaction()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let metadata = {
        let mut metadata = item
            .metadata_json
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(object) = metadata.as_object_mut() {
            object.insert(
                "modrinthProjectId".to_string(),
                serde_json::Value::String(provider().project_id().to_string()),
            );
            object.insert(
                "modrinthVersionId".to_string(),
                serde_json::Value::String(resolved.version_id.clone()),
            );
        }
        serde_json::to_string(&metadata).unwrap_or_default()
    };
    let write_result = (|| -> rusqlite::Result<()> {
        transaction.execute(
            "INSERT INTO managed_content(id, instance_id, kind, provider, project_id, version_id, version_number, file_sha1, file_sha256, installed_path, installed_by_launcher, created_at)
             VALUES(?1, ?2, 'MULTIPLAYER_HELPER', 'modrinth', ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)
             ON CONFLICT(id) DO UPDATE SET
                instance_id=excluded.instance_id,
                provider='modrinth',
                project_id=excluded.project_id,
                version_id=excluded.version_id,
                version_number=excluded.version_number,
                file_sha1=excluded.file_sha1,
                file_sha256=excluded.file_sha256,
                installed_path=excluded.installed_path,
                installed_by_launcher=1,
                created_at=excluded.created_at",
            rusqlite::params![
                format!("e4mc-{instance_id}"),
                instance_id,
                E4MC_MODRINTH_PROJECT_ID,
                resolved.version_id,
                resolved.version_number,
                resolved.sha1,
                file_sha256,
                installed_path.to_string_lossy(),
                chrono_like_timestamp()
            ],
        )?;
        transaction.execute(
            "INSERT INTO content_provenance(content_id, provider, project_id, version_id, file_id, source_url, sha1, sha256, installed_at)
             VALUES(?1, 'modrinth', ?2, ?3, NULL, ?4, ?5, ?6, ?7)
             ON CONFLICT(content_id) DO UPDATE SET
                provider='modrinth',
                project_id=excluded.project_id,
                version_id=excluded.version_id,
                file_id=NULL,
                source_url=excluded.source_url,
                sha1=excluded.sha1,
                sha256=excluded.sha256,
                installed_at=excluded.installed_at",
            rusqlite::params![
                item.id,
                E4MC_MODRINTH_PROJECT_ID,
                resolved.version_id,
                resolved.file_url,
                resolved.sha1,
                file_sha256,
                chrono_like_timestamp()
            ],
        )?;
        transaction.execute(
            "UPDATE content_items SET source='modrinth', metadata_json=?1 WHERE id=?2",
            rusqlite::params![metadata, item.id],
        )?;
        Ok(())
    })();
    if let Err(error) = write_result {
        drop(transaction);
        return Err(LauncherError::storage(error.to_string()));
    }
    transaction
        .commit()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(ManagedMultiplayerHelper {
        provider: E4MC_PROVIDER_ID.to_string(),
        project_id: E4MC_MODRINTH_PROJECT_ID.to_string(),
        version_id: resolved.version_id,
        version_number: Some(resolved.version_number),
        file_sha256,
        installed_path,
        installed_by_launcher: true,
        game_version: game_version.to_string(),
        loader: loader.to_string(),
    })
}

async fn ensure_helper_for_session(
    app: &AppHandle,
    instance_id: i64,
    game_version: &str,
    loader: &str,
    session_id: &str,
    token: &CancellationToken,
) -> Result<ManagedMultiplayerHelper, LauncherError> {
    let connection = open_database(app)?;
    let mods_dir = PathBuf::from(instance_root(app, instance_id)?)
        .join(".minecraft")
        .join("mods");
    let status = verify_managed_helper(&connection, instance_id, game_version, loader, &mods_dir)?;
    drop(connection);
    match status {
        HelperStatus::Ready(helper) => Ok(helper),
        HelperStatus::ReadyUserJar(helper) => {
            let connection = open_database(app)?;
            adopt_managed_helper(&connection, instance_id, &helper)?;
            Ok(helper)
        }
        HelperStatus::UserConflict(files) => Err(conflict_error(&files)),
        HelperStatus::NeedsInstall(reason) => {
            mutate_session(app, session_id, |session| {
                session.state = MultiplayerState::InstallingHelper;
                session.user_message = Some(reason);
            });
            install_e4mc(app, instance_id, game_version, loader, &mods_dir, token).await
        }
    }
}

fn conflict_error(files: &[String]) -> LauncherError {
    LauncherError::classified(
        ERR_HELPER_INCOMPATIBLE,
        format!(
            "检测到你自行安装的 e4mc 与当前实例不兼容（{}）。请在模组页删除这些文件，或保留后自行管理联机；SH 启动器不会覆盖你的模组。",
            files.join("、")
        ),
        false,
    )
}

fn session_closed_error() -> LauncherError {
    LauncherError::classified(ERR_SESSION_CLOSED, "联机已取消或结束。", false)
}

fn vanilla_error() -> LauncherError {
    LauncherError::classified(
        ERR_HELPER_INCOMPATIBLE,
        "原版实例暂不支持一键联机。请使用 Fabric / Forge / NeoForge / Quilt 模组实例。",
        false,
    )
}

fn is_retryable(code: &str) -> bool {
    !matches!(code, ERR_HELPER_INCOMPATIBLE | ERR_SESSION_CLOSED)
}

fn mutate_session(
    app: &AppHandle,
    session_id: &str,
    update: impl FnOnce(&mut MultiplayerSession),
) -> Option<RoomInfo> {
    let mut guard = sessions_map().get_mut(session_id)?;
    update(&mut guard);
    let info = snapshot(&guard);
    drop(guard);
    let _ = app.emit("multiplayer-state", &info);
    Some(info)
}

fn fail_session(
    app: &AppHandle,
    session_id: &str,
    code: &str,
    user_message: &str,
    technical: Option<String>,
) -> LauncherError {
    let instance_id = sessions_map()
        .get(session_id)
        .map(|guard| guard.instance_id);
    if let Some(mut guard) = sessions_map().get_mut(session_id) {
        guard.state = MultiplayerState::Error;
        guard.error_code = Some(code.to_string());
        guard.user_message = Some(user_message.to_string());
        guard.technical_message = technical;
        let info = snapshot(&guard);
        drop(guard);
        let _ = app.emit("multiplayer-state", &info);
    }
    if let Some((_, cancel)) = session_cancels_map().remove(session_id) {
        cancel.cancel();
    }
    if let Some(instance_id) = instance_id {
        let points_at_self = instance_sessions_map()
            .get(&instance_id)
            .is_some_and(|entry| entry.value() == session_id);
        if points_at_self {
            instance_sessions_map().remove(&instance_id);
        }
    }
    if let Ok(connection) = open_database(app) {
        finalize_history(&connection, session_id, false, "error");
    }
    LauncherError::classified(code, user_message, is_retryable(code))
}

fn close_session_in(
    sessions: &DashMap<String, MultiplayerSession>,
    cancels: &DashMap<String, CancellationToken>,
    instance_map: &DashMap<i64, String>,
    connection: Option<&rusqlite::Connection>,
    session_id: &str,
    exit_reason: &str,
    user_message: &str,
) -> Option<RoomInfo> {
    if let Some((_, cancel)) = cancels.remove(session_id) {
        cancel.cancel();
    }
    let (_, session) = sessions.remove(session_id)?;
    let points_at_self = instance_map
        .get(&session.instance_id)
        .is_some_and(|entry| entry.value() == session_id);
    if points_at_self {
        instance_map.remove(&session.instance_id);
    }
    if let Some(connection) = connection {
        finalize_history(
            connection,
            session_id,
            session.public_address.is_some(),
            exit_reason,
        );
    }
    let mut info = snapshot(&session);
    info.state = MultiplayerState::Closed;
    info.public_address = None;
    info.user_message = Some(user_message.to_string());
    Some(info)
}

fn close_session(
    app: &AppHandle,
    session_id: &str,
    exit_reason: &str,
    user_message: &str,
) -> Option<RoomInfo> {
    let connection = open_database(app).ok();
    let info = close_session_in(
        sessions_map(),
        session_cancels_map(),
        instance_sessions_map(),
        connection.as_ref(),
        session_id,
        exit_reason,
        user_message,
    );
    if let Some(info) = &info {
        let _ = app.emit("multiplayer-state", info);
    }
    info
}

fn prune_instance_sessions(instance_id: i64) {
    let stale: Vec<String> = sessions_map()
        .iter()
        .filter(|entry| entry.instance_id == instance_id)
        .map(|entry| entry.id.clone())
        .collect();
    for session_id in stale {
        if let Some((_, cancel)) = session_cancels_map().remove(&session_id) {
            cancel.cancel();
        }
        sessions_map().remove(&session_id);
    }
    if let Some(entry) = instance_sessions_map().get(&instance_id) {
        let current = entry.value().clone();
        if !sessions_map().contains_key(&current) {
            drop(entry);
            instance_sessions_map().remove(&instance_id);
        }
    }
}

fn record_history_start(
    connection: &rusqlite::Connection,
    session_id: &str,
    instance_id: i64,
    game_version: &str,
    loader: &str,
) {
    let _ = connection.execute(
        "INSERT INTO multiplayer_history(session_id, instance_id, provider, game_version, loader, started_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            session_id,
            instance_id,
            provider().id(),
            game_version,
            loader,
            chrono_like_timestamp()
        ],
    );
}

fn finalize_history(
    connection: &rusqlite::Connection,
    session_id: &str,
    got_address: bool,
    exit_reason: &str,
) {
    let _ = connection.execute(
        "UPDATE multiplayer_history
         SET ended_at=?1, got_address=?2, exit_reason=?3
         WHERE session_id=?4 AND ended_at IS NULL",
        rusqlite::params![
            chrono_like_timestamp(),
            i64::from(got_address),
            exit_reason,
            session_id
        ],
    );
}

/// 联机受管理 helper 的 DB 对账：文件缺失 / 哈希变化即修正 DB，绝不假装可信。
pub fn reconcile_managed_helpers(
    connection: &rusqlite::Connection,
    instance_id: i64,
) -> Result<(), LauncherError> {
    let rows: Vec<(String, String, String)> = {
        let mut statement = connection
            .prepare(
                "SELECT id, file_sha256, installed_path FROM managed_content
                 WHERE instance_id=?1 AND kind='MULTIPLAYER_HELPER'",
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let rows = statement
            .query_map([instance_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        rows
    };
    for (id, expected_hash, installed_path) in rows {
        let path = PathBuf::from(&installed_path);
        let trusted = path.is_file()
            && sha256_file_sync(&path)
                .map(|hash| hash.eq_ignore_ascii_case(&expected_hash))
                .unwrap_or(false);
        if !trusted {
            connection
                .execute("DELETE FROM managed_content WHERE id=?1", [&id])
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            log::info!("联机 helper 已失效，删除受管理记录：{id}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareResult {
    pub ready: bool,
    pub provider: String,
    pub helper_version: Option<String>,
    pub message: String,
}

#[tauri::command]
pub async fn multiplayer_prepare(
    app: AppHandle,
    instance_id: i64,
) -> Result<PrepareResult, LauncherError> {
    let (game_version, loader) = instance_identity(&app, instance_id)?;
    if is_vanilla_loader(&loader) {
        return Err(vanilla_error());
    }
    let token = CancellationToken::new();
    let helper = ensure_helper_for_session(
        &app,
        instance_id,
        &game_version,
        &loader,
        "__prepare__",
        &token,
    )
    .await?;
    Ok(PrepareResult {
        ready: true,
        provider: E4MC_PROVIDER_ID.to_string(),
        helper_version: helper.version_number,
        message: "联机组件已就绪，可以创建房间。".to_string(),
    })
}

#[tauri::command]
pub async fn multiplayer_start(
    app: AppHandle,
    instance_id: i64,
    account_id: i64,
    java_path: String,
) -> Result<RoomInfo, LauncherError> {
    let (game_version, loader) = instance_identity(&app, instance_id)?;
    if is_vanilla_loader(&loader) {
        return Err(vanilla_error());
    }
    if has_active_session(instance_id) {
        return Err(LauncherError::validation(
            "该实例已有一个进行中的联机房间。",
        ));
    }
    prune_instance_sessions(instance_id);
    let session_id = Uuid::new_v4().to_string();
    let token = CancellationToken::new();
    let started_at = chrono_like_timestamp().parse().unwrap_or(0);
    sessions_map().insert(
        session_id.clone(),
        MultiplayerSession::new(
            session_id.clone(),
            instance_id,
            game_version.clone(),
            loader.clone(),
            started_at,
        ),
    );
    session_cancels_map().insert(session_id.clone(), token.clone());
    instance_sessions_map().insert(instance_id, session_id.clone());
    if let Ok(connection) = open_database(&app) {
        record_history_start(
            &connection,
            &session_id,
            instance_id,
            &game_version,
            &loader,
        );
    }
    mutate_session(&app, &session_id, |_| {});
    let helper = match ensure_helper_for_session(
        &app,
        instance_id,
        &game_version,
        &loader,
        &session_id,
        &token,
    )
    .await
    {
        Ok(helper) => helper,
        Err(error) => {
            // 用户在安装阶段主动取消：会话已被关闭，直接返回 CLOSED，避免再次报错。
            if error.error_code() == ERR_SESSION_CLOSED && !sessions_map().contains_key(&session_id)
            {
                return Ok(RoomInfo {
                    session_id: Some(session_id),
                    instance_id,
                    state: MultiplayerState::Closed,
                    lan_port: None,
                    public_address: None,
                    provider: Some(E4MC_PROVIDER_ID.to_string()),
                    helper_version: None,
                    error_code: None,
                    user_message: Some("联机创建已取消。".to_string()),
                    technical_message: None,
                    started_at: Some(started_at),
                    reconnect_count: 0,
                });
            }
            return Err(fail_session(
                &app,
                &session_id,
                error.error_code(),
                error.error_message(),
                None,
            ));
        }
    };
    mutate_session(&app, &session_id, |session| {
        session.helper_version = helper.version_number.clone();
        session.state = MultiplayerState::GameStarting;
        session.user_message = Some("正在启动游戏…".to_string());
    });
    let launched = match multiplayer_launch(app.clone(), instance_id, account_id, java_path).await {
        Ok(launched) => launched,
        Err(error) => {
            return Err(fail_session(
                &app,
                &session_id,
                ERR_GAME_START_FAILED,
                "游戏启动失败，联机未建立。",
                Some(error.error_message().to_string()),
            ));
        }
    };
    if token.is_cancelled() {
        let _ = stop_game(instance_id);
        let _ = close_session(&app, &session_id, "cancelled", "联机创建已取消。");
        return Ok(RoomInfo {
            session_id: Some(session_id),
            instance_id,
            state: MultiplayerState::Closed,
            lan_port: None,
            public_address: None,
            provider: Some(E4MC_PROVIDER_ID.to_string()),
            helper_version: helper.version_number,
            error_code: None,
            user_message: Some("联机创建已取消。".to_string()),
            technical_message: None,
            started_at: Some(started_at),
            reconnect_count: 0,
        });
    }
    mutate_session(&app, &session_id, |session| {
        session.game_pid = Some(launched.process_id);
        session.state = MultiplayerState::WaitingForLan;
        session.user_message = Some(
            "游戏已启动。进入你的世界并点击“对局域网开放”，这里会自动显示邀请地址。".to_string(),
        );
    });
    watch_game_log(session_id.clone(), launched.log_path, token, app.clone());
    Ok(sessions_map()
        .get(&session_id)
        .map(|guard| snapshot(&guard))
        .unwrap_or_else(|| RoomInfo {
            session_id: Some(session_id),
            instance_id,
            state: MultiplayerState::WaitingForLan,
            lan_port: None,
            public_address: None,
            provider: Some(E4MC_PROVIDER_ID.to_string()),
            helper_version: helper.version_number,
            error_code: None,
            user_message: None,
            technical_message: None,
            started_at: Some(started_at),
            reconnect_count: 0,
        }))
}

#[tauri::command]
pub fn multiplayer_stop(app: AppHandle, session_id: String) -> Result<RoomInfo, LauncherError> {
    let instance_id = sessions_map()
        .get(&session_id)
        .map(|guard| guard.instance_id)
        .ok_or_else(|| {
            LauncherError::classified(ERR_SESSION_CLOSED, "联机会话已经结束。", false)
        })?;
    mutate_session(&app, &session_id, |session| {
        session.state = MultiplayerState::Stopping;
        session.user_message = Some("正在结束联机并关闭游戏…".to_string());
    });
    if let Some(cancel) = session_cancels_map().get(&session_id) {
        cancel.cancel();
    }
    // e4mc 隧道与单机世界同生命周期：结束联机 = 关闭当前游戏（UI 已明确说明）。
    let _ = stop_game(instance_id);
    Ok(
        close_session(&app, &session_id, "user_stopped", "联机已结束。").unwrap_or_else(|| {
            RoomInfo {
                session_id: Some(session_id),
                instance_id,
                state: MultiplayerState::Closed,
                lan_port: None,
                public_address: None,
                provider: Some(E4MC_PROVIDER_ID.to_string()),
                helper_version: None,
                error_code: None,
                user_message: Some("联机已结束。".to_string()),
                technical_message: None,
                started_at: None,
                reconnect_count: 0,
            }
        }),
    )
}

#[tauri::command]
pub fn multiplayer_cancel(app: AppHandle, session_id: String) -> Result<RoomInfo, LauncherError> {
    let state = sessions_map()
        .get(&session_id)
        .map(|guard| guard.state)
        .ok_or_else(|| {
            LauncherError::classified(ERR_SESSION_CLOSED, "联机会话已经结束。", false)
        })?;
    if !matches!(
        state,
        MultiplayerState::Preparing | MultiplayerState::InstallingHelper
    ) {
        return Err(LauncherError::validation(
            "游戏已经启动，请使用“结束联机”来关闭。",
        ));
    }
    if let Some(cancel) = session_cancels_map().get(&session_id) {
        cancel.cancel();
    }
    Ok(
        close_session(&app, &session_id, "cancelled", "联机创建已取消。").unwrap_or_else(|| {
            RoomInfo {
                session_id: Some(session_id),
                instance_id: 0,
                state: MultiplayerState::Closed,
                lan_port: None,
                public_address: None,
                provider: Some(E4MC_PROVIDER_ID.to_string()),
                helper_version: None,
                error_code: None,
                user_message: Some("联机创建已取消。".to_string()),
                technical_message: None,
                started_at: None,
                reconnect_count: 0,
            }
        }),
    )
}

/// 加入侧握手兼容 shim。
///
/// Forge/NeoForge 客户端会在 Minecraft 握手的 serverAddress 尾部追加 `\0FORGE` / `\0FML3`
/// 等品牌后缀（原版服务器会按第一个 NUL 截断），但 e4mc relay 只做整串域名精确匹配，
/// 导致模组客户端连不上“对原版可用”的 e4mc 域名。
///
/// 该 shim 只做两件事：在本机 loopback 上承接游戏连接，把握手中的 serverAddress 规范化为
/// 纯 e4mc 域名，再原样转发到 relay 边缘。它不实现任何隧道、不缓存凭据、不转发到
/// 任意地址（目标仅限经严格校验的 `*.e4mc.link` 解析出的 relay IP），游戏退出即取消。
pub struct JoinRelayShim {
    port: u16,
    cancel: CancellationToken,
}

impl JoinRelayShim {
    pub fn port(&self) -> u16 {
        self.port
    }
}

/// 单个 Minecraft 握手帧的最大字节数（防御异常输入，正常远小于此值）。
const MAX_HANDSHAKE_FRAME_BYTES: usize = 256 * 1024;

fn resolve_relay_address(domain: &str) -> Result<SocketAddr, LauncherError> {
    let mut addrs: Vec<SocketAddr> = (domain, 25565)
        .to_socket_addrs()
        .map_err(|error| {
            LauncherError::classified(
                ERR_TUNNEL_CONNECT_FAILED,
                format!("无法解析 e4mc 域名 {domain}：{error}"),
                false,
            )
        })?
        .collect();
    addrs.sort_by_key(|addr| match addr.ip() {
        IpAddr::V4(_) => 0,
        IpAddr::V6(_) => 1,
    });
    addrs.into_iter().next().ok_or_else(|| {
        LauncherError::classified(
            ERR_TUNNEL_CONNECT_FAILED,
            format!("e4mc 域名 {domain} 没有可连接的解析地址。"),
            false,
        )
    })
}

fn read_var_int(bytes: &[u8], offset: usize) -> io::Result<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    let mut index = offset;
    loop {
        let byte = *bytes
            .get(index)
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "VarInt 数据不完整"))?;
        value |= u64::from(byte & 0x7f) << shift;
        index += 1;
        if byte & 0x80 == 0 {
            return Ok((value, index));
        }
        shift += 7;
        if shift >= 63 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "VarInt 过长"));
        }
    }
}

fn write_var_int(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// 把握手中的 serverAddress 规范化为纯 e4mc 域名（等价于原版服务器对 `\0` 后缀的截断）。
/// 无法解析的帧原样返回，交给 relay/服务器按正常协议处理。
fn rewrite_handshake_hostname(frame: &[u8], domain: &str) -> Vec<u8> {
    let Ok((packet_id, cursor)) = read_var_int(frame, 0) else {
        return frame.to_vec();
    };
    if packet_id != 0 {
        return frame.to_vec();
    }
    let Ok((protocol, cursor)) = read_var_int(frame, cursor) else {
        return frame.to_vec();
    };
    let Ok((host_len, cursor)) = read_var_int(frame, cursor) else {
        return frame.to_vec();
    };
    let host_len = usize::try_from(host_len).unwrap_or(usize::MAX);
    let Some(host_end) = cursor.checked_add(host_len) else {
        return frame.to_vec();
    };
    if host_end > frame.len() {
        return frame.to_vec();
    }
    let mut payload = Vec::with_capacity(frame.len() + domain.len() + 8);
    write_var_int(packet_id, &mut payload);
    write_var_int(protocol, &mut payload);
    write_var_int(domain.len() as u64, &mut payload);
    payload.extend_from_slice(domain.as_bytes());
    payload.extend_from_slice(&frame[host_end..]);
    let mut rewritten = Vec::with_capacity(payload.len() + 5);
    write_var_int(payload.len() as u64, &mut rewritten);
    rewritten.extend_from_slice(&payload);
    rewritten
}

fn read_one_handshake_frame(stream: &mut StdTcpStream) -> io::Result<Vec<u8>> {
    let mut frame_len: u64 = 0;
    let mut shift = 0u32;
    for _ in 0..5 {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte)?;
        let byte = byte[0];
        frame_len |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            let length = usize::try_from(frame_len)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "握手帧长度溢出"))?;
            if length > MAX_HANDSHAKE_FRAME_BYTES {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "握手帧过大"));
            }
            let mut frame = vec![0u8; length];
            stream.read_exact(&mut frame)?;
            return Ok(frame);
        }
        shift += 7;
        if shift >= 63 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "握手帧 VarInt 过长",
            ));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "握手帧 VarInt 超过 5 字节",
    ))
}

fn serve_join_shim_connection(
    mut client: StdTcpStream,
    domain: &str,
    relay: SocketAddr,
) -> io::Result<()> {
    // 监听器是非阻塞的，Windows 上 accept 出的流会继承该模式；游戏协议按阻塞读写。
    client.set_nonblocking(false)?;
    client.set_read_timeout(Some(Duration::from_secs(300)))?;
    let frame = read_one_handshake_frame(&mut client)?;
    let rewritten = rewrite_handshake_hostname(&frame, domain);
    // 诊断记录（stderr）：握手帧长度、改写结果与真实目标域名，供 E2E 取证。
    eprintln!(
        "[e4mc-shim] handshake in={} out={} target={}",
        frame.len(),
        rewritten.len(),
        domain
    );
    let mut upstream = StdTcpStream::connect_timeout(&relay, Duration::from_secs(15))?;
    upstream.set_nodelay(true)?;
    upstream.set_read_timeout(Some(Duration::from_secs(300)))?;
    upstream.write_all(&rewritten)?;
    // 双向泵：game→relay 与 relay→game 各走一个线程，任一侧 EOF/超时即整体结束。
    let mut client_to_upstream = client.try_clone()?;
    let mut upstream_to_client = upstream.try_clone()?;
    let forward = std::thread::spawn(move || {
        let copied = io::copy(&mut client_to_upstream, &mut upstream);
        eprintln!("[e4mc-shim] game->relay copied={:?}", copied);
        let _ = upstream.shutdown(Shutdown::Write);
    });
    let copied = io::copy(&mut upstream_to_client, &mut client);
    eprintln!("[e4mc-shim] relay->game copied={:?}", copied);
    let _ = client.shutdown(Shutdown::Write);
    let _ = forward.join();
    Ok(())
}

pub fn start_join_relay_shim(domain: String) -> Result<JoinRelayShim, LauncherError> {
    let relay = resolve_relay_address(&domain)?;
    let listener = StdTcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| LauncherError::storage(format!("无法启动联机加入通道：{error}")))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| LauncherError::storage(format!("无法配置联机加入通道：{error}")))?;
    let port = listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|error| LauncherError::storage(format!("无法读取加入通道端口：{error}")))?;
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let task_domain = domain.clone();
    std::thread::Builder::new()
        .name(format!("sh-e4mc-join-shim-{port}"))
        .spawn(move || {
            while !task_cancel.is_cancelled() {
                match listener.accept() {
                    Ok((client, _)) => {
                        let connection_domain = task_domain.clone();
                        std::thread::Builder::new()
                            .name("sh-e4mc-join-connection".to_string())
                            .spawn(move || {
                                if let Err(error) =
                                    serve_join_shim_connection(client, &connection_domain, relay)
                                {
                                    eprintln!("[e4mc-shim] connection error: {error}");
                                }
                            })
                            .ok();
                    }
                    Err(ref error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    Err(_) => break,
                }
            }
        })
        .map_err(|error| LauncherError::storage(format!("无法启动联机加入通道：{error}")))?;
    Ok(JoinRelayShim { port, cancel })
}

pub fn cancel_join_shim(instance_id: i64) {
    if let Some((_, shim)) = join_shims_map().remove(&instance_id) {
        shim.cancel.cancel();
    }
}

pub fn register_join_shim(instance_id: i64, shim: JoinRelayShim) {
    if let Some(previous) = join_shims_map().insert(instance_id, shim) {
        previous.cancel.cancel();
    }
}

#[tauri::command]
pub async fn multiplayer_join(
    app: AppHandle,
    address: String,
    instance_id: i64,
    account_id: i64,
    java_path: String,
) -> Result<serde_json::Value, LauncherError> {
    if !validate_e4mc_public_address(&address) {
        return Err(LauncherError::validation(
            "邀请地址格式不正确，请输入形如 xxxx.e4mc.link 的地址。",
        ));
    }
    // 模组客户端会追加 `\0FORGE` 等品牌后缀，e4mc relay 只认纯域名；通过本机 shim 在
    // 进入公网前规范化握手，保证 Forge/NeoForge 加入路径与原版一样可用。
    let shim = start_join_relay_shim(address)?;
    let join_port = shim.port();
    register_join_shim(instance_id, shim);
    let launched = multiplayer_join_launch(
        app,
        instance_id,
        account_id,
        java_path,
        "127.0.0.1".to_string(),
        join_port,
    )
    .await?;
    Ok(serde_json::json!({
        "processId": launched.process_id,
        "logPath": launched.log_path,
        "joinPort": join_port
    }))
}

#[tauri::command]
pub fn multiplayer_state(instance_id: i64) -> RoomInfo {
    let session_id = instance_sessions_map()
        .get(&instance_id)
        .map(|entry| entry.value().clone());
    session_id
        .and_then(|id| sessions_map().get(&id).map(|guard| snapshot(&guard)))
        .unwrap_or(RoomInfo {
            session_id: None,
            instance_id,
            state: MultiplayerState::Idle,
            lan_port: None,
            public_address: None,
            provider: None,
            helper_version: None,
            error_code: None,
            user_message: None,
            technical_message: None,
            started_at: None,
            reconnect_count: 0,
        })
}

/// 高级诊断：只导出脱敏后的状态摘要，不导出 token / 凭据 / 私密路径 / 原始域名。
#[tauri::command]
pub fn multiplayer_diagnostics(
    app: AppHandle,
    session_id: String,
) -> Result<serde_json::Value, LauncherError> {
    if let Some(guard) = sessions_map().get(&session_id) {
        let session = guard.value();
        return Ok(serde_json::json!({
            "sessionId": session.id,
            "provider": session.provider,
            "gameVersion": session.game_version,
            "loader": session.loader,
            "helperVersion": session.helper_version,
            "state": serde_json::to_value(session.state).unwrap_or(serde_json::Value::Null),
            "lanPort": session.lan_port,
            "publicAddressAvailable": session.public_address.is_some(),
            "reconnectCount": session.reconnect_count,
            "startedAt": session.started_at,
            "errorCode": session.error_code,
            "userMessage": session.user_message,
            "events": session
                .events
                .iter()
                .map(|event| serde_json::json!({ "at": event.at, "kind": event.kind }))
                .collect::<Vec<_>>(),
        }));
    }
    let connection = open_database(&app)?;
    let row = connection
        .query_row(
            "SELECT instance_id, provider, game_version, loader, helper_version, got_address, started_at, ended_at, exit_reason
             FROM multiplayer_history WHERE session_id=?1",
            [&session_id],
            |row| {
                Ok(serde_json::json!({
                    "sessionId": session_id,
                    "instanceId": row.get::<_, i64>(0)?,
                    "provider": row.get::<_, String>(1)?,
                    "gameVersion": row.get::<_, Option<String>>(2)?,
                    "loader": row.get::<_, Option<String>>(3)?,
                    "helperVersion": row.get::<_, Option<String>>(4)?,
                    "publicAddressAvailable": row.get::<_, i64>(5)? != 0,
                    "startedAt": row.get::<_, String>(6)?,
                    "endedAt": row.get::<_, Option<String>>(7)?,
                    "exitReason": row.get::<_, Option<String>>(8)?,
                    "state": "CLOSED",
                    "events": [],
                }))
            },
        )
        .optional()
        .map_err(|error| LauncherError::storage(error.to_string()))?
        .ok_or_else(|| LauncherError::validation("找不到该联机会话。"))?;
    Ok(row)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub session_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub provider: String,
    pub game_version: Option<String>,
    pub loader: Option<String>,
    pub helper_version: Option<String>,
    pub got_address: bool,
    pub exit_reason: Option<String>,
}

#[tauri::command]
pub fn multiplayer_history(
    app: AppHandle,
    instance_id: i64,
) -> Result<Vec<HistoryEntry>, LauncherError> {
    let connection = open_database(&app)?;
    let mut statement = connection
        .prepare(
            "SELECT session_id, started_at, ended_at, provider, game_version, loader, helper_version, got_address, exit_reason
             FROM multiplayer_history WHERE instance_id=?1 ORDER BY id DESC LIMIT 50",
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let entries = statement
        .query_map([instance_id], |row| {
            Ok(HistoryEntry {
                session_id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                provider: row.get(3)?,
                game_version: row.get(4)?,
                loader: row.get(5)?,
                helper_version: row.get(6)?,
                got_address: row.get::<_, i64>(7)? != 0,
                exit_reason: row.get(8)?,
            })
        })
        .map_err(|error| LauncherError::storage(error.to_string()))?
        .filter_map(Result::ok)
        .collect();
    Ok(entries)
}

/// 游戏退出后闭环：只关闭当前实例对应的 session，清理 token、地址、历史与实例映射。
pub fn on_game_exit(app: &AppHandle, instance_id: i64) {
    cancel_join_shim(instance_id);
    let session_id = instance_sessions_map()
        .get(&instance_id)
        .map(|entry| entry.value().clone());
    if let Some(session_id) = session_id {
        close_session(app, &session_id, "game_exited", "游戏已退出，联机已结束。");
    }
}

/// 联机活跃时禁止删除实例 / 破坏性对账 / 更新整合包（§47）。
pub fn has_active_session(instance_id: i64) -> bool {
    match instance_sessions_map().get(&instance_id) {
        Some(entry) => {
            let active = sessions_map().contains_key(entry.value());
            if !active {
                drop(entry);
                instance_sessions_map().remove(&instance_id);
            }
            active
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn test_session(instance_id: i64) -> MultiplayerSession {
        MultiplayerSession::new(
            format!("session-{instance_id}"),
            instance_id,
            "1.20.1".to_string(),
            "forge".to_string(),
            0,
        )
    }

    fn build_handshake_frame(hostname: &str, protocol: u64, port: u16, state: u64) -> Vec<u8> {
        let mut payload = Vec::new();
        write_var_int(0, &mut payload);
        write_var_int(protocol, &mut payload);
        write_var_int(hostname.len() as u64, &mut payload);
        payload.extend_from_slice(hostname.as_bytes());
        payload.extend_from_slice(&port.to_be_bytes());
        write_var_int(state, &mut payload);
        let mut frame = Vec::new();
        write_var_int(payload.len() as u64, &mut frame);
        frame.extend_from_slice(&payload);
        frame
    }

    fn parse_handshake_hostname(frame: &[u8]) -> Option<String> {
        // 跳过长前缀（帧长 VarInt）后再解析包体。
        let (_, cursor) = read_var_int(frame, 0).ok()?;
        let (packet_id, cursor) = read_var_int(frame, cursor).ok()?;
        if packet_id != 0 {
            return None;
        }
        let (_, cursor) = read_var_int(frame, cursor).ok()?;
        let (host_len, cursor) = read_var_int(frame, cursor).ok()?;
        let host_len = usize::try_from(host_len).ok()?;
        let end = cursor.checked_add(host_len)?;
        if end > frame.len() {
            return None;
        }
        String::from_utf8(frame[cursor..end].to_vec()).ok()
    }

    /// 去掉帧长前缀，得到与运行时 read_one_handshake_frame 一致的包体。
    fn frame_payload(frame: &[u8]) -> &[u8] {
        let (_, cursor) = read_var_int(frame, 0).expect("帧长前缀");
        &frame[cursor..]
    }

    #[test]
    fn handshake_rewrite_replaces_forge_branded_hostname() {
        let domain = "boxer-retail.jp.e4mc.link";
        let frame = build_handshake_frame(&format!("{domain}\0FORGE"), 765, 25565, 2);
        let rewritten = rewrite_handshake_hostname(frame_payload(&frame), domain);
        assert_eq!(
            parse_handshake_hostname(&rewritten).as_deref(),
            Some(domain),
            "握手 serverAddress 应被规范化为纯域名（去掉 \\0FORGE 后缀）"
        );
        // 帧其余部分（端口 / 下一状态）必须原样保留。
        assert_eq!(&rewritten[rewritten.len() - 3..], &frame[frame.len() - 3..]);
    }

    #[test]
    fn handshake_rewrite_keeps_plain_domain_and_passthrough() {
        let domain = "cod-sprinkled.jp.e4mc.link";
        let frame = build_handshake_frame(domain, 765, 25565, 2);
        let rewritten = rewrite_handshake_hostname(frame_payload(&frame), domain);
        assert_eq!(
            parse_handshake_hostname(&rewritten).as_deref(),
            Some(domain)
        );

        // 非握手包（legacy ping）与无法解析的畸形帧：原样透传，不破坏字节。
        let legacy = vec![0xfe, 0x01];
        assert_eq!(rewrite_handshake_hostname(&legacy, domain), legacy);
        let truncated = vec![0x02, 0x00, 0x05, b'a'];
        assert_eq!(rewrite_handshake_hostname(&truncated, domain), truncated);
    }

    #[test]
    fn join_shim_rewrites_and_forwards_forge_branded_handshake() {
        let relay_listener = StdTcpListener::bind(("127.0.0.1", 0)).unwrap();
        let relay_addr = relay_listener.local_addr().unwrap();
        let shim_listener = StdTcpListener::bind(("127.0.0.1", 0)).unwrap();
        let shim_addr = shim_listener.local_addr().unwrap();
        let domain = "shim-test.jp.e4mc.link";

        let relay_thread = std::thread::spawn(move || {
            let (mut relay_side, _) = relay_listener.accept().unwrap();
            let frame = read_one_handshake_frame(&mut relay_side).unwrap();
            let (packet_id, cursor) = read_var_int(&frame, 0).unwrap();
            assert_eq!(packet_id, 0);
            let (_, cursor) = read_var_int(&frame, cursor).unwrap();
            let (host_len, cursor) = read_var_int(&frame, cursor).unwrap();
            let host_len = host_len as usize;
            assert_eq!(&frame[cursor..cursor + host_len], domain.as_bytes());
            relay_side.write_all(&[0x99]).unwrap();
            let mut extra = [0u8; 5];
            relay_side.read_exact(&mut extra).unwrap();
            assert_eq!(&extra, b"hello");
        });

        let client_thread = std::thread::spawn(move || {
            let mut client = StdTcpStream::connect(shim_addr).unwrap();
            let handshake = build_handshake_frame(&format!("{domain}\0FORGE"), 765, 25565, 2);
            client.write_all(&handshake).unwrap();
            client.write_all(b"hello").unwrap();
            let mut response = [0u8; 1];
            client.read_exact(&mut response).unwrap();
            assert_eq!(response, [0x99]);
        });

        let (shim_client, _) = shim_listener.accept().unwrap();
        serve_join_shim_connection(shim_client, domain, relay_addr).unwrap();
        relay_thread.join().unwrap();
        client_thread.join().unwrap();
    }

    fn fixture_versions() -> Vec<serde_json::Value> {
        serde_json::json!([
            {
                "id": "v_forge_621",
                "version_number": "6.2.1-forge",
                "version_type": "release",
                "date_published": "2026-05-02T00:00:00.000Z",
                "loaders": ["forge"],
                "game_versions": ["1.18.2", "1.20.1", "1.20.4"],
                "files": [{
                    "primary": true,
                    "filename": "e4mc-6.2.1-forge.jar",
                    "url": "https://cdn.modrinth.com/data/qANg5Jrr/versions/v_forge_621/e4mc.jar",
                    "size": 120000,
                    "hashes": { "sha1": "a".repeat(40), "sha512": "b".repeat(128) }
                }]
            },
            {
                "id": "v_forge_620",
                "version_number": "6.2.0-forge",
                "version_type": "release",
                "date_published": "2026-04-01T00:00:00.000Z",
                "loaders": ["forge"],
                "game_versions": ["1.20.1"],
                "files": [{
                    "primary": true,
                    "filename": "e4mc-6.2.0-forge.jar",
                    "url": "https://cdn.modrinth.com/data/qANg5Jrr/versions/v_forge_620/e4mc.jar",
                    "size": 120000,
                    "hashes": { "sha1": "c".repeat(40) }
                }]
            },
            {
                "id": "v_fabric_621",
                "version_number": "6.2.1-fabric",
                "version_type": "release",
                "date_published": "2026-05-02T00:00:00.000Z",
                "loaders": ["fabric", "quilt"],
                "game_versions": ["1.20.1", "1.21.1", "1.21.11"],
                "files": [{
                    "primary": true,
                    "filename": "e4mc-6.2.1-fabric.jar",
                    "url": "https://cdn.modrinth.com/data/qANg5Jrr/versions/v_fabric_621/e4mc.jar",
                    "size": 120000,
                    "hashes": { "sha1": "d".repeat(40) }
                }]
            }
        ])
        .as_array()
        .unwrap()
        .clone()
    }

    #[test]
    fn lan_port_never_becomes_public_address() {
        let mut session = test_session(1);
        session.state = MultiplayerState::WaitingForLan;
        let event = provider()
            .parse_log_line("[14:02:10] [Server thread/INFO]: Local game hosted on port 52913")
            .expect("lan line parsed");
        assert_eq!(event, ProviderEvent::LanOpened(52913));
        let info = apply_event(&mut session, &event).expect("state changed");
        assert_eq!(info.state, MultiplayerState::LanOpened);
        assert_eq!(info.lan_port, Some(52913));
        assert_eq!(info.public_address, None);
    }

    #[test]
    fn valid_e4mc_address_marks_ready() {
        let mut session = test_session(1);
        session.state = MultiplayerState::WaitingForTunnel;
        let event = provider()
            .parse_log_line(
                "[14:02:13] [Server thread/INFO] [e4mc/]: Domain assigned: sunset-abc.e4mc.link",
            )
            .expect("domain line parsed");
        let info = apply_event(&mut session, &event).expect("ready");
        assert_eq!(info.state, MultiplayerState::Ready);
        assert_eq!(info.public_address.as_deref(), Some("sunset-abc.e4mc.link"));
    }

    #[test]
    fn invalid_public_address_rejected() {
        let mut session = test_session(1);
        session.state = MultiplayerState::WaitingForTunnel;
        for line in [
            "[e4mc/]: Domain assigned: play.example.com",
            "[e4mc/]: e4mc link: abc.e4mc.link:25565",
            "[e4mc/]: e4mc link: evil//host.e4mc.link",
            "[e4mc/]: e4mc link: -bad-.e4mc.link",
        ] {
            if let Some(event) = provider().parse_log_line(line) {
                assert_eq!(apply_event(&mut session, &event), None);
            }
        }
        assert_eq!(session.state, MultiplayerState::WaitingForTunnel);
        assert!(session.public_address.is_none());
    }

    #[test]
    fn unknown_log_does_not_change_state() {
        let mut session = test_session(1);
        session.state = MultiplayerState::WaitingForTunnel;
        let before = snapshot(&session);
        for line in [
            "[main/INFO]: Loading 15 mods",
            "[STDERR]: some noise",
            "Starting integrated minecraft server version 1.20.1",
        ] {
            if let Some(event) = provider().parse_log_line(line) {
                assert_eq!(apply_event(&mut session, &event), None);
            }
        }
        assert_eq!(snapshot(&session).state, before.state);
        assert_eq!(snapshot(&session).public_address, before.public_address);
    }

    #[test]
    fn provider_error_sets_error_state() {
        let mut session = test_session(1);
        session.state = MultiplayerState::Ready;
        session.public_address = Some("sunset-abc.e4mc.link".to_string());
        let event = provider()
            .parse_log_line("[14:03:00] [Server thread/ERROR] [e4mc/]: error in e4mc")
            .expect("error line parsed");
        let info = apply_event(&mut session, &event).expect("error state");
        assert_eq!(info.state, MultiplayerState::Error);
        assert_eq!(info.error_code.as_deref(), Some(ERR_PROVIDER_UNAVAILABLE));
    }

    #[test]
    fn old_session_event_does_not_mutate_new_session() {
        let sessions = DashMap::new();
        sessions.insert("new-session".to_string(), test_session(2));
        assert_eq!(
            transition(
                &sessions,
                "old-session",
                &ProviderEvent::PublicAddressReady("abc.e4mc.link".to_string())
            ),
            None
        );
        let current = sessions.get("new-session").expect("new session exists");
        assert_eq!(current.state, MultiplayerState::Preparing);
        assert!(current.public_address.is_none());
    }

    #[test]
    fn cancel_old_session_does_not_cancel_new_session() {
        let old = CancellationToken::new();
        let new = CancellationToken::new();
        old.cancel();
        assert!(old.is_cancelled());
        assert!(!new.is_cancelled());
    }

    #[test]
    fn managed_helper_missing_triggers_repair() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::run_migrations(&mut connection).unwrap();
        let directory = std::env::temp_dir().join(format!("sh-mp-{}", crate::unique_timestamp()));
        let mods = directory.join(".minecraft").join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let status = verify_managed_helper(&connection, 7, "1.20.1", "forge", &mods).unwrap();
        assert!(matches!(status, HelperStatus::NeedsInstall(_)));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn wrong_hash_helper_not_trusted() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::run_migrations(&mut connection).unwrap();
        let directory = std::env::temp_dir().join(format!("sh-mp-{}", crate::unique_timestamp()));
        let mods = directory.join(".minecraft").join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let jar = mods.join("e4mc-tampered.jar");
        std::fs::write(&jar, b"user-tampered-bytes").unwrap();
        connection
            .execute(
                "INSERT INTO instances(id, name, root_path, game_version, loader_type, memory_mb, status, source, created_at)
                 VALUES(7, 'test', '/tmp/sh-mp-instance-7', '1.20.1', 'forge', 4096, 'ready', 'test', '0')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO managed_content(id, instance_id, kind, provider, project_id, version_id, version_number, file_sha1, file_sha256, installed_path, installed_by_launcher, created_at)
                 VALUES('e4mc-7', 7, 'MULTIPLAYER_HELPER', 'modrinth', 'qANg5Jrr', 'v_fake', NULL, NULL, 'deadbeef', ?1, 1, '0')",
                rusqlite::params![jar.to_string_lossy()],
            )
            .unwrap();
        let status = verify_managed_helper(&connection, 7, "1.20.1", "forge", &mods).unwrap();
        assert!(
            matches!(status, HelperStatus::NeedsInstall(ref reason) if reason.contains("修改"))
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn unsupported_loader_returns_actionable_error() {
        let versions = fixture_versions();
        assert!(select_strict_version(&versions, "1.20.1", "sponge").is_none());
        let message = incompatible_message(&versions, "1.20.1", "sponge");
        assert!(message.contains("sponge"));
        assert!(message.contains("forge"));
        assert!(message.contains("fabric"));
    }

    #[test]
    fn compatible_provider_version_selected() {
        let versions = fixture_versions();
        let forge = select_strict_version(&versions, "1.20.1", "forge").unwrap();
        assert_eq!(
            forge.get("version_number").and_then(|v| v.as_str()),
            Some("6.2.1-forge")
        );
        // Quilt 复用 fabric 构件（loaders 同时列出 fabric/quilt）。
        let quilt = select_strict_version(&versions, "1.20.1", "quilt").unwrap();
        assert_eq!(
            quilt.get("version_number").and_then(|v| v.as_str()),
            Some("6.2.1-fabric")
        );
        // 只支持 1.21.11 的 fabric 版本按版本精确匹配。
        assert_eq!(
            select_strict_version(&versions, "1.21.11", "fabric")
                .and_then(|v| v.get("version_number"))
                .and_then(|v| v.as_str()),
            Some("6.2.1-fabric")
        );
        assert!(select_strict_version(&versions, "1.21.11", "forge").is_none());
    }

    #[test]
    fn game_exit_closes_only_current_session() {
        let sessions = DashMap::new();
        let cancels = DashMap::new();
        let instances = DashMap::new();
        sessions.insert("a".to_string(), test_session(1));
        sessions.insert("b".to_string(), test_session(2));
        cancels.insert("a".to_string(), CancellationToken::new());
        cancels.insert("b".to_string(), CancellationToken::new());
        instances.insert(1, "a".to_string());
        instances.insert(2, "b".to_string());
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::run_migrations(&mut connection).unwrap();
        let closed = close_session_in(
            &sessions,
            &cancels,
            &instances,
            Some(&connection),
            "a",
            "game_exited",
            "closed",
        );
        assert!(closed.is_some());
        assert!(!sessions.contains_key("a"));
        assert!(sessions.contains_key("b"));
        assert!(!instances.contains_key(&1));
        assert!(instances.contains_key(&2));
        assert!(cancels.get("b").is_some_and(|token| !token.is_cancelled()));
    }

    #[test]
    fn repeated_create_and_stop_resets_state() {
        let sessions = DashMap::new();
        let cancels = DashMap::new();
        let instances = DashMap::new();
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::run_migrations(&mut connection).unwrap();
        for round in 0..3 {
            let session_id = format!("round-{round}");
            sessions.insert(
                session_id.clone(),
                MultiplayerSession::new(session_id.clone(), 9, "1.20.1".into(), "forge".into(), 0),
            );
            cancels.insert(session_id.clone(), CancellationToken::new());
            instances.insert(9, session_id.clone());
            close_session_in(
                &sessions,
                &cancels,
                &instances,
                Some(&connection),
                &session_id,
                "user_stopped",
                "closed",
            );
            assert!(!sessions.contains_key(&session_id));
            assert!(!instances.contains_key(&9));
        }
        let next = MultiplayerSession::new("next".into(), 9, "1.20.1".into(), "forge".into(), 0);
        sessions.insert("next".into(), next);
        assert_eq!(
            sessions.get("next").unwrap().state,
            MultiplayerState::Preparing
        );
    }

    #[test]
    fn watcher_fixture_roundtrip_with_late_create_and_truncate() {
        let sessions = Arc::new(DashMap::new());
        let mut session = test_session(1);
        session.state = MultiplayerState::WaitingForLan;
        sessions.insert("w".to_string(), session);
        let directory = std::env::temp_dir().join(format!("sh-mp-{}", crate::unique_timestamp()));
        std::fs::create_dir_all(&directory).unwrap();
        let log = directory.join("latest.log");
        let updates = Arc::new(Mutex::new(Vec::<RoomInfo>::new()));
        let cancel = CancellationToken::new();
        let handle = {
            let updates = Arc::clone(&updates);
            let log = log.to_string_lossy().to_string();
            spawn_log_watcher(
                "w".to_string(),
                Arc::clone(&sessions),
                log,
                cancel.clone(),
                move |info| updates.lock().unwrap().push(info),
            )
        };
        std::thread::sleep(Duration::from_millis(120));
        // 日志晚创建：watcher 必须能等到文件出现。
        {
            let mut file = std::fs::File::create(&log).unwrap();
            writeln!(
                file,
                "[14:02:10] [Server thread/INFO]: Local game hosted on port 52913"
            )
            .unwrap();
        }
        std::thread::sleep(Duration::from_millis(300));
        {
            let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
            writeln!(
                file,
                "[14:02:13] [Server thread/INFO] [e4mc/]: Domain assigned: sunset-abc.e4mc.link"
            )
            .unwrap();
        }
        std::thread::sleep(Duration::from_millis(300));
        // truncate + 重建：新会话的地址必须重新识别。
        std::fs::write(&log, "").unwrap();
        std::thread::sleep(Duration::from_millis(150));
        {
            let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
            writeln!(
                file,
                "[14:05:00] [Server thread/INFO] [e4mc/]: Domain assigned: morning-xyz.e4mc.link"
            )
            .unwrap();
        }
        std::thread::sleep(Duration::from_millis(400));
        cancel.cancel();
        handle.join().unwrap();
        let session = sessions.get("w").unwrap();
        assert_eq!(session.state, MultiplayerState::Ready);
        assert_eq!(
            session.public_address.as_deref(),
            Some("morning-xyz.e4mc.link")
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn public_endpoint_notice_after_timeout() {
        let mut session = test_session(1);
        session.state = MultiplayerState::WaitingForTunnel;
        session.lan_port = Some(52913);
        session.lan_opened_at =
            Instant::now().checked_sub(Duration::from_secs(PUBLIC_ENDPOINT_NOTICE_SECS + 5));
        let info = apply_notice(&mut session).expect("notice emitted");
        assert_eq!(info.state, MultiplayerState::WaitingForTunnel);
        assert_eq!(
            info.error_code.as_deref(),
            Some(ERR_PUBLIC_ADDRESS_NOT_FOUND)
        );
        assert!(apply_notice(&mut session).is_none());
    }

    #[test]
    fn zh_localization_lines_parsed() {
        let lan = provider()
            .parse_log_line("[14:02:10] [Server thread/INFO]: 已开启本地局域网服务器，端口: 52913");
        assert_eq!(lan, Some(ProviderEvent::LanOpened(52913)));
        let domain =
            provider().parse_log_line("[CHAT] 将本地游戏托管在域名[sunset-abc.e4mc.link]上");
        assert_eq!(
            domain,
            Some(ProviderEvent::PublicAddressReady(
                "sunset-abc.e4mc.link".to_string()
            ))
        );
    }

    #[test]
    fn fixture_parsers_cover_all_loaders() {
        for fixture in [
            include_str!("multiplayer_fixtures/forge-1.20.1.log"),
            include_str!("multiplayer_fixtures/fabric-1.21.1.log"),
            include_str!("multiplayer_fixtures/neoforge-1.21.1.log"),
            include_str!("multiplayer_fixtures/quilt-1.20.1.log"),
        ] {
            let events: Vec<ProviderEvent> = fixture
                .lines()
                .filter_map(|line| provider().parse_log_line(line))
                .collect();
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, ProviderEvent::LanOpened(_))),
                "fixture must contain LAN open"
            );
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    ProviderEvent::PublicAddressReady(address) if validate_e4mc_public_address(address)
                )),
                "fixture must contain valid public address"
            );
        }
    }

    #[test]
    fn validate_e4mc_public_address_strict() {
        assert!(validate_e4mc_public_address("abc.e4mc.link"));
        assert!(validate_e4mc_public_address(" play.eu-1.e4mc.link. "));
        assert!(!validate_e4mc_public_address("localhost"));
        assert!(!validate_e4mc_public_address("abc.e4mc.link:25565"));
        assert!(!validate_e4mc_public_address("evil.example.com"));
        assert!(!validate_e4mc_public_address(".e4mc.link"));
        assert!(!validate_e4mc_public_address("a..e4mc.link"));
        assert!(!validate_e4mc_public_address("中文.e4mc.link"));
    }

    #[test]
    fn parses_real_regional_e4mc_chat_line() {
        let line = "[18:55:18] [Render thread/INFO] [minecraft/ChatComponent]: [System] [CHAT] Local game hosted on domain [cod-sprinkled.jp.e4mc.link] (Click here to stop)";
        match parse_e4mc_log_line(line) {
            Some(ProviderEvent::PublicAddressReady(domain)) => {
                assert_eq!(domain, "cod-sprinkled.jp.e4mc.link");
                assert!(validate_e4mc_public_address(&domain));
            }
            other => panic!("真实 e4mc 区域域名日志行未被识别：{other:?}"),
        }
    }

    #[test]
    fn log_tailer_delivers_backlogged_lines_after_writer_stops() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("sh-tailer-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("game.log");
        {
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(b"line1\nline2\n").unwrap();
        }
        let mut tail = LogTailer::new(path.clone());
        let mut delivered: Vec<String> = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while delivered.len() < 2 && std::time::Instant::now() < deadline {
            if let Some(line) = tail.next_line() {
                delivered.push(line);
            }
        }
        assert_eq!(delivered, vec!["line1", "line2"]);
        // 追加一行后停止写入：这行必须从缓冲区交付，而不是永远卡住。
        {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            file.write_all(b"line3\n").unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut third = None;
        while third.is_none() && std::time::Instant::now() < deadline {
            third = tail.next_line();
        }
        assert_eq!(third.as_deref(), Some("line3"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
