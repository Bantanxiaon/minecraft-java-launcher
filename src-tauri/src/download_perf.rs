//! 下载性能可观测性：滑动窗口测速、Host Health、退避策略与诊断。

use crate::LauncherError;
use dashmap::DashMap;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tokio::sync::Semaphore;

/// 极慢源时间驱动检测阈值：连续观测达到该时长、窗口内字节极少且吞吐极低时，
/// 不要求先积累 256KB 样本即可判定慢源（1.3KB/s 下积累 256KB 需要约 197 秒）。
pub const EXTREME_SLOW_OBSERVE_SECS: u64 = 8;
pub const EXTREME_SLOW_MAX_BPS: f64 = 8.0 * 1024.0;
pub const EXTREME_SLOW_MAX_WINDOW_BYTES: u64 = 64 * 1024;

/// 滑动窗口测速器，多个 worker 写入同一个实例。
pub struct SpeedMeter {
    window: Duration,
    samples: VecDeque<(Instant, u64)>,
}

impl Default for SpeedMeter {
    fn default() -> Self {
        Self::new(Duration::from_secs(3))
    }
}

impl SpeedMeter {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            samples: VecDeque::new(),
        }
    }

    pub fn record(&mut self, bytes: u64) {
        let now = Instant::now();
        self.samples.push_back((now, bytes));
        self.trim(now);
    }

    fn trim(&mut self, now: Instant) {
        while let Some((at, _)) = self.samples.front() {
            if now.duration_since(*at) > self.window {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn bytes_per_second(&mut self) -> f64 {
        let now = Instant::now();
        self.trim(now);
        let Some((first, _)) = self.samples.front() else {
            return 0.0;
        };
        let elapsed = now.duration_since(*first).as_secs_f64().max(0.1);
        let bytes: u64 = self.samples.iter().map(|(_, bytes)| *bytes).sum();
        bytes as f64 / elapsed
    }

    /// 首个样本到当前时刻的连续观测时长；无样本时为 0。
    pub fn observed_span(&mut self) -> Duration {
        let now = Instant::now();
        self.trim(now);
        match self.samples.front() {
            Some((first, _)) => now.saturating_duration_since(*first),
            None => Duration::ZERO,
        }
    }

    /// 当前窗口内的字节总量。
    pub fn window_bytes(&mut self) -> u64 {
        let now = Instant::now();
        self.trim(now);
        self.samples.iter().map(|(_, bytes)| *bytes).sum()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStats {
    pub host: String,
    pub requests: u64,
    pub success: u64,
    pub failure: u64,
    pub bytes: u64,
    pub last_success_at_ms: Option<i64>,
    pub last_failure_at_ms: Option<i64>,
}

pub fn host_health() -> &'static DashMap<String, HostStats> {
    static HEALTH: OnceLock<DashMap<String, HostStats>> = OnceLock::new();
    HEALTH.get_or_init(DashMap::new)
}

pub fn record_host_request(host: &str, success: bool, bytes: u64) {
    let mut entry = host_health()
        .entry(host.to_string())
        .or_insert_with(|| HostStats {
            host: host.to_string(),
            ..Default::default()
        });
    entry.requests += 1;
    entry.bytes = entry.bytes.saturating_add(bytes);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    if success {
        entry.success += 1;
        entry.last_success_at_ms = Some(now);
    } else {
        entry.failure += 1;
        entry.last_failure_at_ms = Some(now);
    }
}

static HOST_SPEEDS: OnceLock<DashMap<String, Arc<std::sync::Mutex<SpeedMeter>>>> = OnceLock::new();

pub fn record_host_bytes(host: &str, bytes: u64) {
    let meter = host_speeds()
        .entry(host.to_string())
        .or_insert_with(|| {
            Arc::new(std::sync::Mutex::new(SpeedMeter::new(Duration::from_secs(
                12,
            ))))
        })
        .clone();
    let lock = meter.lock();
    if let Ok(mut inner) = lock {
        inner.record(bytes);
    }
}

fn host_speeds() -> &'static DashMap<String, Arc<std::sync::Mutex<SpeedMeter>>> {
    HOST_SPEEDS.get_or_init(DashMap::new)
}

pub fn host_recent_speed(host: &str) -> f64 {
    host_speeds()
        .get(host)
        .and_then(|meter| meter.lock().ok().map(|mut inner| inner.bytes_per_second()))
        .unwrap_or(0.0)
}

/// 极慢源时间驱动判定（纯函数，便于注入时间测试）：
/// 连续观测达到阈值时长、窗口字节极少且吞吐极低，不要求先积累 256KB。
pub fn is_extreme_slow(span: Duration, window_bytes: u64, speed: f64) -> bool {
    span >= Duration::from_secs(EXTREME_SLOW_OBSERVE_SECS)
        && window_bytes > 0
        && window_bytes < EXTREME_SLOW_MAX_WINDOW_BYTES
        && speed > 0.0
        && speed < EXTREME_SLOW_MAX_BPS
}

/// 来源是否被判定为“慢”：
/// 1. 近期失败率过高（≥3 次请求且失败 ≥2/3）；
/// 2. 极慢源时间驱动判定：连续观测 ≥8s、窗口字节 <64KB 且吞吐 <8KB/s，
///    不要求先积累 256KB 样本；
/// 3. 常规慢源判定：累计样本 ≥256KB 且近期窗口吞吐 <64KB/s。
pub fn host_is_slow(host: &str) -> bool {
    if let Some(stats) = host_health().get(host).map(|entry| entry.clone()) {
        if stats.requests >= 3 && stats.success.saturating_mul(2) < stats.requests {
            return true;
        }
    }
    if let Some(meter) = host_speeds().get(host) {
        if let Ok(mut inner) = meter.lock() {
            let span = inner.observed_span();
            let window_bytes = inner.window_bytes();
            let speed = inner.bytes_per_second();
            if is_extreme_slow(span, window_bytes, speed) {
                return true;
            }
        }
    }
    if let Some(stats) = host_health().get(host).map(|entry| entry.clone()) {
        if stats.bytes >= 256 * 1024 {
            let speed = host_recent_speed(host);
            if speed > 0.0 && speed < 64.0 * 1024.0 {
                return true;
            }
        }
    }
    false
}

/// 带抖动的指数退避，避免多个 worker 同时重试。
pub fn retry_delay(attempt: u32) -> Duration {
    let base = 300u64.saturating_mul(1u64 << attempt.min(5));
    let jitter = unique_jitter();
    Duration::from_millis(base.saturating_add(jitter % 250))
}

fn unique_jitter() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let value = COUNTER.fetch_add(1, Ordering::Relaxed);
    value.wrapping_mul(1_103_515_245).wrapping_add(12_345) % 251
}

static TOTAL_NETWORK_BYTES: AtomicU64 = AtomicU64::new(0);

pub fn record_network_bytes(bytes: u64) {
    TOTAL_NETWORK_BYTES.fetch_add(bytes, Ordering::Relaxed);
}

/// 按资源类型分级并发：metadata 不堵下载，大文件不占满所有连接。
pub struct DownloadConcurrency {
    pub metadata: Arc<Semaphore>,
    pub small: Arc<Semaphore>,
    pub library: Arc<Semaphore>,
    pub large: Arc<Semaphore>,
}

impl DownloadConcurrency {
    pub fn new() -> Self {
        Self {
            metadata: Arc::new(Semaphore::new(6)),
            small: Arc::new(Semaphore::new(16)),
            library: Arc::new(Semaphore::new(12)),
            large: Arc::new(Semaphore::new(4)),
        }
    }
}

pub fn download_concurrency() -> &'static DownloadConcurrency {
    static CONCURRENCY: OnceLock<DownloadConcurrency> = OnceLock::new();
    CONCURRENCY.get_or_init(DownloadConcurrency::new)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadDiagnostics {
    pub total_network_bytes: u64,
    pub hosts: Vec<HostStats>,
}

#[tauri::command]
pub fn download_diagnostics(_app: AppHandle) -> Result<DownloadDiagnostics, LauncherError> {
    let mut hosts = host_health()
        .iter()
        .map(|entry| entry.value().clone())
        .collect::<Vec<_>>();
    hosts.sort_by_key(|host| std::cmp::Reverse(host.requests));
    Ok(DownloadDiagnostics {
        total_network_bytes: TOTAL_NETWORK_BYTES.load(Ordering::Relaxed),
        hosts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_meter_aggregates_multiple_workers() {
        let mut meter = SpeedMeter::new(Duration::from_secs(3));
        meter.record(1_000_000);
        meter.record(1_000_000);
        let speed = meter.bytes_per_second();
        assert!(
            speed >= 2_000_000.0,
            "speed should aggregate both samples: {speed}"
        );
    }

    #[test]
    fn retry_delay_grows_and_has_jitter() {
        let first = retry_delay(0);
        let second = retry_delay(2);
        assert!(second > first);
        assert!(retry_delay(10).as_millis() <= 300 * 32 + 250);
    }

    #[test]
    fn concurrency_classes_have_independent_limits() {
        let concurrency = download_concurrency();
        assert_eq!(concurrency.metadata.available_permits(), 6);
        assert_eq!(concurrency.small.available_permits(), 16);
        assert_eq!(concurrency.library.available_permits(), 12);
        assert_eq!(concurrency.large.available_permits(), 4);
    }

    #[test]
    fn slow_and_failing_hosts_are_detected() {
        let host = "slow-host.test";
        host_health().remove(host);
        host_speeds().remove(host);
        // 高失败率：3 次请求 2 次失败。
        record_host_request(host, true, 0);
        record_host_request(host, false, 0);
        record_host_request(host, false, 0);
        assert!(host_is_slow(host), "失败率过高应判定为慢来源");
        host_health().remove(host);
        // 低吞吐：累计样本足够但近 3 秒窗口只有极小流量。
        record_host_request(host, true, 1024 * 1024);
        record_host_bytes(host, 1024);
        assert!(host_is_slow(host), "吞吐过低应判定为慢来源");
    }

    #[test]
    fn extreme_slow_host_detected_by_time_without_256kb_sample() {
        // 1.3KB/s 持续 10 秒：约 13KB 样本，远低于 256KB，也必须判定为极慢。
        assert!(is_extreme_slow(
            Duration::from_secs(10),
            13 * 1024,
            1.3 * 1024.0
        ));
    }

    #[test]
    fn healthy_burst_after_slow_start_is_not_flagged() {
        // 短暂慢启动后立即爆发：窗口字节超过阈值，或速度高于阈值，不应误判。
        assert!(!is_extreme_slow(
            Duration::from_secs(10),
            128 * 1024,
            12.8 * 1024.0
        ));
        assert!(!is_extreme_slow(
            Duration::from_secs(4),
            8 * 1024,
            2.0 * 1024.0
        ));
        assert!(!is_extreme_slow(Duration::from_secs(10), 0, 0.0));
    }
}
