# SH Launcher 联机模块专项验收清单（e4mc 稳定性 / 易用性 / 最终验收）

> 依据：`SHLauncher_联机模块专项补丁_e4mc_稳定性_易用性_最终验收_Codex.md` §64/§65。
> 规则：只要本文件存在任何 `- [ ]`，不得将“联机模块稳定”写入 Release Notes；本清单已接入 `scripts/release-gate.mjs`。
> 真机双端 E2E 必须由用户协助完成，代码侧绝不以编造数据代替。

## 一、状态与地址正确性

- [x] localhost 不作为邀请地址
  - Implementation: `src-tauri/src/multiplayer.rs` `apply_event` 中 `LanOpened` 只写入 `lan_port`，绝不写入 `public_address`。
  - Tests: `lan_port_never_becomes_public_address`、`invalid_public_address_rejected`。
- [x] LAN_OPENED / READY 分离
  - Implementation: `MultiplayerState::{LanOpened, WaitingForTunnel, Ready}` 三段式；`Local game hosted on port` → LAN_OPENED；`Domain assigned:` + 严格校验 → READY。
  - Tests: `lan_port_never_becomes_public_address`、`valid_e4mc_address_marks_ready`。
- [x] public address 严格验证
  - Implementation: `validate_e4mc_public_address`：仅 `label.e4mc.link`、长度 ≤253、拒绝 `/` `\` `:`、拒绝连续点与非法前后缀。
  - Tests: `validate_e4mc_public_address_strict`、`invalid_public_address_rejected`。
- [x] 未知日志不改变状态
  - Tests: `unknown_log_does_not_change_state`。
- [x] Provider 错误进入 ERROR 状态
  - Tests: `provider_error_sets_error_state`。

## 二、e4mc 受管理身份与版本

- [x] e4mc 安装检测使用 provider identity/hash（禁止文件名猜测）
  - Implementation: `verify_managed_helper` 只信 `managed_content(provider='modrinth', project_id='qANg5Jrr')` + 磁盘存在 + SHA-256 一致。
  - Tests: `wrong_hash_helper_not_trusted`、`managed_helper_missing_triggers_repair`。
- [x] 动态版本匹配
  - Implementation: `resolve_e4mc_version` 拉取 Modrinth 全量版本 → `select_strict_version` 严格按 `game_versions` + `loaders` 过滤 → 最新 release。
  - Tests: `compatible_provider_version_selected`。
- [x] 动态 Loader 匹配（Fabric/Forge/NeoForge/Quilt）
  - Implementation: 与上同一解析器；Quilt 复用 `loaders: [fabric, quilt]` 构件；无匹配返回可读的 `HELPER_INCOMPATIBLE`。
  - Tests: `compatible_provider_version_selected`、`unsupported_loader_returns_actionable_error`。
- [x] managed_content / content_provenance 正确
  - Implementation: 安装写真实 `version_id`、`file_sha1`、计算后的 `file_sha256`、`installed_path`；provenance 写 `version_id` + `source_url` + hash；v11 迁移新增 `version_number` 列并新建 `multiplayer_history`。
  - Verification: 下载→校验→staging→安装→事务化双写（`install_e4mc`）。
- [x] 用户自装 e4mc 冲突处理
  - Implementation: 扫描 mods 目录按 jar 身份（mod_id/loader/游戏版本/哈希）识别；兼容则复用（`installed_by_launcher=0`，不接管所有权），不兼容给出可操作的冲突提示，绝不静默覆盖。
- [x] 原版实例给出明确不可用提示（不偷偷改 Loader）
  - Implementation: `vanilla_error()` → `HELPER_INCOMPATIBLE`。

## 三、会话生命周期

- [x] per-session state
  - Implementation: `session_id(UUID) -> MultiplayerSession`，事件携带 `sessionId`，按实例限制单房间。
  - Tests: `old_session_event_does_not_mutate_new_session`。
- [x] per-session cancellation
  - Tests: `cancel_old_session_does_not_cancel_new_session`、`game_exit_closes_only_current_session`。
- [x] log late-create/reopen/truncate/rotate/lossy UTF-8
  - Implementation: `LogTailer`：等待文件出现、len<offset 时重开、按字节读取 + lossy UTF-8。
  - Tests: `watcher_fixture_roundtrip_with_late_create_and_truncate`。
- [x] game exit cleanup
  - Implementation: `on_game_exit` 只关闭当前实例对应 session，取消 token、清地址、CLOSED、写历史。
  - Tests: `game_exit_closes_only_current_session`。
- [x] repeated create/stop
  - Tests: `repeated_create_and_stop_resets_state`。
- [ ] Launcher UI 关闭后联机继续（后台驻留生命周期）
  - 实现采用“关闭窗口后 Tauri 进程后台监督游戏”方案，代码路径已存在；但必须由真实双端 E2E 验证游戏继续、好友不掉线、退出后清理。
- [x] crash cleanup（代码路径）
  - Implementation: 崩溃/退出统一走 `game-crashed`/`game-exited` → `on_game_exit` 闭环。
  - 真机崩溃场景归入下方真实 E2E 项。

## 四、日志 fixtures（真实来源）

- [x] 真实日志 fixtures
  - 来源: `vgskye/e4mc-minecraft-architectury` commit `5b0db932660638ebd49b0719050abf7dbcb9e5bb`，消息正文逐字取自上游 logger 调用；来源与合成部分说明见 `src-tauri/src/multiplayer_fixtures/README.md`。
- [x] Forge parser fixture
- [x] Fabric parser fixture
- [x] NeoForge parser fixture
- [x] Quilt parser fixture
  - Tests: `fixture_parsers_cover_all_loaders` 覆盖上述四者。

## 五、UX / 安全 / 诊断

- [x] Provider error UX（弹窗级状态、可重试、可看诊断）
  - Implementation: `ServersPage` 按状态机渲染中文状态、ERROR 提供“重试/重新启动联机/查看诊断”；命令错误经 `errorText` 提取用户消息。
- [x] offline / session 提示准确
  - Implementation: 页面明示“e4mc 只提供隧道，不绕过正版验证；离线账户能否加入取决于世界/整合包身份验证方式”。
- [x] normal single-player unaffected
  - Implementation: 联机为 opt-in：普通启动路径不变，只有联机页创建房间才安装 e4mc 并带 `force=true` 启动。
- [x] Multiplayer diagnostics（脱敏导出）
  - Implementation: `multiplayer_diagnostics` 只导出状态摘要/事件类型，不导出 token、凭据、私密路径、原始域名（仅布尔标记）。
- [x] 房间历史（轻量）
  - Implementation: `multiplayer_history` 表 + `multiplayer_history` 命令，只记录时间/结果/时长。
- [x] Modpack 导出隐私
  - Implementation: 联机历史/诊断只存启动器数据库，不在实例目录内；`exports.rs` 的 `sh-modpack.json` 已声明不包含账户、Token、启动器数据库或凭据，导出不携带会话数据。
- [x] 实例删除保护 / Storage 保护 / Reconciler 识别
  - Implementation: 活跃会话禁止删除实例；storage 清理排除 `MULTIPLAYER_HELPER` 落盘路径；`reconcile_apply` 调用 `reconcile_managed_helpers` 修正失效 DB 记录。

## 六、真实双端 E2E（不可由代码替代）

- [ ] 两台真实客户端互相加入
- [ ] 至少 30 分钟稳定性测试（记录断线/重连/地址变化/退出清理）
- [ ] 重复创建/结束 ≥3 次真实场景
- [ ] 至少一个 1.20.x 与一个 1.21.x
- [ ] Loader 覆盖（Forge/Fabric/NeoForge/Quilt，无法实测者明确标注“未实测”）
- [ ] 游戏崩溃清理实测
- [ ] Launcher 窗口关闭后联机继续实测

> 结论：本地可实现项已全部完成并有自动化证据；真实 E2E 项完成前，不得宣称“联机模块稳定”，也不得通过 release-gate。
