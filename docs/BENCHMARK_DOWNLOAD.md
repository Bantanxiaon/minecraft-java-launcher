# 真实下载基准（本机实测）

执行环境：Windows 开发机 `D:\minecraft-java-launcher`，真实公网，`src-tauri/src/bin/download_benchmark.rs`，repeat=2，输出 `docs/benchmark-download.json`。

| 场景 | 文件 | 大小 | TTFB | 吞吐 |
|---|---:|---:|---:|---:|
| Modrinth CDN 小文件 | kotlinforforge-4.12.0-all.jar | 7.4 MB | ~1.9–2.3 s | 0.28–0.42 MB/s |
| BMCLAPI Forge | forge-1.20.1-47.4.22-universal.jar | 2.5 MB | ~0.5–0.6 s | 2.5–3.2 MB/s |
| Microsoft OpenJDK 大文件 | microsoft-jdk-17-windows-x64.zip | 186.9 MB | ~1.1–1.2 s | 14.7–20.4 MB/s |

结论：

- 本机 Modrinth CDN 单连接吞吐约 0.28–0.42 MB/s，是“模组下载慢”的主要网络瓶颈；启动器通过 16 并发 + 连接复用 + 对象缓存（热缓存零联网）缓解，小文件聚合吞吐 ≈ 单连接吞吐 × 并发。
- BMCLAPI 与官方大文件源吞吐正常（14–20 MB/s）。
- 冷/热缓存：`docs/benchmark-download.json` 记录 run 0（冷）与 run 1（热）数值；对象缓存命中时第二次为零联网（逻辑由 `reuse_object_cache` 覆盖，需在 GUI 场景验证）。

同机同网 PCL 对照：PCL 为图形界面，无法在此环境自动驱动；按规范由用户在真实机器运行同一整合包安装并记录 PCL 总耗时，填入本表后该对照项方可闭合。
