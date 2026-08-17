//! 更新安装包多源下载：GitHub 直链 + 国内代理镜像，带速度检测与自动切换。
//! 下载完成后执行与官方 tauri-updater 插件相同的 minisign 签名校验，
//! 签名通过后才交给插件执行安装。任何绕过签名的路径都是不允许的。

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use futures_util::StreamExt;
use minisign_verify::{PublicKey, Signature};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

const USER_AGENT: &str = "SHLauncher/0.9.5";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 连续多久没有收到任何字节就判定卡死并切换源。
const NO_PROGRESS_ABORT: Duration = Duration::from_secs(20);
/// 连续观测窗口：极慢源在窗口内收到的字节数不足就切换源。
/// 阈值按平均 64KB/s 设定（8 秒需收到 512KB），低于此速度的源会立即切换，
/// 避免在蜗牛源上干等几分钟。
const EXTREME_SLOW_WINDOW: Duration = Duration::from_secs(8);
const EXTREME_SLOW_BYTES: u64 = 512 * 1024;
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(500);

/// 从 tauri.conf.json 读取 updater 公钥（与插件启动时读取的是同一份配置）。
fn updater_pubkey() -> Result<String, String> {
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .map_err(|error| format!("读取更新配置失败：{error}"))?;
    config
        .pointer("/plugins/updater/pubkey")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| "缺少 updater 公钥配置".to_string())
}

/// 生成候选下载源：官方直链优先，其余为国内代理镜像，顺序即优先级。
pub(crate) fn update_mirror_candidates(download_url: &str) -> Vec<String> {
    let mut candidates = vec![download_url.to_string()];
    for prefix in [
        "https://ghproxy.net/",
        "https://gh-proxy.com/",
        "https://ghfast.top/",
    ] {
        let candidate = format!("{prefix}{download_url}");
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

/// 与 tauri-plugin-updater 内部 `verify_signature` 相同语义：
/// base64 解码公钥/签名文本 → minisign 校验（含 trusted comment）。
fn verify_update_signature(
    data: &[u8],
    signature_b64: &str,
    pubkey_b64: &str,
) -> Result<(), String> {
    let pubkey_text = String::from_utf8(
        BASE64
            .decode(pubkey_b64)
            .map_err(|error| format!("公钥解码失败：{error}"))?,
    )
    .map_err(|_| "公钥不是有效文本".to_string())?;
    let public_key =
        PublicKey::decode(&pubkey_text).map_err(|error| format!("公钥解析失败：{error}"))?;
    let signature_text = String::from_utf8(
        BASE64
            .decode(signature_b64)
            .map_err(|error| format!("签名解码失败：{error}"))?,
    )
    .map_err(|_| "签名不是有效文本".to_string())?;
    let signature =
        Signature::decode(&signature_text).map_err(|error| format!("签名解析失败：{error}"))?;
    public_key
        .verify(data, &signature, true)
        .map_err(|error| format!("更新签名校验失败：{error}"))?;
    Ok(())
}

async fn download_candidate(
    client: &reqwest::Client,
    app: &AppHandle,
    url: &str,
) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("请求失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let total = response.content_length().unwrap_or(0);
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    let mut window_start = Instant::now();
    let mut window_bytes = 0u64;
    let mut last_data_at = Instant::now();
    let mut last_emit_at = Instant::now();
    let mut meter = crate::download_perf::SpeedMeter::new(Duration::from_secs(3));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("读取数据失败：{error}"))?;
        bytes.extend_from_slice(&chunk);
        let now = Instant::now();
        meter.record(chunk.len() as u64);
        window_bytes += chunk.len() as u64;
        last_data_at = now;
        if now.duration_since(window_start) >= EXTREME_SLOW_WINDOW {
            if window_bytes < EXTREME_SLOW_BYTES {
                return Err(format!(
                    "极慢源：{} 秒仅收到 {} 字节，切换源",
                    EXTREME_SLOW_WINDOW.as_secs(),
                    window_bytes
                ));
            }
            window_start = now;
            window_bytes = 0;
        }
        if now.duration_since(last_emit_at) >= PROGRESS_EMIT_INTERVAL {
            let _ = app.emit(
                "update-progress",
                serde_json::json!({
                    "downloaded": bytes.len(),
                    "total": total,
                    "speed": meter.bytes_per_second(),
                    "url": url,
                }),
            );
            last_emit_at = now;
        }
    }
    if last_data_at.elapsed() >= NO_PROGRESS_ABORT {
        return Err("下载超过 20 秒没有收到数据，切换源".to_string());
    }
    let _ = app.emit(
        "update-progress",
        serde_json::json!({
            "downloaded": bytes.len(),
            "total": total,
            "speed": 0.0,
            "url": url,
            "done": true,
        }),
    );
    Ok(bytes)
}

/// 检查更新并以多源下载 + 签名校验 + 插件安装的完整流程执行更新。
/// 返回 `Ok(None)` 表示当前已是最新版；安装器启动后进程会交给安装程序接管。
#[tauri::command]
pub(crate) async fn install_update_fast(app: AppHandle) -> Result<Option<String>, String> {
    let updater = app
        .updater()
        .map_err(|error| format!("更新器初始化失败：{error}"))?;
    let update = updater
        .check()
        .await
        .map_err(|error| format!("检查更新失败：{error}"))?;
    let Some(update) = update else {
        return Ok(None);
    };

    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("HTTP 客户端创建失败：{error}"))?;

    let candidates = update_mirror_candidates(update.download_url.as_str());
    let mut bytes = None;
    let mut last_error = String::new();
    for (index, url) in candidates.iter().enumerate() {
        match download_candidate(&client, &app, url).await {
            Ok(data) => {
                bytes = Some(data);
                break;
            }
            Err(error) => {
                last_error = format!("{url}：{error}");
                log::warn!("update source {} failed: {}", index + 1, last_error);
                let host = url::Url::parse(url)
                    .ok()
                    .and_then(|parsed| parsed.host_str().map(str::to_string))
                    .unwrap_or_else(|| "unknown".to_string());
                crate::download_perf::record_host_request(&host, false, 0);
                if index + 1 < candidates.len() {
                    let _ = app.emit(
                        "update-source-fallback",
                        serde_json::json!({ "source": url, "error": error }),
                    );
                }
            }
        }
    }
    let bytes = bytes.ok_or_else(|| format!("所有下载源均失败，最后错误：{last_error}"))?;

    let pubkey = updater_pubkey()?;
    verify_update_signature(&bytes, &update.signature, &pubkey)?;
    // 签名校验通过后才允许安装（Windows 上会写临时安装包并启动安装器，随后进程退出）。
    update
        .install(&bytes)
        .map_err(|error| format!("启动安装程序失败：{error}"))?;
    Ok(Some(update.version.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_candidates_keep_primary_first_and_no_duplicates() {
        let url = "https://github.com/Bantanxiaon/minecraft-java-launcher/releases/download/v0.9.0/SH._0.9.0_x64-setup.exe";
        let candidates = update_mirror_candidates(url);
        assert_eq!(candidates.len(), 4);
        assert_eq!(candidates[0], url);
        assert!(candidates[1].starts_with("https://ghproxy.net/"));
        assert!(candidates[2].starts_with("https://gh-proxy.com/"));
        assert!(candidates[3].starts_with("https://ghfast.top/"));
        assert!(candidates.iter().all(|c| c.starts_with("https://")));
    }

    #[test]
    fn updater_pubkey_config_is_present_and_decodable() {
        let pubkey = updater_pubkey().expect("pubkey 必须可读取");
        let decoded = BASE64.decode(&pubkey).expect("pubkey 必须是合法 base64");
        let text = String::from_utf8(decoded).expect("pubkey 解码后必须是文本");
        assert!(text.contains("minisign public key"));
    }

    #[test]
    fn signature_verification_accepts_valid_and_rejects_tampered() {
        // 使用 minisign-verify 官方测试向量，语义与 tauri-updater 完全一致。
        let pubkey_text = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        let signature_text = "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";
        let pubkey_b64 = BASE64.encode(pubkey_text.as_bytes());
        let signature_b64 = BASE64.encode(signature_text.as_bytes());
        verify_update_signature(b"test", &signature_b64, &pubkey_b64).expect("合法签名必须通过");
        verify_update_signature(b"Test", &signature_b64, &pubkey_b64)
            .expect_err("被篡改的数据必须被拒绝");
    }

    #[test]
    fn signature_verification_rejects_garbage() {
        assert!(verify_update_signature(b"x", "!!!", "!!!").is_err());
    }

    /// 真实网络回归：用已发布 v0.9.0 的安装包与签名验证“内置公钥 + 下载字节”链路。
    #[tokio::test]
    #[ignore = "network"]
    async fn real_signed_installer_verifies_against_embedded_pubkey() {
        const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRNlEzRFNJdU9Sa0N6blo5eXd4V0dXdjdXdlhLNlliNUlwNS9jRzRuaXQxNWQxTWJpSjdDeWRRTG5LQ1VwUEg4TW1RSkU0RTR0OTZZNVlsT3RyRWl3eDllcVdBa3NudXc4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzg2ODk5MDY1CWZpbGU6U0jlkK/liqjlmahfMC45LjBfeDY0LXNldHVwLmV4ZQpZeXZYWDZhNDVEVGg5Yzl2LytzK0FpaFhVRVRuVlRwdStmd2dqUmlrd0JyZnRnNmlzcGIrRWVhYUZHUnpOa2FYVnNsRjFlQkNjOVpxclBTa0M3ZU1BUT09Cg==";
        let url =
            "https://github.com/Bantanxiaon/minecraft-java-launcher/releases/download/v0.9.0/SH._0.9.0_x64-setup.exe";
        let client = reqwest::Client::builder()
            .user_agent("SHLauncher-update-test")
            .build()
            .expect("client");
        let bytes = client
            .get(url)
            .send()
            .await
            .expect("下载失败")
            .bytes()
            .await
            .expect("读取失败")
            .to_vec();
        assert!(bytes.len() > 4_000_000, "安装包大小异常");
        let pubkey = updater_pubkey().expect("pubkey");
        verify_update_signature(&bytes, SIGNATURE, &pubkey).expect("真实签名必须通过");
    }
}
