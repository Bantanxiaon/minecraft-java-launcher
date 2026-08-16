# v0.8.1 零遗留核对表（细粒度）

每项格式：`- [ ] 子项 — Implementation: …；Tests: …；Verification: …`

## Resolver（优先链：provider metadata → dependency metadata → hash → trusted mapping → 限定精确搜索 → Ambiguous/Unknown）

- [x] provider 原始 metadata（CurseForge 索引 modId→project/file，`resolve_curseforge_dependency` 第一层）— Tests: `curseforge_dependency_resolution_prefers_mod_id`；Verification: cargo test
- [x] provider 原始 metadata（instance_pack_source → 整合包版本依赖反查 project_id，第一优先层）
- [x] provider dependency metadata（已安装 Modrinth 项目版本依赖 → 依赖项目 title/slug 归一化匹配，结果写入 content_identity_cache）
- [x] exact hash reverse lookup（`modrinth_project_by_hash`，SHA-1 → version_file）— Tests: 待补 URL/解析单测
- [x] trusted mapping（已核对别名 + `content_identity_cache` 表；别名只作为一层）— Tests: `missing_dependencies_reports_kotlinforforge_when_absent`
- [x] 限定精确搜索（project_type=mod + 精确 slug/title 匹配）— Tests: `resolver_unknown_mod_without_exact_match_returns_no_candidates`
- [x] AMBIGUOUS 安全（同名多候选返回错误，不自动装第一条）— Tests: `resolver_rejects_ambiguous_same_slug_projects`
- [x] UNKNOWN 安全（无精确匹配返回错误）— Tests: 同上
- [x] “第 11 个未知 Mod”测试 — Tests: `resolver_unknown_mod_without_exact_match_returns_no_candidates`
- [x] 同名多候选测试 — Tests: `resolver_rejects_ambiguous_same_slug_projects`
- [x] Forge/NeoForge 多 `[[mods]]` metadata 全读（其余 modId 归入 provides，依赖汇总全部条目）— Verification: cargo test
- [x] Fabric/Quilt provides — Tests: `inspects_fabric_mod_descriptor` 等（provides 解析已加入 installed_mod_ids）

## Reconciliation / Duplicate

- [x] Reconciler scan（db_missing_on_disk / disk_missing_in_db / duplicate_groups / fingerprint）— Tests: `canonical_name_strips_timestamp_prefix`
- [x] Reconciler apply（重扫指纹、运行中禁止、重复项移备份、DB 增删）— Verification: cargo test
- [ ] Reconciler 完整集成测试（临时实例目录往返 + apply 后 rescan 一致）

## Modpack / Update

- [x] 整合包事务：staging 下载 → hash → overrides → loader/dependency/disk 验证 → atomic commit → DB commit — Implementation: `import_modrinth_pack_inner` 三阶段（staging 下载+校验复用 / overrides 安全解压 / FsTransaction 移动 + DB 双写，失败整体回滚 `discard_instance_immediately`）；Tests: `stage_pack_overrides_extracts_only_safe_tree`、`stage_pack_overrides_rejects_traversal_entry`、`pack_target_relative_rejects_managed_directories_and_traversal`
- [x] 崩溃恢复基础：`.staging/<operation-id>/operation.json` 元数据（running/committed/failed）+ list/cleanup 命令，Modrinth 导入已接入生命周期
- [x] 崩溃恢复完整：failed 操作继续/回滚 — Implementation: `retry_modpack_operation`（staged 文件按 SHA-1 复用续传）+ `committing` 状态机（提交中崩溃 → 启动时按 instance_id 回滚并清 staging，`recover_interrupted_modpack_operations`）；Tests: `operation_metadata_roundtrips_and_cleanup`
- [x] Content UpdatePlan 接入业务 — Implementation: `content_update::ContentUpdatePlan` + `prepare_mod_update`（版本比对/下载校验/依赖差集）+ `update_modrinth_mod` 应用计划（FsTransaction 备份回滚、依赖自动补齐经受信解析链、conflicts 记录并 emit `mod-update-plan`）；Tests: `dependency_delta_detects_added_and_removed`
- [x] Modpack UpdatePlan（pack-owned / user-added / user-modified / saves / config 区分，snapshot + rollback）— Implementation: `pack_owned_files` 归属表（migration v9，sha1/sha256 双重记录）+ `update_modrinth_modpack`（staging 下载→无副作用分类→TOCTOU 复检→FsTransaction+rusqlite 事务双写回滚，移除文件进 `.launcher-backup/pack-update-*`）
- [x] 用户 saves / 自加 Mod / 修改 config 保护测试 — Tests: `pack_classification_protects_user_content`（存档默认不覆盖、用户配置保留、自加同名模组保留、全新文件安装、未修改文件更新）

## Clone

- [x] libraries/assets 复制
- [x] memory / resolution 复制
- [x] 复制后置待校验状态（不继承 READY），运行中禁止
- [x] Java selection / JVM args / game args / account（instance_launch_settings 整表复制）
- [x] loader 信息（loader_version + java_profile 复制）
- [x] pack provenance 复制（instance_pack_source）
- [x] content 记录复制（content_items 重建为 source='clone'）
- [x] managed content 复制（managed_content 标记 installed_by_launcher=0）
- [x] instance icon 复制（instances.icon）
- [x] resourcepacks/shaderpacks/config/saves policy（saves 可选开关）— Implementation: `clone_instance` 新增 `copy_saves` 参数，前端 confirm 选择；mods/config/resourcepacks/shaderpacks/versions 复制，saves 按开关
- [x] assets/libraries 共享/复用架构（避免 Clone 复制数 GB 公共运行时）— Implementation: Windows `FSCTL_DUPLICATE_EXTENTS_TO_FILE` CoW reflink（`fs_safe::reflink_copy_file` + `copy_directory_contents_deduped`），同卷共享底层数据块、写入互不影响，失败回退普通复制；Tests: `reflink_copy_is_independent_when_supported`、`deduped_copy_reproduces_directory_contents`
- [ ] Clone 完成后 reconcile/validate + 回归测试（Vanilla/Forge/Fabric/NeoForge/Quilt/大包/自定义 Java/JVM/saves/provenance/managed e4mc）— Implementation: 克隆后运行 `reconcile_scan` 校验并记录日志；Java/JVM/启动设置/pack provenance/managed content 整表复制已完成；Verification: 待真机 acceptance（`LAUNCHER_E2E_CLONE`）执行后转 [x]

## FsTransaction / Fault Injection

- [x] FsTransaction move + LIFO rollback（`fs_safe::FsTransaction` + 测试）
- [x] 接入删除实例（DB 失败回滚文件移动）
- [x] 接入 Mod update（备份/替换/DB 失败均回滚）
- [x] 接入 Modpack import/update（import：staging+原子提交；update：UpdatePlan+快照回滚）
- [x] 接入 Content delete / World import/restore — Implementation: `remove_mod_to_backup`/`remove_content_to_backup`/`set_content_enabled` 用 FsTransaction + DB 失败回滚；`import_world` 改为 staging 落盘 → FsTransaction 原子就位 → DB 双写失败回滚；`restore_removed_backup` 复用受事务保护的 install/import 路径
- [x] 接入 Java runtime swap — Implementation: `install_managed_java` 旧运行时移备份 + staging 就位均走 FsTransaction，校验失败整体回滚，成功后清理旧备份
- [x] 接入 Reconcile destructive apply — Implementation: `reconcile_apply` 重复项移备份用 FsTransaction，DB 增删用 rusqlite 事务，任一失败联动回滚
- [x] 接入 Clone commit — Implementation: `clone_instance` 任何填充阶段失败 → `discard_instance_immediately` 丢弃半成品实例；补 pack_owned_files 复制；不复制 saves 时跳过 world 内容记录
- [x] file move / rollback 顺序测试（fs_transaction_rolls_back_moves_in_reverse_order）
- [x] DB commit failure 端到端故障注入 — Tests: `modpack_update_db_failure_rolls_back_rows`（rusqlite 事务未提交即回滚 content_items/pack_owned_files，正常提交生效）+ `fs_transaction_rolls_back_moves_in_reverse_order`（文件回滚）；真实 AppHandle 路径由 acceptance（LAUNCHER_E2E_*）在真机阶段验证

## Archive / Path

- [x] SecureArchiveExtractor 统一核心（`fs_safe::extract_zip_securely` + traversal/absolute/zip-slip/safe 测试）
- [x] Java 安装接入统一解压
- [x] Native JAR 接入（staging 安全解压后按原结构复制，META-INF 排除）
- [x] Modpack overrides 接入（安全解压到 staging → 校验路径 → 冲突备份后落位）
- [x] World ZIP 接入（安全解压 staging → level.dat 定位世界根 → 原子 rename）
- [x] Resource Pack / Shader Pack：以压缩包原样保存（Minecraft 直接读取），入口已有结构校验，无需解压
- [x] Windows 保留名 / 结尾点空格 / 禁用字符（`validate_windows_filename` 接入 instance 字段）
- [x] symlink/reparse 全路径实测 — Tests: `rejects_symlink_entries_and_reserved_names`（手工构造 Unix symlink 条目被“符号链接”原因拒绝、Windows 保留名/结尾点拒绝）、`junction_or_symlink_on_disk_is_not_followed_by_extraction`（真实 reparse 点不被 walkdir 跟随，无权限时降级跳过）；所有 archive 入口统一 `SecureArchiveExtractor`

## Supervisor / 生命周期

- [x] 方案 B：关闭 UI 后进程驻留监督 Java；游戏退出后才退出
- [x] game PID / started / ended / exit_code / crash / play_history / game-exited 更新（既有 watcher + 退出判定）
- [x] e4mc cleanup on game exit 验证 — Implementation: 游戏退出 watcher 调用 `multiplayer::on_game_exit`（取消日志监听、房间 CLOSED）；Tests: `game_exit_closes_room_state`
- [x] post-game task 验证 — Implementation: 游戏退出后 emit `post-game-tasks`（instanceId/exitCode），前端可刷新健康与日志；supervisor 进程保持到退出码落库后
- [x] unfinished session 恢复（PID 不存在标记 abnormal end）— Implementation: `recover_unfinished_sessions` 启动时把 ended_at 为空的 play_history 标记异常结束并写 crash_reports 建议；Tests: `unfinished_sessions_are_closed_on_startup`

## Startup / 性能

- [x] 独立 Splash Window + 主窗口初始隐藏
- [x] Splash 居中配置（center:true）+ 内容中心轴
- [x] 无强制 >1s 等待（350ms 最小品牌曝光）
- [x] 更新检查后台化
- [x] Java 检测不重复（启动仅一次，全量探测保留）
- [x] Mod full scan 移出关键路径（runBackgroundHealth）
- [x] Storage scan lazy（打开页面才扫描）
- [x] Instance Health Cache（mods 指纹增量）— Implementation: `mods_directory_fingerprint`（游戏版本/加载器 + 顶层 jar 名称/大小/mtime 哈希）命中 `settings['instance-health-cache:<id>']` 直接复用缺失依赖/不兼容结果，变化才全量扫描；Tests: `mods_fingerprint_changes_with_directory_contents`
- [x] Java runtime cache（path 失效才局部验证）— Implementation: `detect_java_runtimes_cached` 候选 java.exe 全部存在时复用 settings 缓存，任何路径失效才重新探测；diagnostics/boot health 共用缓存
- [x] Startup Metrics 输出 — Implementation: setup 阶段记录 total/db/recovered 指标，写 `startup-metrics.json` 并 emit `startup-metrics`
- [ ] 启动 benchmark（5 次 min/median/P95）— Implementation: `scripts/startup-benchmark.mjs`（`SH_STARTUP_BENCH_EXIT=1` 自退出 + 读 startup-metrics.json）；Verification: 真机 5 次运行产出 `docs/benchmark-startup.json` 后转 [x]
- [ ] 100/125/150% DPI 与多显示器实测（需真实 Windows 观察）

## 下载性能

- [x] 持久 HTTP client / 连接池
- [x] 分级并发 semaphore（metadata 6 / small 16 / library 12 / large 4，接入 metadata、Libraries、Assets 与大文件）
- [x] Vanilla Libraries 由串行改为受控并发（此前是逐个 await，是历史慢下载主因之一）
- [x] 任务级滑动窗口测速（SpeedMeter + 测试）
- [x] SHA-1 对象缓存命中零联网
- [x] 404 不重试 / 429 Retry-After / 指数退避 + 抖动
- [x] 来源健康统计 + download_diagnostics
- [x] Slow-source fallback（Host Health 已有，自动切换未接）— Implementation: `download_perf::host_is_slow`（近 3 秒窗口 <64KB/s 且有 ≥256KB 样本，或近期失败率 ≥2/3）+ 主源判定慢时优先 BMCLAPI 镜像、SHA-1 校验不变、失败回主源；Tests: `slow_and_failing_hosts_are_detected`
- [x] SQLite 移出 hot path（内存 + 250ms 节流 + 低频 checkpoint）
- [x] 真实下载基准（Modrinth 0.28–0.42MB/s、BMCLAPI 2.5–3.2MB/s、JDK 14.7–20.4MB/s；见 BENCHMARK_DOWNLOAD.md）
- [ ] 冷/热缓存 GUI 场景真实验收
- [ ] PCL 同机同网对照（需用户协助运行 PCL GUI）

## UI

- [x] 存储页（真实数据）
- [x] 联机页（创建房间/邀请地址/结束）
- [x] 实例详情页（游戏库 → 详情：概览健康状态 / 设置内存 / 日志 / 磁盘对账），后端 instance_health
- [ ] 实例详情内嵌模组/资源包/光影/存档 钻取式 IA（仍为全局页选择实例）
- [ ] Mod UX 升级（图标 + 简洁名称，技术文件名进详情）
- [x] 下载诊断 UI（下载页显示来源健康：请求数/成功/失败/流量）
- [x] 无假按钮 / 隐藏未完成 English
- [ ] 前端行为测试扩展（Instance Detail / Reconcile / Ambiguous / UpdatePlan / Cleanup / Restore / Fallback / Startup handoff / Multiplayer lifecycle / Error recovery / Settings）

## 测试与发布

- [x] cargo fmt / clippy -D warnings / 54 Rust tests / 8 联网忽略
- [x] pnpm lint / Vitest 6 / build
- [x] release-gate.mjs 已接 release.yml（核对表/版本/notes/benchmark 校验，失败 exit 1）
- [ ] migration fixture（真实 v0.8.0 DB 原地升级）
- [ ] updater upgrade 从 v0.8.0 实测
- [ ] 外部：Cloudflare R2 凭据（未上传自有 CDN）
