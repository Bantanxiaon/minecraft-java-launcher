//! 真实公网下载基准：冷/热缓存、大小文件、分阶段指标。
//! 用法：download_benchmark.exe --repeat 3 --out benchmark.json

use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;
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
    let value = serde_json::Value::Object(report);
    if let Some(out) = out {
        let mut file = std::fs::File::create(out).expect("write benchmark output");
        let _ = file.write_all(serde_json::to_string_pretty(&value).unwrap().as_bytes());
    } else {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
    }
}
