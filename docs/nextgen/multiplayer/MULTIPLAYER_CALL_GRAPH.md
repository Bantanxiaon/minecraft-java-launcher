# Multiplayer / e4mc 调用图（普通单机启动隔离审计）

> 审计基线：`dev` HEAD `4c908c80e9d3dd0b7ff6601d72d82624fa1f8cd8`（本会话 DEV_HEAD）。
> 产品状态：`MULTIPLAYER_ENABLED=false`；Multiplayer Core 保留，所有公开命令带 capability guard。

## 1. 结论

`launch_instance`（普通单机启动链 Home→Play / Library→Play / Instance→Play 的唯一入口）
不调用任何 Multiplayer / e4mc 安装、准备、启动、隧道或网络逻辑。
所有 `multiplayer_*` 前端命令在非实验环境直接返回 `feature_disabled`。
因此 `reachable_from_normal_launch = false`（全部调用点）。

## 2. 调用点清单

| # | 源文件 | 函数 | caller | 普通启动可达 | 改实例 | 下载 | 写 mods/ | 联网 | 说明 |
|---|--------|------|--------|--------------|--------|------|---------|------|------|
| 1 | src-tauri/src/multiplayer.rs | `multiplayer_prepare` | 仅旧 `ServersPage`（已从生产移除）/ 实验环境 | 否 | 是（仅实验） | 是（仅实验） | 是（仅实验） | 是（仅实验） | 有 `multiplayer_experimental_enabled()` guard，非实验返回 FEATURE_DISABLED |
| 2 | src-tauri/src/multiplayer.rs | `multiplayer_start` | 仅旧 `ServersPage`（已从生产移除）/ 实验环境 | 否 | 是（仅实验） | 是（仅实验） | 是（仅实验） | 是（仅实验） | 同上；内部调用 `multiplayer_launch` |
| 3 | src-tauri/src/multiplayer.rs | `multiplayer_stop` | 仅旧 `ServersPage` / 实验环境 | 否 | 否 | 否 | 否 | 否 | guard；停止会话并关游戏 |
| 4 | src-tauri/src/multiplayer.rs | `multiplayer_cancel` | 仅旧 `ServersPage` / 实验环境 | 否 | 否 | 否 | 否 | 否 | guard |
| 5 | src-tauri/src/multiplayer.rs | `multiplayer_join` | 仅旧 `ServersPage` / 实验环境 | 否 | 是（仅实验） | 否 | 否 | 是（仅实验） | guard；内部启动本机 shim 转发 e4mc 域名 |
| 6 | src-tauri/src/multiplayer.rs | `multiplayer_state` | 仅旧 `ServersPage` / 实验环境 | 否 | 否 | 否 | 否 | 否 | guard；非实验返回 CLOSED + feature_disabled |
| 7 | src-tauri/src/multiplayer.rs | `multiplayer_diagnostics` | 仅旧 `ServersPage` / 实验环境 | 否 | 否 | 否 | 否 | 否 | guard |
| 8 | src-tauri/src/multiplayer.rs | `multiplayer_history` | 仅旧 `ServersPage` / 实验环境 | 否 | 否 | 否 | 否 | 否 | guard |
| 9 | src-tauri/src/lib.rs | `multiplayer_launch` | 仅 `multiplayer_start` | 否 | 否 | 否 | 否 | 否 | 包装 `launch_instance`，仅实验可达 |
| 10 | src-tauri/src/lib.rs | `multiplayer_join_launch` | 仅 `multiplayer_join` | 否 | 否 | 否 | 否 | 否 | 同上 |
| 11 | src-tauri/src/lib.rs | `launch_instance` 退出线程 | `multiplayer::on_game_exit` | 是（清理回调） | 否 | 否 | 否 | 否 | 纯只读会话清理（按 session 存在与否）；不安装、不下载、不改 mods |
| 12 | src-tauri/src/acceptance.rs | `run_multiplayer_prepare_acceptance` / `run_multiplayer_matrix_acceptance` | 仅 `LAUNCHER_E2E_MULTIPLAYER` 环境变量 | 否 | 实验专用 | 实验专用 | 实验专用 | 实验专用 | 仓库内自动化验收专用，普通启动不设置该变量 |
| 13 | src/pages/ServersPage.tsx | 旧联机/服务器页全部按钮 | 旧 App.tsx（已删除引用） | 否 | 是（若运行） | 是（若运行） | 是（若运行） | 是（若运行） | 死代码：生产 `App.tsx` 不再 import，Router 无该路由，Nav 无该入口 |
| 14 | src/pages/ServersPage.test.tsx | 测试 | vitest | 否 | 否 | 否 | 否 | 否 | mock invoke，仅验证旧组件行为 |

## 3. 普通启动链

```
Home → 开始游戏 ─┐
Library → 启动 ──┼─► launchSelectedInstance ─► invoke("launch_instance")
Instance → 启动 ─┘        │
                          ├─ installClientFiles（普通游戏文件，仅缺时）
                          ├─ installInstanceLoaderFiles（普通 Loader，仅缺时）
                          ├─ ensureJavaForGame（普通受管 Java，仅缺时）
                          └─ launch_instance（Rust）
                               ├─ 本地元数据 / 账户 / Java 校验
                               ├─ Command(java) 启动 Minecraft
                               ├─ play_history 记录
                               └─ 退出线程：on_game_exit（只读会话清理）
```

全链不出现 `multiplayer_prepare / multiplayer_start / ensure_e4mc / install_e4mc /
relay bootstrap / tunnel initialization / helper 安装`。

## 4. 隔离保证（代码层）

1. 生产导航（`src/app/Router.tsx` + `src/app/AppShell.tsx`）只有 6 个一级入口：
   首页 / 游戏库 / 发现 / 下载 / 账户 / 设置，无“联机/服务器/创建远程房间/e4mc/联机历史/服务器管理”。
2. 生产 `App.tsx` 不 import `ServersPage`，无 `multiplayer_*` invoke。
3. 所有 `multiplayer_*` 命令带 `multiplayer_experimental_enabled()` guard；
   `SH_MULTIPLAYER_EXPERIMENTAL=1` 或 `LAUNCHER_E2E_MULTIPLAYER` 才放行。
4. `launch_instance` 不调用 Multiplayer 准备/安装/启动；仅退出线程做只读会话清理。
5. 健康检查 `instance_health` 不把 e4mc 列为必需依赖；Repair/Reconcile 不补 e4mc。

## 5. e4mc 文件处置

- 不自动删除任何现有 `e4mc*.jar`。
- 若存在 SH 旧版受管安装记录（`content_items.provider=modrinth` +
  `metadataJson.projectId=qANg5Jrr` 或等效 provenance），UI 可提示用户确认后走
  staging/backup/remove/rollback；用户自装文件绝不自动删除或禁用。
- 本审计中实例 `Closing Song1.6.5` 的 `mods/` 内未发现 e4mc 文件（见隔离证据）。
