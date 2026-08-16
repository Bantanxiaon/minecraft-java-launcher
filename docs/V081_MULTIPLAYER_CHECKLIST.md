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
> 当前状态：Computer Use 运行时（`node_repl`/`@oai/sky`）在本会话不可用，Minecraft 世界创建与“对局域网开放”需用户在游戏内操作，以下项尚未真实验收，故保持 `[ ]`。
> 完成步骤见 `docs/V081_MULTIPLAYER_E2E_RUNBOOK.md`，实测数据填入 `docs/V081_MULTIPLAYER_E2E_REPORT.md`。

- [ ] 同机双隔离 Minecraft 客户端
  - Evidence: 需真实运行两个独立 instance root 的客户端；当前仅实例 3（Forge 1.20.1，277 mods，无存档），尚无第二个隔离实例。
- [ ] Host 获取真实 *.e4mc.link
  - Evidence: 需 SH 创建房间后真实进世界；已确认 broker/relaymap 可达（HTTP 200，jp.e4mc.link），但未完成真实域名获取。
- [ ] Guest 通过公网域名加入
  - Evidence: 需第二个客户端输入真实公网地址加入。
- [ ] 禁止 localhost / LAN 地址
  - Evidence: 由上述两项的真实地址记录共同证明（`*.e4mc.link`）。
- [ ] 30 分钟稳定性
  - Evidence: 需记录 disconnect/reconnect/address change/provider error。
- [ ] 连续创建/结束 ≥3 次
  - Evidence: 需真实三轮 Create → Join → Stop。
- [ ] Launcher UI 关闭后联机继续
  - Evidence: READY 且 Guest 加入后关闭主窗口，验证游戏/tunnel 继续、退出后清理。
- [ ] Host crash cleanup
  - Evidence: 至少一次强制结束 Host 进程，验证 state 不停留 READY、history 落盘。
- [ ] 至少一个 1.20.x
  - Evidence: 实例 3 为 Forge 1.20.1，可作 Host；尚未进入真实房间流程。
- [ ] 至少一个 1.21.x
  - Evidence: 当前无 1.21.x 实例，需先创建（Fabric/NeoForge 以 e4mc 实际可用版本为准）。
- [ ] 主要 Loader smoke E2E
  - Evidence: Forge 待测；Fabric/NeoForge/Quilt 按环境允许范围，无法运行时标注 LOCAL_ENV_PENDING。

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
