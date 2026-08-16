# 同机公网联机 E2E 操作手册（RELEASE_BLOCKING）

> 目标：在**当前这台 Windows 机器**上真实完成 `docs/V081_MULTIPLAYER_CHECKLIST.md`
> 第二节的全部 `RELEASE_BLOCKING` 项。全程禁止 localhost / 127.0.0.1 / LAN IP 冒充公网测试。
> 完成一项就在清单里如实勾 `[x]`，把数据填进 `docs/V081_MULTIPLAYER_E2E_REPORT.md`。

## 0. 当前环境事实（已实测）

- 唯一就绪实例：id 3 `Closing Song1.6.5`，Forge 1.20.1，277 个模组，saves 为空（需要先在游戏里新建一个世界）。
- Java 17 / 21 运行时已安装。
- 两个离线账户：Player、123。
- e4mc broker 可达（jp.e4mc.link:25575），relay map 可达。
- Modrinth CDN 下载很慢（实测约 1.3 KB/s，e4mc jar 约 1.3 MB 需 15–20 分钟）。建议先预热缓存：
  把已校验的文件 `D:/tmp/e4mc-forge-6.2.1.jar`（SHA-1 `6fc90baef39cff5f9466ddf39f4421e3e9475308`）
  复制为 `D:\MinecraftLauncherData\cache\sha1\6fc90baef39cff5f9466ddf39f4421e3e9475308`，
  启动器下载时按 SHA-1 命中对象缓存（`cache/sha1/<sha1>`），即可跳过慢速 CDN。

## 1. 准备两个隔离实例

- Host：实例 3（Forge 1.20.1）即可。
- Guest：在启动器“游戏库”新建一个**独立实例**（建议 Fabric 或 NeoForge 1.21.x，同时满足 1.21.x 覆盖）。
  两个实例必须使用不同 root，绝不复用同一实例。
- 若创建 1.21.x 实例需要联网下载 loader 文件，请预留时间。

## 2. Host：创建房间（真实公网链路）

1. 运行新版启动器（`pnpm tauri dev`，或用已打包的新版二进制）。
2. 联机页选择实例 3 → “创建房间并启动游戏”。
3. 等待状态：准备联机组件 → 安装 e4mc（首次会下载）→ 启动游戏 → 等待你进入世界。
4. 进入游戏，新建/打开单人世界，点击“对局域网开放”。
5. 观察启动器状态依次变为：`LAN_OPENED → WAITING_FOR_TUNNEL → READY`，记录：
   - session_id、instance_id、MC 版本、Loader、e4mc 版本、LAN 端口、`*.e4mc.link` 地址、time_to_ready。
6. 点击“复制邀请地址”。

> 若 60 秒后仍无地址：等待并检查联机页“查看诊断”；若最终拿不到域名，如实记录失败原因，禁止改 localhost。

## 3. Guest：通过公网域名加入

1. 在联机页“快速加入”输入 Host 的 `*.e4mc.link` 地址。
2. 选择 Guest 实例与第二个账户（如 `123`），点“启动并加入”。
3. 确认 Guest 通过公网地址进入 Host 世界（不是 localhost/LAN）。
4. 若因离线账户/整合包校验无法加入：如实记录实际结果，不得宣称成功。

## 4. 稳定性与生命周期

- 30 分钟长测：记录 disconnect / reconnect / 地址变化 / 卡顿 / provider error / 状态变化。
- 连续三轮：Create → Join → Stop，确认旧会话不影响新会话、旧地址不残留、`multiplayer_history` 正常记录。
- UI 关闭测试：READY 且 Guest 已加入后关闭启动器主窗口，确认 Host Minecraft 不退出、Guest 不掉线、tunnel 继续；
  随后正常退出 Host 世界/游戏，确认后台进程退出、状态 CLOSED、历史落盘。
- Crash 清理：用任务管理器（或 `taskkill /PID <pid> /T /F`）强制结束 Host 游戏进程，
  确认启动器状态变为 CLOSED/ERROR、地址被清除、UI 不停留 READY、历史写入了退出原因。

## 5. 版本 / Loader 覆盖

- 1.20.x：实例 3（Forge）。
- 1.21.x：第 1 步新建的 Guest 实例（Fabric/NeoForge，以 e4mc 当前真实可用版本为准）。
- 其余 Loader：按环境允许做一轮 smoke（Create → LAN → 公网地址 → Join → Stop）；无法运行的写明 `LOCAL_ENV_PENDING` 原因。

## 6. 回填

1. 把每轮实测数据写入 `docs/V081_MULTIPLAYER_E2E_REPORT.md`。
2. 勾选 `docs/V081_MULTIPLAYER_CHECKLIST.md` 第二节对应项。
3. 运行 `node scripts/release-gate.mjs` 确认联机部分无 `[ ]`（主清单 4 项外部项仍会阻塞最终发布，属预期）。
