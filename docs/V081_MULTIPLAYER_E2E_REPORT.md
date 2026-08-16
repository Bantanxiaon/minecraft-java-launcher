# Multiplayer E2E Report（分层结果 · 基于真实日志）

> 本报告只记录真实测试证据。公网链路各层（Tunnel / Relay / Handshake / RSA transport）
> 的 PASS 与“经 Mojang/Microsoft session 验证后进入世界”的
> EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING 是两回事，后者不得被前者掩盖，也不得被伪装。

## Test Type

- [x] SAME_MACHINE_PUBLIC_TUNNEL
  - 说明：同机双隔离实例 + 真实 `*.e4mc.link` 公网隧道。只能写
    `PASS: Same-machine public e4mc tunnel E2E`，不代表跨设备/异网结论。
- [ ] TWO_PHYSICAL_DEVICES（EXTERNAL_ACCEPTANCE_PENDING）
- [ ] DIFFERENT_NETWORKS（EXTERNAL_ACCEPTANCE_PENDING）

## Environment

- Host Physical Device: SAME_MACHINE
- Guest Physical Device: SAME_MACHINE
- Host Instance: `Acceptance forge 1.20.4 Host`（instance root 隔离：
  `D:\MinecraftLauncherData-Acceptance\instances\2`）
- Guest Instance: `Acceptance forge 1.20.4 Guest`
  （`D:\MinecraftLauncherData-Acceptance\instances\3`）
- Connection Used: `woven-widen.jp.e4mc.link`（真实公网域名，2026-08-16 21:57 修复后复测轮）
- LAN/localhost used: NO
  - 说明：Guest 进程内的 helper 连接的是本机“加入 shim”
    （`127.0.0.1:<随机端口>`），shim 仅对 Minecraft 握手帧中的 serverAddress 做
    `\0FML\0` 品牌后缀规范化后转发到 `bullion-coerce.jp.e4mc.link:25565`；
    Host 侧出现的是 `QuicStreamAddress{streamId=1}`（e4mc relay 的 QUIC 流入流），
    而非局域网地址 `AcceptGuest[/192.168...`。判断依据是 Host 端真实日志，不是 shim 地址。
- Minecraft: 1.20.4 / Forge 49.2.0
- e4mc: 6.2.1-forge（version_id `CUKdAmgx`，SHA-1 `6fc90baef39cff5f9466ddf39f4421e3e9475308`）
- SH Launcher: 分类修复后版本（`passed_with_external_account_pending` 新分层证据）
- Machine-readable evidence:
  `docs/acceptance/latest-acceptance-multiplayer-run.json`

## 分层结果（Forge 1.20.4 同机公网 E2E，2026-08-16 21:57 修复后复测轮）

| 层 | 状态 | 证据 |
| --- | --- | --- |
| Public e4mc tunnel | PASS | Host log：`Domain assigned: woven-widen.jp.e4mc.link`；broker `getBestRelay` HTTP 200；relay `jp`。time_to_ready = 12 s |
| Relay forwarding | PASS | Host log：`AcceptGuest[QuicStreamAddress{streamId=1}] logged in with entity id 203`。streamId=1 是非控制通道（控制通道为 streamId=0），证明 Guest 流量经 e4mc relay QUIC 流真实到达 Host |
| Minecraft handshake | PASS | Guest log：`SH_E2E_HELPER_JOIN:127.0.0.1:52592` + `Connecting to 127.0.0.1, 52592`；Host 出现该连接的登录监听记录 |
| RSA login transport | PASS | Guest 到达 login/auth 边界：`Failed to log in: Invalid session (Try restarting your game and the launcher)`；Host `User Authenticator #1` 完成 `hasJoinedServer` 校验。该边界仅在加密响应阶段完成后才会发生 |
| Authenticated Guest world join | BLOCKED_BY_TEST_ACCOUNT_SESSION | 离线测试账户无合法 Mojang/Microsoft session，session 校验失败 |

### 世界连接细节（不得误读为“已认证加入”）

- Host log（复测轮 launcher 日志，节选已固化进 evidence JSON）：
  - `[21:57:26] Domain assigned: woven-widen.jp.e4mc.link`
  - `[21:57:47] [User Authenticator #1/WARN] Failed to verify username but will let them in anyway!`
  - `[21:57:48] AcceptGuest[QuicStreamAddress{streamId=1}] logged in with entity id 203`
  - `[21:57:48] AcceptGuest joined the game`
- Guest log（复测轮 launcher 日志，节选已固化进 evidence JSON）：
  - `[21:57:45] Connecting to 127.0.0.1, 49543`
  - `[21:57:46] Failed to log in: Invalid session (Try restarting your game and the launcher)`
  - `SH_E2E_CLIENT_JOINED:AcceptGuest`

> 解释：`Failed to verify username but will let them in anyway!` 是 **原版 1.20.4
> integrated server 自身的行为**——session 校验失败且 `MinecraftServer.isSingleplayer()`
> 为真时，用 `UUIDUtil.createOfflineProfile` 放行。该行为由本机真实 jar
> `ServerLoginPacketListenerImpl$1` 字节码证实，不是 SH Launcher 的认证绕过。
> 因此本轮 Guest 确实进入了世界，但走的是原版单机世界离线回退，**不是
> Mojang/Microsoft session 验证通过**。`sessionVerified=false`。

## e4mc 上游连接语义查证（来源证据）

- e4mc 官方描述（[Modrinth `mod/e4mc`](https://modrinth.com/mod/e4mc)）：安装后
  “Open to LAN as normal”，分配公开域名，
  “Others can simply connect to the public domain to connect to your LAN server”；
  “Works with vanilla clients … It just gives you a domain. Just share the domain.”
  → 客户端把 `*.e4mc.link` 当作**普通服务器地址**连接即可，上游不要求任何 LAN 标记。
- 上游仓库：[`vgskye/e4mc-minecraft-architectury`](https://github.com/vgskye/e4mc-minecraft-architectury)
  （MIT）。同组织 relay：`e4mc.link`。
- 离线账户在 e4mc 联机中出现 “Invalid session” 是上游已知现象，第三方
  [`xsyanic/offline-e4mc`](https://github.com/xsyanic/offline-e4mc) 等项目专门绕过它。
  SH Launcher **禁止**集成这类
  online-mode=false / session 伪造方案。
- 结论：测试 helper 使用 `ServerData.Type.OTHER`（普通服务器认证语义）是正确的；
  `Type.LAN` 只影响 UI 展示，且不改变 joinServer/session 校验，但为避免任何“伪装 LAN”
  的语义误导，NeoForge helper 已从 `Type.LAN` 改为 `Type.OTHER`。

## NeoForge 1.21.11（依据本次决定性证据）

- 公网 Tunnel / Relay / Handshake / RSA transport：PASS（Host 出现真实 QuicStreamAddress）。
- 最终断开：`disconnect.loginFailedInfo.invalidSession` → 分类为
  **BLOCKED_BY_TEST_ACCOUNT_SESSION**，不是网络失败。

## Stability / Recreate / Crash

- 30 分钟稳定性（Guest 驻留世界）：EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING
  - 原因：离线账户无法完成 session 验证进入世界，驻留型稳定性无法在本机闭环；
    Host 侧隧道状态机稳定性仍可在下一轮单独测量。
- 连续创建/结束 ≥3 次：本轮仅完成 1 轮（真实记录 N=1，不外推）。
- Launcher UI 关闭后联机继续：未在本轮执行。
- Host crash cleanup：PASS
  - 证据：`forcedTerminated=true`、`uiLeftReady=true`（state 离开 READY）、
    `closedOrError=true`，session 历史落盘。

## EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING：精确人工验收步骤

当前物理电脑无法提供合法在线测试账户（禁止用认证绕过替代）。有合法正版账户时执行：

1. 在 SH Launcher 登录两个合法正版账户 A（Host）与 B（Guest），确认账号状态正常。
2. 设置环境变量后运行验收：
   `LAUNCHER_E2E_HOST_ACCOUNT=<A用户名> LAUNCHER_E2E_GUEST_ACCOUNT=<B用户名>
   pnpm acceptance:multiplayer`
   （Harness 会自动使用这两个在线账户，并要求 session 验证通过后才判定 PASS。）
3. 验收判定标准（Harness 自动校验，全部满足才算 PASS）：
   - Host `*.e4mc.link` 为真实公网域名，禁止 localhost/LAN 回退；
   - Host 日志出现 `AcceptGuest[QuicStreamAddress{streamId>0}]`；
   - Guest 日志出现 `SH_E2E_CLIENT_JOINED:AcceptGuest`；
   - 双方日志无 `Invalid session` / `loginFailedInfo.invalidSession`；
   - 无 `Failed to verify username but will let them in anyway!` 离线回退。
4. 完成后把 `docs/acceptance/latest-acceptance-multiplayer-run.json` 的结果写回本报告，
   并将 `docs/V081_MULTIPLAYER_CHECKLIST.md` 对应项改为 `[x]`。

## Result

- Public e4mc tunnel: **PASS**
- Relay forwarding: **PASS**
- Minecraft handshake: **PASS**
- RSA login transport: **PASS**
- Authenticated Guest world join: **BLOCKED_BY_TEST_ACCOUNT_SESSION** →
  **EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING**

> 备注：不得把上述四层 PASS 表述为“联机已完全验证/跨网络稳定”；成功样本 N=1，
> 只能写“本次测试 1/1 完成四层公网链路 + 认证边界受限”。
