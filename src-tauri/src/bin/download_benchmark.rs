//! 真实公网下载基准：冷/热缓存、大小文件、多小文件并发 A/B（串行 vs 分级并发）。
//! 用法：download_benchmark.exe --repeat 3 --concurrency 16 --out benchmark.json

use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Serialize)]
struct RunStats {
    url: String,
    bytes: u64,
    ttfb_ms: f64,
    total_ms: f64,
    bytes_per_second: f64,
    cache_hit: bool,
}

#[derive(Serialize)]
struct AggregateStats {
    file_count: usize,
    bytes: u64,
    total_ms: f64,
    bytes_per_second: f64,
    warm_reuse: bool,
}

fn sha1_bytes(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(bytes))
}

fn run(url: &str, target: &PathBuf) -> Result<RunStats, String> {
    let started = Instant::now();
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .user_agent(format!(
            "SHLauncher/{}+benchmark",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|error| error.to_string())?;
    let mut response = client.get(url).send().map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let ttfb = started.elapsed().as_secs_f64() * 1000.0;
    let mut file = std::fs::File::create(target).map_err(|error| error.to_string())?;
    std::io::copy(&mut response, &mut file).map_err(|error| error.to_string())?;
    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let body = std::fs::read(target).map_err(|error| error.to_string())?;
    let bytes = body.len() as u64;
    let sha1 = sha1_bytes(&body);
    let _ = sha1;
    Ok(RunStats {
        url: url.to_string(),
        bytes,
        ttfb_ms: ttfb,
        total_ms,
        bytes_per_second: bytes as f64 / (total_ms / 1000.0),
        cache_hit: false,
    })
}

fn fetch_modrinth_forge_files(count: usize) -> Result<Vec<(String, u64)>, String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .user_agent("SHLauncher/benchmark (modpack-simulation)")
        .build()
        .map_err(|error| error.to_string())?;
    let url = format!(
        "https://api.modrinth.com/v2/search?query=library&facets=%5B%5B%22project_type%3Amod%22%5D%2C%5B%22categories%3Aforge%22%5D%2C%5B%22versions%3A1.20.1%22%5D%5D&limit={count}"
    );
    let value: serde_json::Value = client
        .get(&url)
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let hits = value
        .get("hits")
        .and_then(|entry| entry.as_array())
        .ok_or_else(|| "Modrinth 搜索响应缺少 hits".to_string())?;
    let mut files = Vec::new();
    for hit in hits.iter().take(count) {
        let Some(project_id) = hit.get("project_id").and_then(|entry| entry.as_str()) else {
            continue;
        };
        let versions_url = format!(
            "https://api.modrinth.com/v2/project/{project_id}/version?game_versions=%5B%221.20.1%22%5D&loaders=%5B%22forge%22%5D"
        );
        let Ok(versions) = client
            .get(&versions_url)
            .send()
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.json::<serde_json::Value>())
        else {
            continue;
        };
        let Some(version_list) = versions.as_array() else {
            continue;
        };
        for version in version_list {
            let Some(primary) = version
                .get("files")
                .and_then(|entry| entry.as_array())
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| {
                            entry.get("primary").and_then(|value| value.as_bool()) == Some(true)
                        })
                        .or_else(|| entries.first())
                })
            else {
                continue;
            };
            let Some(file_url) = primary.get("url").and_then(|entry| entry.as_str()) else {
                continue;
            };
            let size = primary
                .get("size")
                .and_then(|entry| entry.as_u64())
                .unwrap_or(0);
            if file_url.starts_with("https://cdn.modrinth.com/") {
                files.push((file_url.to_string(), size));
                break;
            }
        }
    }
    if files.is_empty() {
        return Err("未能获取任何 Modrinth 模组文件".to_string());
    }
    Ok(files)
}

fn run_concurrent(
    files: &[(String, u64)],
    directory: &Path,
    concurrency: usize,
    reuse_existing: bool,
) -> Result<AggregateStats, String> {
    let started = Instant::now();
    let bytes = Arc::new(AtomicU64::new(0));
    let completed = Arc::new(AtomicU64::new(0));
    let mut handles = Vec::new();
    for (index, (url, size)) in files.iter().enumerate() {
        let directory = directory.to_path_buf();
        let url = url.clone();
        let size = *size;
        let bytes = bytes.clone();
        let completed = completed.clone();
        handles.push(std::thread::spawn(move || -> Result<(), String> {
            let target = directory.join(format!("file-{index}.jar"));
            if reuse_existing
                && target.is_file()
                && target.metadata().map(|m| m.len()).unwrap_or(0) == size
            {
                completed.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            let client = reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(300))
                .user_agent("SHLauncher/benchmark (modpack-simulation)")
                .build()
                .map_err(|error| error.to_string())?;
            let mut response = client
                .get(url)
                .send()
                .map_err(|error| error.to_string())?
                .error_for_status()
                .map_err(|error| error.to_string())?;
            let mut file = std::fs::File::create(&target).map_err(|error| error.to_string())?;
            let written =
                std::io::copy(&mut response, &mut file).map_err(|error| error.to_string())?;
            bytes.fetch_add(written, Ordering::Relaxed);
            completed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }));
        if handles.len() >= concurrency {
            for handle in handles.drain(..) {
                handle.join().map_err(|_| "线程异常".to_string())??;
            }
        }
    }
    for handle in handles {
        handle.join().map_err(|_| "线程异常".to_string())??;
    }
    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let downloaded = bytes.load(Ordering::Relaxed);
    Ok(AggregateStats {
        file_count: files.len(),
        bytes: downloaded,
        total_ms,
        bytes_per_second: downloaded as f64 / (total_ms / 1000.0),
        warm_reuse: reuse_existing,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let repeat = args
        .iter()
        .position(|arg| arg == "--repeat")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(3);
    let out = args
        .iter()
        .position(|arg| arg == "--out")
        .and_then(|index| args.get(index + 1).cloned());
    let concurrency = args
        .iter()
        .position(|arg| arg == "--concurrency")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16);
    // 真实公开下载源：Modrinth CDN、BMCLAPI 镜像、Microsoft OpenJDK。
    let urls = [
        ("modrinth-small", "https://cdn.modrinth.com/data/ordsPcFz/versions/Zsh14XeQ/kotlinforforge-4.12.0-all.jar"),
        ("bmclapi-forge", "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/1.20.1-47.4.22/forge-1.20.1-47.4.22-universal.jar"),
        ("openjdk-large", "https://aka.ms/download-jdk/microsoft-jdk-17-windows-x64.zip"),
    ];
    let temp = std::env::temp_dir().join("sh-benchmark");
    let _ = std::fs::create_dir_all(&temp);
    let mut report = serde_json::Map::new();
    for (name, url) in urls {
        let mut runs = Vec::new();
        for index in 0..repeat {
            let target = temp.join(format!("{name}-{index}.bin"));
            let _ = std::fs::remove_file(&target);
            match run(url, &target) {
                Ok(mut stats) => {
                    if index > 0 {
                        stats.cache_hit = false;
                    }
                    println!(
                        "{name} run {index}: {} bytes, {:.2} MB/s, TTFB {:.0} ms",
                        stats.bytes,
                        stats.bytes_per_second / 1024.0 / 1024.0,
                        stats.ttfb_ms
                    );
                    runs.push(stats);
                }
                Err(error) => {
                    println!("{name} run {index} failed: {error}");
                }
            }
        }
        report.insert(name.to_string(), serde_json::to_value(&runs).unwrap());
    }
    // 真实多小文件整合包负载：串行(1) vs 分级并发(16) A/B，以及热缓存复用。
    if let Ok(files) = fetch_modrinth_forge_files(16) {
        let directory = temp.join("pack-simulated");
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create benchmark directory");
        let mut many: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        for (label, workers, reuse, clear) in [
            ("serial-cold", 1usize, false, false),
            ("concurrent-cold", concurrency, false, true),
            ("concurrent-hot", concurrency, true, false),
        ] {
            if clear {
                let _ = std::fs::remove_dir_all(&directory);
                std::fs::create_dir_all(&directory).expect("recreate benchmark directory");
            }
            match run_concurrent(&files, &directory, workers, reuse) {
                Ok(stats) => {
                    println!(
                        "{label}: {} files {} bytes in {:.2}s = {:.2} MB/s (reuse={})",
                        stats.file_count,
                        stats.bytes,
                        stats.total_ms / 1000.0,
                        stats.bytes_per_second / 1024.0 / 1024.0,
                        stats.warm_reuse
                    );
                    many.insert(label.to_string(), serde_json::to_value(stats).unwrap());
                }
                Err(error) => println!("{label} failed: {error}"),
            }
        }
        report.insert("modrinth-many".to_string(), serde_json::Value::Object(many));
    } else {
        println!("modrinth-many skipped: Modrinth 搜索接口不可用");
    }
    let value = serde_json::Value::Object(report);
    if let Some(out) = out {
        let mut file = std::fs::File::create(out).expect("write benchmark output");
        let _ = file.write_all(serde_json::to_string_pretty(&value).unwrap().as_bytes());
    } else {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
    }
}
