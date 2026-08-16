//! 服务器连接诊断：地址解析（含 IPv6/SRV）、DNS、TCP 探测与断线分类。
//! Launcher 的 TCP 探测成功不等于“已进入服务器”；Minecraft 协议握手/登录
//! 由游戏客户端完成，本模块只提供可分类的中间证据。

use serde::Serialize;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
// 完整断线分类学；部分分类由后续协议探测路径构造，先保留全集。
#[allow(dead_code)]
pub(crate) enum ConnectionClassification {
    InvalidAddress,
    DnsFailed,
    SrvFailed,
    TcpTimeout,
    TcpRefused,
    NetworkUnreachable,
    ProxyFailure,
    ProtocolStatusFailed,
    ServerOffline,
    ServerFull,
    ServerWhitelisted,
    VersionMismatch,
    LoaderMismatch,
    ModChannelMismatch,
    MissingClientMod,
    MissingServerMod,
    AuthRequired,
    InvalidSession,
    NotAuthenticated,
    Banned,
    Kicked,
    EncryptionHandshakeFailed,
    ClientCrashDuringJoin,
    ClientMixinCrashDuringJoin,
    ServerDisconnectOther,
    UnknownConnectionFailure,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AddressInfo {
    raw: String,
    host: String,
    port: u16,
    srv_target: Option<String>,
    srv_port: Option<u16>,
    resolved: Vec<String>,
    dns_latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ServerProbe {
    classification: ConnectionClassification,
    address: AddressInfo,
    tcp_connect_ms: Option<u64>,
    message: String,
}

/// 解析服务器地址：hostname、hostname:port、IPv4:port、[IPv6]:port。
pub(crate) fn parse_address(raw: &str) -> Result<(String, u16), ConnectionClassification> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ConnectionClassification::InvalidAddress);
    }
    if let Some(rest) = raw.strip_prefix('[') {
        let Some((host, port_part)) = rest.split_once("]:") else {
            if rest.ends_with(']') {
                let host = rest.trim_end_matches(']');
                if host.parse::<Ipv6Addr>().is_ok() {
                    return Ok((host.to_string(), 25565));
                }
            }
            return Err(ConnectionClassification::InvalidAddress);
        };
        if host.parse::<Ipv6Addr>().is_err() {
            return Err(ConnectionClassification::InvalidAddress);
        }
        let port = port_part
            .parse::<u16>()
            .map_err(|_| ConnectionClassification::InvalidAddress)?;
        return Ok((host.to_string(), port));
    }
    if let Ok(ipv4) = raw.parse::<Ipv4Addr>() {
        return Ok((ipv4.to_string(), 25565));
    }
    if let Ok(ipv6) = raw.parse::<Ipv6Addr>() {
        return Ok((ipv6.to_string(), 25565));
    }
    if raw.contains(':') {
        if let Some((host, port)) = raw.rsplit_once(':') {
            if host.contains(':') {
                return Err(ConnectionClassification::InvalidAddress);
            }
            let port = port
                .parse::<u16>()
                .map_err(|_| ConnectionClassification::InvalidAddress)?;
            return Ok((host.to_string(), port));
        }
    }
    if raw.contains(' ') || raw.contains('/') {
        return Err(ConnectionClassification::InvalidAddress);
    }
    Ok((raw.to_string(), 25565))
}

/// 极简 DNS SRV 查询（UDP）：仅用于诊断 `_minecraft._tcp.<host>`。
pub(crate) fn build_srv_query(host: &str, id: u16) -> Vec<u8> {
    let mut packet = Vec::new();
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&0x0100u16.to_be_bytes()); // RD
    packet.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    packet.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT
    packet.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    packet.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT
    let name = format!("_minecraft._tcp.{host}");
    for label in name.split('.') {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&33u16.to_be_bytes()); // SRV
    packet.extend_from_slice(&1u16.to_be_bytes()); // IN
    packet
}

/// 解析 DNS 响应中的第一个 SRV 记录。
pub(crate) fn parse_srv_response(packet: &[u8]) -> Result<(String, u16), ()> {
    if packet.len() < 12 {
        return Err(());
    }
    let ancount = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    if ancount == 0 {
        return Err(());
    }
    let mut offset = 12usize;
    // 跳过 question
    let question_count = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    for _ in 0..question_count {
        offset = skip_name(packet, offset)?;
        offset += 4; // type + class
    }
    for _ in 0..ancount {
        offset = skip_name(packet, offset)?;
        if offset + 10 > packet.len() {
            return Err(());
        }
        let record_type = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
        let data_len = u16::from_be_bytes([packet[offset + 8], packet[offset + 9]]) as usize;
        offset += 10;
        if offset + data_len > packet.len() {
            return Err(());
        }
        if record_type == 33 && data_len >= 6 {
            // rdata: priority(2) + weight(2) + port(2) + target
            let port = u16::from_be_bytes([packet[offset + 4], packet[offset + 5]]);
            let mut target_offset = offset + 6;
            let target = read_name(packet, &mut target_offset)?;
            if !target.is_empty() {
                return Ok((target, port));
            }
        }
        offset += data_len;
    }
    Err(())
}

fn skip_name(packet: &[u8], mut offset: usize) -> Result<usize, ()> {
    loop {
        if offset >= packet.len() {
            return Err(());
        }
        let length = packet[offset] as usize;
        if length == 0 {
            return Ok(offset + 1);
        }
        if length & 0xC0 == 0xC0 {
            return Ok(offset + 2); // compression pointer
        }
        offset += 1 + length;
    }
}

fn read_name(packet: &[u8], offset: &mut usize) -> Result<String, ()> {
    let mut labels = Vec::new();
    let mut jumped = false;
    let mut cursor = *offset;
    loop {
        if cursor >= packet.len() {
            return Err(());
        }
        let length = packet[cursor] as usize;
        if length == 0 {
            if !jumped {
                *offset = cursor + 1;
            }
            break;
        }
        if length & 0xC0 == 0xC0 {
            if cursor + 1 >= packet.len() {
                return Err(());
            }
            let pointer = ((length & 0x3F) << 8) | packet[cursor + 1] as usize;
            if !jumped {
                *offset = cursor + 2;
            }
            cursor = pointer;
            jumped = true;
            continue;
        }
        if cursor + 1 + length > packet.len() {
            return Err(());
        }
        labels.push(String::from_utf8_lossy(&packet[cursor + 1..cursor + 1 + length]).to_string());
        cursor += 1 + length;
    }
    Ok(labels.join("."))
}

// 供 UI 断线分类使用；当前由测试覆盖，注册 UI 后由诊断页面调用。
#[allow(dead_code)]
pub(crate) fn classify_disconnect(log_text: &str) -> ConnectionClassification {
    let lower = log_text.to_ascii_lowercase();
    if lower.contains("mixin") && lower.contains("transform") {
        return ConnectionClassification::ClientMixinCrashDuringJoin;
    }
    if lower.contains("failed to log in: invalid session")
        || lower.contains("invalid session")
        || lower.contains("loginfailedinfo.invalidsession")
    {
        return ConnectionClassification::InvalidSession;
    }
    if lower.contains("you are not whitelisted") || lower.contains("not whitelisted") {
        return ConnectionClassification::ServerWhitelisted;
    }
    if lower.contains("server is full") || lower.contains("server full") {
        return ConnectionClassification::ServerFull;
    }
    if lower.contains("outdated client") || lower.contains("outdated server") {
        return ConnectionClassification::VersionMismatch;
    }
    if lower.contains("banned") {
        return ConnectionClassification::Banned;
    }
    if lower.contains("authentication servers are down")
        || lower.contains("authentication required")
    {
        return ConnectionClassification::AuthRequired;
    }
    if lower.contains("connection refused") || lower.contains("connectexception") {
        return ConnectionClassification::TcpRefused;
    }
    if lower.contains("unknown host")
        || lower.contains("unknownhost")
        || lower.contains("unresolvedaddress")
    {
        return ConnectionClassification::DnsFailed;
    }
    if lower.contains("timed out") || lower.contains("timeout") {
        return ConnectionClassification::TcpTimeout;
    }
    if lower.contains("internal exception") || lower.contains("io.netty") {
        return ConnectionClassification::ProtocolStatusFailed;
    }
    if lower.contains("disconnect.") || lower.contains("kicked") {
        return ConnectionClassification::Kicked;
    }
    ConnectionClassification::UnknownConnectionFailure
}

async fn resolve_srv(host: &str) -> Option<(String, u16)> {
    const DNS_SERVERS: [&str; 4] = ["223.5.5.5", "119.29.29.29", "114.114.114.114", "8.8.8.8"];
    const TIMEOUT: Duration = Duration::from_secs(3);
    for server in DNS_SERVERS {
        let Ok(addr) = format!("{server}:53").parse::<SocketAddr>() else {
            continue;
        };
        let Ok(socket) = tokio::net::UdpSocket::bind("0.0.0.0:0").await else {
            continue;
        };
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.subsec_nanos() as u16)
            .unwrap_or(1);
        let query = build_srv_query(host, id);
        if socket.send_to(&query, addr).await.is_err() {
            continue;
        }
        let mut buffer = [0u8; 4096];
        if let Ok(Ok((size, _))) =
            tokio::time::timeout(TIMEOUT, socket.recv_from(&mut buffer)).await
        {
            if let Ok((target, port)) = parse_srv_response(&buffer[..size]) {
                return Some((target, port));
            }
        }
    }
    None
}

/// 诊断服务器：解析地址 → SRV → DNS A/AAAA → TCP 探测。
#[tauri::command]
pub(crate) async fn diagnose_server(raw_address: String) -> Result<ServerProbe, String> {
    let (mut host, mut port) = parse_address(&raw_address).map_err(|classification| {
        format!(
            "地址无效（{}）：{raw_address}",
            classification_name(&classification)
        )
    })?;
    let mut srv_target = None;
    let mut srv_port = None;
    if !host.parse::<Ipv4Addr>().is_ok() && !host.parse::<Ipv6Addr>().is_ok() {
        if let Some((target, srv_port_value)) = resolve_srv(&host).await {
            srv_target = Some(target.clone());
            srv_port = Some(srv_port_value);
            host = target;
            port = srv_port_value;
        }
    }

    let dns_started = std::time::Instant::now();
    let resolved: Vec<String> = match tokio::net::lookup_host((host.as_str(), port)).await {
        Ok(addresses) => addresses
            .take(4)
            .map(|address| address.to_string())
            .collect(),
        Err(_) => Vec::new(),
    };
    let dns_latency_ms = Some(dns_started.elapsed().as_millis() as u64);
    if resolved.is_empty() {
        return Ok(ServerProbe {
            classification: ConnectionClassification::DnsFailed,
            address: AddressInfo {
                raw: raw_address,
                host,
                port,
                srv_target,
                srv_port,
                resolved,
                dns_latency_ms,
            },
            tcp_connect_ms: None,
            message: "DNS 解析失败，无法找到服务器地址。".to_string(),
        });
    }

    let tcp_started = std::time::Instant::now();
    let target = resolved[0].clone();
    let tcp_result = tokio::time::timeout(
        Duration::from_secs(8),
        tokio::net::TcpStream::connect(&target),
    )
    .await;
    let tcp_connect_ms = Some(tcp_started.elapsed().as_millis() as u64);
    let (classification, message) = match tcp_result {
        Ok(Ok(_)) => (
            ConnectionClassification::ServerOffline,
            format!("TCP 连接成功（{tcp_connect_ms:?} ms），等待 Minecraft 协议握手；连接成功不等于已进入服务器。"),
        ),
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => (
            ConnectionClassification::TcpRefused,
            "TCP 连接被拒绝：目标端口未监听。".to_string(),
        ),
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::TimedOut => (
            ConnectionClassification::TcpTimeout,
            "TCP 连接超时，目标可能离线或防火墙拦截。".to_string(),
        ),
        Err(_) => (
            ConnectionClassification::TcpTimeout,
            "TCP 探测超时（8 秒）。".to_string(),
        ),
        Ok(Err(_)) => (
            ConnectionClassification::UnknownConnectionFailure,
            "TCP 连接失败。".to_string(),
        ),
    };

    Ok(ServerProbe {
        classification,
        address: AddressInfo {
            raw: raw_address,
            host,
            port,
            srv_target,
            srv_port,
            resolved,
            dns_latency_ms,
        },
        tcp_connect_ms,
        message,
    })
}

fn classification_name(classification: &ConnectionClassification) -> &'static str {
    match classification {
        ConnectionClassification::InvalidAddress => "INVALID_ADDRESS",
        ConnectionClassification::DnsFailed => "DNS_FAILED",
        ConnectionClassification::SrvFailed => "SRV_FAILED",
        ConnectionClassification::TcpTimeout => "TCP_TIMEOUT",
        ConnectionClassification::TcpRefused => "TCP_REFUSED",
        ConnectionClassification::NetworkUnreachable => "NETWORK_UNREACHABLE",
        ConnectionClassification::ProxyFailure => "PROXY_FAILURE",
        ConnectionClassification::ProtocolStatusFailed => "PROTOCOL_STATUS_FAILED",
        ConnectionClassification::ServerOffline => "SERVER_OFFLINE",
        ConnectionClassification::ServerFull => "SERVER_FULL",
        ConnectionClassification::ServerWhitelisted => "SERVER_WHITELISTED",
        ConnectionClassification::VersionMismatch => "VERSION_MISMATCH",
        ConnectionClassification::LoaderMismatch => "LOADER_MISMATCH",
        ConnectionClassification::ModChannelMismatch => "MOD_CHANNEL_MISMATCH",
        ConnectionClassification::MissingClientMod => "MISSING_CLIENT_MOD",
        ConnectionClassification::MissingServerMod => "MISSING_SERVER_MOD",
        ConnectionClassification::AuthRequired => "AUTH_REQUIRED",
        ConnectionClassification::InvalidSession => "INVALID_SESSION",
        ConnectionClassification::NotAuthenticated => "NOT_AUTHENTICATED",
        ConnectionClassification::Banned => "BANNED",
        ConnectionClassification::Kicked => "KICKED",
        ConnectionClassification::EncryptionHandshakeFailed => "ENCRYPTION_HANDSHAKE_FAILED",
        ConnectionClassification::ClientCrashDuringJoin => "CLIENT_CRASH_DURING_JOIN",
        ConnectionClassification::ClientMixinCrashDuringJoin => "CLIENT_MIXIN_CRASH_DURING_JOIN",
        ConnectionClassification::ServerDisconnectOther => "SERVER_DISCONNECT_OTHER",
        ConnectionClassification::UnknownConnectionFailure => "UNKNOWN_CONNECTION_FAILURE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hostname_default_port() {
        let (host, port) = parse_address("mc.example.com").expect("ok");
        assert_eq!(host, "mc.example.com");
        assert_eq!(port, 25565);
    }

    #[test]
    fn parse_hostname_with_port() {
        let (host, port) = parse_address("mc.example.com:25566").expect("ok");
        assert_eq!(host, "mc.example.com");
        assert_eq!(port, 25566);
    }

    #[test]
    fn parse_ipv4() {
        let (host, port) = parse_address("127.0.0.1").expect("ok");
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 25565);
        let (host, port) = parse_address("1.2.3.4:19132").expect("ok");
        assert_eq!(host, "1.2.3.4");
        assert_eq!(port, 19132);
    }

    #[test]
    fn parse_ipv6_bracketed() {
        let (host, port) = parse_address("[2001:db8::1]:25566").expect("ok");
        assert_eq!(host, "2001:db8::1");
        assert_eq!(port, 25566);
        let (host, port) = parse_address("[::1]").expect("ok");
        assert_eq!(host, "::1");
        assert_eq!(port, 25565);
    }

    #[test]
    fn parse_invalid_addresses() {
        assert_eq!(
            parse_address(""),
            Err(ConnectionClassification::InvalidAddress)
        );
        assert_eq!(
            parse_address("a b"),
            Err(ConnectionClassification::InvalidAddress)
        );
        assert_eq!(
            parse_address("2001:db8::1:25566"),
            Err(ConnectionClassification::InvalidAddress)
        );
    }

    #[test]
    fn srv_query_has_correct_shape() {
        let packet = build_srv_query("example.com", 0x1234);
        assert_eq!(&packet[0..2], &[0x12, 0x34]);
        assert_eq!(&packet[4..6], &[0, 1]);
        let text = String::from_utf8_lossy(&packet[12..]);
        assert!(text.contains("_minecraft"));
        assert!(text.contains("_tcp"));
    }

    #[test]
    fn srv_response_parses_target_and_port() {
        // 构造一个最小 SRV 响应（answer 带压缩指针指向 question name）。
        let mut packet = Vec::new();
        packet.extend_from_slice(&0x1234u16.to_be_bytes());
        packet.extend_from_slice(&0x8180u16.to_be_bytes());
        packet.extend_from_slice(&1u16.to_be_bytes()); // QD
        packet.extend_from_slice(&1u16.to_be_bytes()); // AN
        packet.extend_from_slice(&0u16.to_be_bytes());
        packet.extend_from_slice(&0u16.to_be_bytes());
        // question: _minecraft._tcp.example.com SRV IN
        for label in ["_minecraft", "_tcp", "example", "com"] {
            packet.push(label.len() as u8);
            packet.extend_from_slice(label.as_bytes());
        }
        packet.push(0);
        packet.extend_from_slice(&33u16.to_be_bytes());
        packet.extend_from_slice(&1u16.to_be_bytes());
        // answer: name pointer 0xC00C, SRV IN, ttl, rdlen=...
        packet.extend_from_slice(&[0xC0, 0x0C]);
        packet.extend_from_slice(&33u16.to_be_bytes());
        packet.extend_from_slice(&1u16.to_be_bytes());
        packet.extend_from_slice(&60u32.to_be_bytes());
        // rdata: priority 0, weight 0, port 25566, target play.example.com
        let target = "play.example.com";
        let rdlen = 6 + target.len() + 1;
        packet.extend_from_slice(&(rdlen as u16).to_be_bytes());
        packet.extend_from_slice(&0u16.to_be_bytes());
        packet.extend_from_slice(&0u16.to_be_bytes());
        packet.extend_from_slice(&25566u16.to_be_bytes());
        for label in target.split('.') {
            packet.push(label.len() as u8);
            packet.extend_from_slice(label.as_bytes());
        }
        packet.push(0);

        let (target_name, port) = parse_srv_response(&packet).expect("parse");
        assert_eq!(target_name, "play.example.com");
        assert_eq!(port, 25566);
    }

    #[test]
    fn disconnect_classifier_distinguishes_causes() {
        assert_eq!(
            classify_disconnect("Failed to log in: Invalid session"),
            ConnectionClassification::InvalidSession
        );
        assert_eq!(
            classify_disconnect("You are not whitelisted on this server"),
            ConnectionClassification::ServerWhitelisted
        );
        assert_eq!(
            classify_disconnect("Outdated client! Please use 1.20.1"),
            ConnectionClassification::VersionMismatch
        );
        assert_eq!(
            classify_disconnect("Connection refused: no further information"),
            ConnectionClassification::TcpRefused
        );
        assert_eq!(
            classify_disconnect("UnknownHostException: mc.example.com"),
            ConnectionClassification::DnsFailed
        );
        assert_eq!(
            classify_disconnect("disconnect.loginFailedInfo.invalidSession"),
            ConnectionClassification::InvalidSession
        );
        assert_eq!(
            classify_disconnect(
                "org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError"
            ),
            ConnectionClassification::ClientMixinCrashDuringJoin
        );
    }
}
