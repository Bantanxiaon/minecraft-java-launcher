# SH Launcher v0.9.0 Beta —— Offline Account 体系全量稳定性整改报告

> 按 `SH Launcher v0.9.0 Beta —— 离线账户体系全量稳定性整改` 规范执行。
> 2026-08-17 起作为 v0.9.0 Beta 离线版发布：不进入 Microsoft OAuth，联机/e4mc
> 不承诺稳定性（在线端到端验收 pending）。所有结论均有 machine-readable
> evidence，禁止只靠 Markdown 手动打勾。

## 一、找到的 Offline Account 问题（审计结果）

1. `display_name` 历史上有 UNIQUE 约束、后来又移除：仅大小写不同的同名账户（Steve/steve）
   可以重复创建，产生肉眼不可区分的身份。
2. 没有 `username_normalized` 重复检测，也没有并发防护：双击/并发 Create 会创建两个账户。
3. 当前账户只保存在前端内存（`selectedAccountId`），重启后只能退回列表第一个；
   没有 default/active 的 account_id 持久化，也没有悬空引用清理。
4. 删除账户不是事务：settings/实例绑定/凭据分步操作，中途失败会留下半状态；
   删除当前/默认账户没有确定性回退。
5. 启动参数在各处临时拼 `--username/--uuid/--accessToken`，没有
   `LaunchIdentity::Offline` 强类型与统一构建入口；启动期间改账户无快照冻结。
6. `play_history` 只有 instance FK，没有身份快照：账户删除/改名后无法审计当时是谁在玩。
7. Offline UUID 有正确实现，但没有与 Java `UUID.nameUUIDFromBytes` 逐字节的 golden 证据，
   也没有“存储 UUID 损坏时确定性修复”路径。
8. 错误直接透传 raw SQL（`UNIQUE constraint failed` 等），违反产品化要求。
9. 缺少统一 invariant checker、真实旧库迁移验收与发布门禁。

## 二、修改的文件

- `src-tauri/src/lib.rs`：OfflineUsernameValidator、normalized 唯一约束与 v12 迁移、
  事务化创建/删除、default/active 状态命令、LaunchIdentity 快照、play_history 身份快照、
  UUID 修复、错误产品化、`account_integrity_report`、验收分发（integrity/flow/migrate）。
- `src-tauri/src/acceptance.rs`：`run_offline_account_acceptance`（§33 最低场景 + §13/§14
  启动矩阵）、`run_account_migration_acceptance`（§27）、`launch_offline_and_verify`。
- `src/App.tsx`：启动时经 `get_account_state` 恢复账户、`selectAccount` 统一切换并持久化
  active、删除后确定性回退。
- `src/types.ts`：`Account.minecraftUuid`。
- `src/pages/SettingsPage.test.tsx`：账户按 id 选择/移除的回归测试。
- `scripts/final-acceptance.mjs`：`account-integrity` / `account-flow` / `account-migrate`。
- `scripts/release-gate.mjs`：离线账户 evidence 门禁（机器生成 JSON，不接受手打勾）。
- `docs/evidence/*.json`：本轮真实证据。

## 三、数据模型变化（Migration v12）

- `accounts.username_normalized TEXT`：仅 OFF 类型做大小写无关重复检测；
  存在历史重复时跳过唯一索引创建（不阻断启动），由 invariant checker 标记。
- 唯一索引 `idx_offline_accounts_normalized`
  `WHERE account_type='OFFLINE' AND username_normalized IS NOT NULL`。
- `play_history` 新增 `account_id`（无 FK，历史必须能在账户删除后保留）、
  `username_snapshot`、`minecraft_uuid_snapshot`、`auth_type_snapshot`。
- 身份语义：`account_id`（不可变 INTEGER 主键，等价内部不可变 ID）≠
  `minecraft_uuid`（游戏身份）≠ `username`（显示名）。
  改名风险过大，v0.9.0 禁止原地改名，只能新建账户。

## 四、UUID 算法证据

`minecraft_offline_uuid` 等价 `UUID.nameUUIDFromBytes(("OfflinePlayer:" + username).getBytes(UTF_8))`：
MD5 name-based、version=3、RFC4122 variant。与 JDK17 本机交叉验证的 golden 值
（`tests::offline_uuid_matches_java_name_uuid_golden_values`）：

```text
Steve           5627dd98-e6be-3c21-b8a8-e92344183641
Alex            36532b5e-c442-3dbb-a24c-c7e55d0f979a
TestPlayer      bb77495a-a740-3169-a238-69654c8bd2c1
abc123          4062f8b7-64b0-384d-8ad1-4206c09391ad
steve           53909932-f794-33c0-9329-948045a4c1ce
STEVE           af74c7dc-2613-3e4e-850c-e6b1a849a686
A_1             0f20fc23-0935-3c24-9603-4c4ab6a40bb0
ABCDEFGHIJKLMNOP 2b74a1cc-0832-35db-8638-abf7e029466d
Player_Name123  1d054b5f-5092-33d3-9db3-191970a255fd
Notch           b50ad385-829d-3141-a216-7e7d7539ba7f
```

启动时 `stored_or_repaired_offline_uuid`：存储 UUID 缺失/非法/与官方不一致 → 事务内重算并
写回（username 本身损坏则标记 ACCOUNT_CORRUPTED_REQUIRES_USER_ACTION，不猜）。

## 五、Migration 结果

- 单元 fixture：`migration_v12_reapply_preserves_accounts_bindings_and_uuid`（旧列剥离后重跑
  迁移，account_id/username/UUID/实例绑定不变，`foreign_key_check` 干净、`integrity_check=ok`）。
- 真实用户数据副本：`docs/evidence/account-migration.json`。
  `D:\MinecraftLauncherData\launcher.sqlite3` 复制到 staging 后升级到 v12：
  `accountsPreserved=true`、`bindingsPreserved=true`、完整性通过。原库未被触碰。

## 六、CRUD / concurrency / crash 测试

- `create_offline_account_rejects_case_insensitive_duplicates`
- `concurrent_create_same_name_only_one_succeeds`（双线程双连接，恰好成功一次）
- `create_crash_rollback_leaves_no_partial_state` / `delete_crash_rollback_keeps_account_and_references`
- `delete_active_and_default_account_switches_references` / `delete_only_account_clears_references`
- `delete_account_keeps_instances_and_nullifies_pinning` / `delete_instance_keeps_account`
- `stored_or_repaired_offline_uuid_repairs_missing_and_corrupt`
- `account_invariants_hold_after_random_operations`（300 步随机 CRUD 后不变量成立）
- `offline_username_validator_is_total_and_deterministic`（2000 组随机输入不 panic、确定）

## 七、各 Minecraft/Loader 真实启动结果（离线账户 SHAcceptance）

全部走生产 `launch_instance` 路径，以 play_history 身份快照
`username/uuid/auth_type matchesFrozenIdentity=true` 作为强证据。

| 组合 | Java | 结果 |
| --- | --- | --- |
| Vanilla 1.20.4（主流程） | 17 | PASS |
| Vanilla 1.20.1 | 17 | PASS |
| Vanilla 1.21.11 | 21 | PASS |
| Forge 1.20.4 | 17 | PASS |
| NeoForge 1.21.11 | 21 | PASS |
| Fabric 1.20.4 | 17 | PASS |
| 第二账户切换 + 删除回退后再次启动 | 17 | PASS |
| Java 8 + Vanilla 1.16.5 | 8 | ENV_BLOCKED：Adoptium JDK8 下载 0 字节停滞，如实记为环境阻塞，未伪造 |

每轮均验证：进程启动、`Setting user: <username>` 出现、客户端真正进入、
快照 UUID 与冻结身份一致、无 Microsoft 登录弹窗/launcher 侧认证请求。

## 八、account integrity evidence

- `docs/evidence/offline-account-integrity.json`：`foreignKeyCheck=clean`、
  `integrityCheck=ok`、violations=[]。
- `docs/evidence/offline-account-flow.json`：主场景 + 矩阵 machine evidence。

## 九、是否存在 release blocker

本地可实现的 Offline Account 项全部完成（126 Rust 测试、25 前端测试、clippy 无 error、
`pnpm build` 通过）。唯一环境阻塞：

- **Java 8 + 1.16.5 启动验收**：本机无 JDK8，Adoptium 下载源在本网络 0 字节停滞。
  代码已加超时保护并如实分类（`java_install_timed_out`）。

联机清单（`docs/V081_MULTIPLAYER_CHECKLIST.md`）仍有 `[ ]` 项，release-gate 保持 FAIL，
按规范本轮不发布。

## 十、最终 commit hash

未提交（本轮按规范只完成代码与验收，等待下一步发布指令；届时统一提交、打 tag）。
