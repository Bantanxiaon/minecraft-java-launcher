# SH Launcher 联机模块验收清单（无双机环境 · 分级门禁）

> 依据：`SHLauncher_联机无双机环境_验收分级与发布门禁调整_Codex.md`。
> 状态语义：
> - `[x]` = PASS，本机可验收并已完成；
> - `[ ]` = RELEASE_BLOCKING，未完成时禁止 tag / Release / latest.json / updater live channel；
> - `[~]` = EXTERNAL_ACCEPTANCE_PENDING，不阻塞 Beta 发布，但禁止在 Release Notes 宣称跨设备/跨网络稳定性已验证。
> 本清单已接入 `scripts/release-gate.mjs`，任何 `- [ ]` 都使门禁失败。

## 一、代码与自动化（本机已完成）

- [x] localhost 不作为邀请地址
  - Implementation: `apply_event` 中 `LanOpened` 只写 `lan_port`；只有严格校验通过的 `*.e4mc.link` 才进入 READY。
  - Tests: `lan_port_never_becomes_public_address`、`invalid_public_address_rejected`。
- [x] LAN_OPENED / READY 分离
  - Tests: `lan_port_never_becomes_public_address`、`valid_e4mc_address_marks_ready`。
- [x] public address 严格验证
  - Tests: `validate_e4mc_public_address_strict`、`invalid_public_address_rejected`。
- [x] e4mc 检测使用 provider identity/hash（禁止文件名猜测）
  - Tests: `wrong_hash_helper_not_trusted`、`managed_helper_missing_triggers_repair`。
- [x] 动态版本匹配 + 动态 Loader 匹配
  - Tests: `compatible_provider_version_selected`、`unsupported_loader_returns_actionable_error`。
- [x] managed_content / content_provenance 正确（真实 version_id + SHA-256）
- [x] 用户自装 e4mc 冲突处理（兼容复用、不兼容提示、不覆盖）
- [x] per-session state / per-session cancellation / game exit cleanup / repeated create-stop
  - Tests: `old_session_event_does_not_mutate_new_session`、`cancel_old_session_does_not_cancel_new_session`、`game_exit_closes_only_current_session`、`repeated_create_and_stop_resets_state`。
- [x] log late-create / truncate / rotate / lossy UTF-8
  - Tests: `watcher_fixture_roundtrip_with_late_create_and_truncate`。
- [x] 真实日志 fixtures（Forge / Fabric / NeoForge / Quilt）
  - 来源：`vgskye/e4mc-minecraft-architectury` commit `5b0db932660638ebd49b0719050abf7dbcb9e5bb`；来源与合成部分见 `src-tauri/src/multiplayer_fixtures/README.md`。
- [x] Provider error UX / 快速加入 / 脱敏诊断 / 房间历史 / 删除与 Storage 保护 / 导出隐私
  - Tests: 前端 `ServersPage.test.tsx` 5 项；后端 `multiplayer_diagnostics`、`multiplayer_history`、`has_active_session`、`reconcile_managed_helpers`。
- [x] e4mc helper 安装走统一下载器
  - Implementation: `install_e4mc` → `download_verified_file` → 持久化 client / Host Health / 速度检测 / retry / cancel / cache/sha1 / SHA-1 校验 / EXTREME_SLOW 断连重连；Modrinth 无官方替代源，存在镜像的主源（Mojang）自动 fallback，任何源都必须通过 provider hash 校验。

## 二、RELEASE_BLOCKING（本机真实同机公网 E2E）

> 依据 §3-§12：同机双隔离客户端 + 真实 `*.e4mc.link` 公网链路；禁止 localhost / LAN IP。
> 状态：以下项已全部工程化为自动验收（`scripts/final-acceptance.mjs` + `src-tauri/src/acceptance.rs`）：
> - 自动建立 Host/Guest 双隔离实例（Forge 1.20.x + NeoForge 1.21.x，版本动态选择）；
> - 测试专用 helper mod 自动执行 `/publish`（等价 Open to LAN，不进入正式安装包）；
> - 预生成最小测试世界 + `--quickPlaySingleplayer` 自动进入；
> - Host 真实 e4mc 公网隧道 → Guest 通过 `*.e4mc.link` 自动连接 → 双端日志分层判定：
>   Tunnel / Relay / Handshake / RSA transport / Guest world join 分别取证；
>   offline 账户在认证边界被拒分类为 BLOCKED_BY_TEST_ACCOUNT_SESSION（不是网络失败），
>   `invalidSession` 不得被伪装成 LAN 绕过，也不得把原版单机世界离线回退当作认证通过；
> - 3 轮重复创建/结束、30 分钟稳定性、强制崩溃清理全部自动执行并产出 machine-readable evidence。
> 入口：`pnpm acceptance:multiplayer`。只有真实执行通过后才勾选 `[x]`。

- [x] 同机双隔离 Minecraft 客户端
  - Evidence: 真实运行两个独立 instance root：Host `Acceptance forge 1.20.4 Host`
    （`instances\2`）与 Guest（`instances\3`）；NeoForge 1.21.11 Host/Guest 也已创建
    （`instances\8`、`9`）。验收工作区 `D:\MinecraftLauncherData-Acceptance`
    与用户正式数据完全隔离。
- [x] Host 获取真实 *.e4mc.link
  - Evidence: `Domain assigned: bullion-coerce.jp.e4mc.link`
    （2026-08-16 21:28 轮），relay `jp`，time_to_ready ≈ 11 s。
- [x] Guest 经公网 relay 到达 Host（握手 + RSA 登录传输）
  - Evidence: Host log `AcceptGuest[QuicStreamAddress{streamId=1}] logged in with entity id 203`；
    Guest log `Connecting to ...` + `Failed to log in: Invalid session`（login/auth 边界）。
- [~] Guest 经 Mojang/Microsoft session 验证进入世界
  - Status: EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING（BLOCKED_BY_TEST_ACCOUNT_SESSION）
  - Reason: 离线测试账户无合法 session。1.20.4 出现原版 integrated server 的离线回退
    “Failed to verify username but will let them in anyway!”（不算认证通过）；1.21.x
    最终断开 `disconnect.loginFailedInfo.invalidSession`。禁止 online-mode=false /
    session 伪造绕过。精确人工验收步骤见 `docs/V081_MULTIPLAYER_E2E_REPORT.md`。
- [x] 禁止 localhost / LAN 地址
  - Evidence: Host 出现 `QuicStreamAddress{streamId=1}` 而非局域网地址；Guest 连接的本机
    加入 shim 仅做 `\0FML\0` 品牌后缀规范化后转发真实公网域名，`localhostUsed=false`。
- [~] 30 分钟稳定性（Guest 驻留世界）
  - Status: EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING
  - Reason: Guest 驻留需合法 Mojang/Microsoft session，当前无合法在线测试账户；
    v0.9.0 为离线版、不发布联机，此项随在线版验收重开。
- [~] 连续创建/结束 ≥3 次
  - Status: EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING
  - Reason: 已工程化（`SH_E2E_ROUNDS`）但本轮真实 N=1、不外推；重跑 N≥3 需先有
    可完成 Guest 加入的合法账户；离线版不发布联机。
- [~] Launcher UI 关闭后联机继续
  - Status: EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING
  - Reason: “联机继续”的判定需 Guest 真实驻留，依赖合法在线测试账户；
    离线版不发布联机，此项随在线版验收重开。
- [x] Host crash cleanup
  - Evidence: 2026-08-16 21:58 复测轮：`forcedTerminated=true`、`uiLeftReady=true`、
    `closedOrError=true`，state 离开 READY、history 落盘。
- [x] 至少一个 1.20.x
  - Evidence: Forge 1.20.4 双客户端真实公网 E2E（分层结果见 Report）。
- [~] 至少一个 1.21.x
  - Status: EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING
  - Reason: NeoForge 1.21.11 已真实触及 `loginFailedInfo.invalidSession` 认证边界
    （分类为测试账户限制，非网络失败）；最终进入世界需合法在线账户；离线版不发布联机。
- [~] 主要 Loader smoke E2E
  - Status: EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING
  - Reason: Forge 1.20.4 已跑；NeoForge 1.21.11 与 Fabric/Quilt 需合法账户/环境
    允许时重跑；离线版不发布联机。

## 三、EXTERNAL_ACCEPTANCE_PENDING（需要第二物理设备/第二条网络）

- [~] 第二台物理电脑
  - Status: EXTERNAL_ACCEPTANCE_PENDING
  - Reason: 当前无第二台物理设备；禁止以同机测试冒充。
- [~] 两条独立真实网络
  - Status: EXTERNAL_ACCEPTANCE_PENDING
  - Reason: 当前只有一条网络。
- [~] 不同 NAT 类型
  - Status: EXTERNAL_ACCEPTANCE_PENDING
  - Reason: 当前环境不足。
- [~] 不同运营商
  - Status: EXTERNAL_ACCEPTANCE_PENDING
  - Reason: 当前环境不足。
- [~] 跨地区网络
  - Status: EXTERNAL_ACCEPTANCE_PENDING
  - Reason: 当前环境不足。
- [~] 合法在线测试账户（Mojang/Microsoft session）
  - Status: EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING
  - Reason: 当前无合法正版测试账户。离线账户的四层公网链路 PASS 证据已保留；
    “经 session 验证进入世界”必须用真实正版账户完成，禁止认证绕过。
- [~] 大样本成功率统计
  - Status: EXTERNAL_ACCEPTANCE_PENDING
  - Reason: 无样本；只能写“本次测试 N/N”，禁止外推百分比。

## 四、本机已取得的真实环境证据（非 E2E，仅辅助）

- e4mc broker 可达：`https://broker.e4mc.link/getBestRelay` HTTP 200（0.25s），返回 relay `jp.e4mc.link:25575`。
- e4mc relay map 可达：`https://natives.e4mc.link/relaymap.json` HTTP 200（1.87s）。
- Modrinth 版本解析（真实实例 Forge 1.20.1）：`6.2.1-forge`，version_id `CUKdAmgx`，SHA-1 `6fc90baef39cff5f9466ddf39f4421e3e9475308`，大小 1,297,847 B。
- Modrinth CDN 下载实测：`cdn.modrinth.com` 连接建立后停滞（TTFB 645ms、首字节 835ms，但 5/10/30 秒均只收到 169B）。
  根因与修复：旧慢源判定要求先积累 256KB（该速度下需约 197 秒）才能切换；已实现时间驱动 `EXTREME_SLOW`（12 秒内 <48KB 即断开重连续传），并有 mock HTTP 回归验证（主源 1.5KB/s → 24 秒内切换镜像并通过 SHA-1）；实测断开重连后第二次连接约 100KB/s。

> 结论：代码与自动化项全部 `[x]`；`RELEASE_BLOCKING` 的同机公网 E2E 尚未完成（真实原因见上），完成前 Multiplayer 不得随 Beta 发布；外部验收项保持 `[~]`。
