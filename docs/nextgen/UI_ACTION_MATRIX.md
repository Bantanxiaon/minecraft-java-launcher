# SH Launcher UI 3.0 Action Matrix

> 全量机器可读矩阵：[ui-actions.json](./ui-actions.json)
> 机器门禁：`node scripts/ui-action-gate.mjs`（BROKEN=0 / PLACEHOLDER=0 / UNTESTED_PRIMARY_ACTION=0）

## 汇总

| 状态 | 数量 |
|---|---|
| IMPLEMENTED | 99 |
| HIDDEN_BY_FEATURE_FLAG | 0（Multiplayer 生产入口整体隐藏，无单按钮 flag） |
| DISABLED_WITH_RUNTIME_REASON | 0（未开发能力不靠灰按钮；Microsoft 登录以非交互状态展示） |
| BROKEN | 0 |
| PLACEHOLDER | 0 |
| UNTESTED_PRIMARY_ACTION | 0 |

> 当前矩阵共 99 项，全部 IMPLEMENTED；门禁输出见
> `docs/evidence/nextgen/ui-action-gate.json`。

## 闭环定义

每个可见交互必须满足：

```
Visible Control → actionId → handler → service/router → backend command 或真实前端行为
→ loading → success → error → 自动化/运行证据
```

## 主要动作与后端命令

| actionId | 控制面 | handler | 后端/路由 |
|---|---|---|---|
| home.launch | 首页主 CTA | launchSelectedInstance | invoke:launch_instance |
| library.play | 游戏库卡片 | launchSelectedInstance | invoke:launch_instance |
| instance.launch | 实例概览 | launchSelectedInstance | invoke:launch_instance |
| home.createInstance | 新建实例 | openInstanceForm/createInstance | invoke:create_instance_profile |
| library.clone/rename/repair/delete | 游戏库操作 | cloneInstance/renameInstance/repairInstance/deleteInstance | invoke:clone_instance / rename_instance / repair / delete_instance_to_backup |
| instance.tab.* | 实例 7 Tab | changeTab | 路由 + 各功能页 |
| instance.mod.* | 模组管理 | toggleMod/removeMod/updateMod/updateAllMods/checkModUpdates/searchOnline/installOnlineMod/installCurseforgeUrl/restoreBackup | invoke:set_mod_enabled / remove_mod_to_backup / update_modrinth_mod / check_mod_updates / search_online_projects / install_online_mod / install_curseforge_url / restore_backup |
| instance.archive.* | 资源包/光影 | toggleArchive/removeArchive/importArchives | invoke:set_content_enabled / remove_content_to_backup / install_content_archive |
| instance.world.* | 存档 | backupWorld/duplicateWorld/exportWorld/removeWorld/deleteWorldPermanently/chooseAndImportWorld | invoke:backup_world / duplicate_world / export_world / remove_world_to_backup / delete_world_permanently / import_world |
| instance.log.* | 日志 | loadLogs/readLog | invoke:list_game_logs / read_game_log |
| discover.tab.* | 发现分类 | setDiscoverTab | 路由 |
| discover.modpack.* | 整合包 | inspectPack/importPack/exportPack/importArchiveAsNewInstance/removeModpackArchive/searchOnline/installOnlinePack | invoke:inspect_modpack / import_modpack / export_modrinth_modpack / import_modpack_archive_as_instance / remove_modpack_archive / search_online_projects / install_online_modpack |
| downloads.* | 下载中心 | refreshDiagnostics/cancelDownloads/exportDiagnostics/setSelectedJob | invoke:list_download_jobs / cancel_download / export_diagnostics / 前端详情弹窗 |
| accounts.* | 账户 | selectAccount/removeAccount/createProfile | invoke:set_active_account / remove_account / create_offline_account |
| settings.* | 设置 6 Tab | saveLauncherSettings/chooseExistingGameDirectory/installManagedJava/checkEnvironment/loginMicrosoft/loginExternal/cleanLauncherCache/exportDiagnostics/setThemeMode | invoke:save_settings / install_managed_java / detect_java_runtimes / login_microsoft / login_external / clean_launcher_cache / export_diagnostics |
| shell.* | Shell/Titlebar | navigate/runWindowAction/onSelectAccount | 路由 + Tauri 窗口 API + set_active_account |
| splash.retry | Splash | retry | invoke:startup_ready |

## 运行时证据

- 主动作（Play/创建/保存/登录/下载/模组开关/存档备份/整合包导入/日志读取）均已通过真实 Tauri EXE 运行验证：
  - `docs/evidence/nextgen/normal-launch-forge.json`（真实 Forge 启动闭环）
  - `docs/evidence/nextgen/ui-after/*.png`（各页面真实运行截图）
- 静态扫描覆盖全部 `src/**/*.tsx|ts`，无空 onClick / console-only / href="#" / TODO 占位。
