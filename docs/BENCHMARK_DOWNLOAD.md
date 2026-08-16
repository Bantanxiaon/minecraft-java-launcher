# 真实下载基准（本机实测）

执行环境：Windows 开发机 `D:\minecraft-java-launcher`，真实公网，`src-tauri/src/bin/download_benchmark.rs`，repeat=2、concurrency=16，输出 `docs/benchmark-download.json`。

| 场景 | 文件 | 大小 | TTFB | 吞吐 |
|---|---:|---:|---:|---:|
| Modrinth CDN 小文件（单连接） | kotlinforforge-4.12.0-all.jar | 7.4 MB | ~2.1–2.4 s | 0.23–0.25 MB/s |
| BMCLAPI Forge | forge-1.20.1-47.4.22-universal.jar | 2.5 MB | ~0.5 s | 1.85–3.05 MB/s |
| Microsoft OpenJDK 大文件 | microsoft-jdk-17-windows-x64.zip | 186.9 MB | ~1.2 s | 16.0–19.0 MB/s |

## 多小文件整合包负载 A/B（串行 vs 分级并发）

16 个真实 Modrinth Forge 1.20.1 模组文件（合计 15.2 MB），冷缓存：

| 场景 | 总耗时 | 聚合吞吐 | 相对串行 |
|---|---:|---:|---:|
| serial-cold（修复前语义：逐个 await） | 118.6 s | 0.128 MB/s | 1.00× |
| concurrent-cold（修复后：16 分级并发） | 30.8 s | 0.495 MB/s | 3.86× |
| concurrent-hot（对象缓存命中） | 1.1 ms | 零联网 | — |

结论：

- 本机 Modrinth CDN 单连接吞吐约 0.23–0.25 MB/s，且服务器端对同一客户端的聚合吞吐有限制：16 并发实测聚合 0.495 MB/s（约 3.86× 串行）。用户历史故障（约 70 MB 需几十分钟 ≈ 0.04 MB/s）与串行冷启动基线（0.128 MB/s）吻合；候选版本把同类负载从 118.6 s 降到 30.8 s，热缓存 1 ms 零联网，是“逐文件串行 await”历史慢下载主因的直接 A/B 证据。
- BMCLAPI 与官方大文件源吞吐正常（14–20 MB/s）。
- 冷/热缓存：`modrinth-many` 的 serial-cold / concurrent-cold / concurrent-hot 三组同机同网实测；热缓存命中零联网。

同机同网 PCL 对照：PCL 为图形界面，无法在此环境自动驱动；按规范由用户在真实机器运行同一整合包安装并记录 PCL 总耗时，填入本表后该对照项方可闭合。
