//! 下载性能可观测性：滑动窗口测速、Host Health、退避策略与诊断。

use crate::LauncherError;
use dashmap::DashMap;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// 2~5 秒滑动窗口测速器，多个 worker 写入同一个实例。
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
}
