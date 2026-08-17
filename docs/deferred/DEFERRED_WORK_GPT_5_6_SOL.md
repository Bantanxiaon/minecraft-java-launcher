# Deferred Work — GPT-5.6 Sol 后续施工清单

恢复条件：
GPT-5.6 Sol 额度/开发预算恢复后，
优先使用 GPT-5.6 Sol 高推理继续施工。

不要重复研究已经真实解决的：
- KotlinForForge 4.12.0 / Expandability 9.0.4 依赖修复
- Dependency Repair force bypass（已移除“仍要启动”，缺失前置阻止启动）
- Startup Window visibility（MainVisibleConfirmed → SplashClosed）
- Multiplayer Isolation（UI 隐藏 + 普通启动零副作用）
- Modpack One-click Import 基础闭环（Modrinth / CurseForge 已到主菜单）

---

## P0-DEFERRED

### 01 Modpack compatibility expansion

- ID: DEFERRED-01
- Priority: P0
- Status: DEFERRED
- Current Evidence: docs/evidence/nextgen/audit/e2e-modpack.json（Modrinth）、e2e-curseforge.json（CurseForge）均 PASS：导入→ready→exact Loader→主菜单。
- Observed Problem: 仅验证了小型合法 fixture；复杂/非标准/历史格式包未覆盖。
- Root Cause / Suspected Cause: 各 Provider 文件结构差异大，Generic/MMC/HMCL/MCBBS 未完成 Installation E2E。
- Relevant Files: src-tauri/src/lib.rs（import_modrinth_pack_inner / import_curseforge_pack / ensure_pack_runtime）、src-tauri/src/modpack_ops.rs
- How To Reproduce: 用真实大型 Modrinth / CurseForge 包导入，观察依赖解析、远程文件下载与启动。
- Desired Final State: 更多 Provider 与复杂包可导入并可启动；失败时有明确文件级错误。
- Acceptance Gate: Fixture 矩阵覆盖 Modrinth/CurseForge/Generic/MMC/HMCL/MCBBS；导入后可启动到主菜单或给出精确失败原因。
- Do Not Regress: 不得恢复 content-first commit（base_missing 半成品）；不得绕过 exact Loader。

### 02 Modpack Update / Round-trip

- ID: DEFERRED-02
- Priority: P0
- Status: DEFERRED
- Current Evidence: content_update 模块存在；update_modrinth_modpack 有测试。
- Observed Problem: 更新/导出/再导入的完整 round-trip 未做真实 E2E。
- Root Cause / Suspected Cause: 预算限制，未纳入本轮。
- Relevant Files: src-tauri/src/content_update.rs、src-tauri/src/content_reconcile.rs
- How To Reproduce: 导入包→升级版本→导出→再导入。
- Desired Final State: 更新计划可预览、应用、回滚；导出包可被重新导入。
- Acceptance Gate: 更新后实例仍可启动；pack_owned_files 与磁盘一致；导出 ZIP 可再导入。
- Do Not Regress: 不得覆盖用户 saves/config 或破坏 pack_owned_files。

### 03 Full Loader World Create E2E

- ID: DEFERRED-03
- Priority: P0
- Status: DEFERRED
- Current Evidence: world-create/forge.json：Forge 世界已创建、集成服务器启动、玩家进入、稳定 300s、世界已保存；但 Save&Quit 后 JVM 120s+ 未退出（强杀 exit=-1），且 Create New World 点击未被机器验证（游戏自动创建默认世界）。
- Observed Problem: Vanilla/Fabric/NeoForge/Quilt 未跑；Forge clean-exit 未闭环。
- Root Cause / Suspected Cause: 自动化依赖 OCR 与窗口控制；部分包自带自动进世界行为；JVM 关闭阶段被某 mod 阻塞。
- Relevant Files: docs/evidence/nextgen/world-create/、D:/.codex/.tmp/world-create-forge.ps1（OCR 驱动）
- How To Reproduce: 见 world-create/forge.json；需在真实 Forge 实例上点“保存并退回到标题屏幕”后观察进程退出。
- Desired Final State: 五个 Loader 全部 Launch→Create World→Integrated Server→Player joined→Save→Quit Game→clean exit。
- Acceptance Gate: 每个 Loader 的 world-create/<loader>.json 全字段 true，exitCode=0。
- Do Not Regress: 不得把 NOT_RUN 说成 PASS；不得伪造创建点击。

---

## P1-DEFERRED

### 04 High DPI 175/200%

- ID: DEFERRED-04
- Priority: P1
- Current Evidence: audit/ui-matrix：1280x800@200% 与 2560x1440@175% 出现 sidebar overlap/offscreen。
- Observed Problem: 高缩放小逻辑视口下 sidebar 与 footer/账户区重叠。
- Root Cause / Suspected Cause: 固定高度/间距未适配极小逻辑高度。
- Relevant Files: src/ui/shell.css、src/ui/pages.css
- Desired Final State: 175/200% 无重叠、无截断。
- Acceptance Gate: ui-matrix 全组合 overlaps=0。
- Do Not Regress: 不影响 100–150% 布局。

### 05 Downloads raw SHA / Job ID

- ID: DEFERRED-05
- Priority: P1
- Current Evidence: audit/ui-v4/audit.json：下载主列表文件名即 SHA-1（33 处），并显示“任务 #xxxx”。
- Observed Problem: 主列表直接暴露技术字段。
- Root Cause / Suspected Cause: 缓存文件以 SHA-1 命名，任务行直接显示目标文件名。
- Relevant Files: src/features/downloads/DownloadsPage.tsx
- Desired Final State: 主列表只显示文件名/来源/进度/速度/ETA/状态；SHA/URL/Job ID 移入详情。
- Acceptance Gate: 主列表泄漏扫描 0 命中。
- Do Not Regress: 详情页仍可查看完整校验信息。

### 06 Crash confidence semantics

- ID: DEFERRED-06
- Priority: P1
- Current Evidence: crash_reports id=3：suspected_cause=“未找到根因异常”却 confidence=90%。
- Observed Problem: 置信度与根因缺失语义矛盾。
- Root Cause / Suspected Cause: confidence 计算与根因存在性未联动。
- Relevant Files: src-tauri/src/crash_diagnosis.rs
- Desired Final State: 无根因时 confidence 显示为 low/unknown。
- Acceptance Gate: 单测覆盖“无根因→非高置信度”。

### 07 Windows BUILD_GATE harness

- ID: DEFERRED-07
- Priority: P1
- Current Evidence: build-gate.mjs 在本机 spawnSync('pnpm.cmd') 无 shell:true 抛 EINVAL。
- Observed Problem: harness 误报产品构建失败。
- Root Cause / Suspected Cause: Node spawnSync 对 .cmd 需要 shell:true。
- Relevant Files: scripts/build-gate.mjs
- Desired Final State: harness 与底层 lint/test/build/fmt/clippy/cargo test 一致。
- Acceptance Gate: node scripts/build-gate.mjs 在本机 exit 0。

### 08 Evidence manifest automation

- ID: DEFERRED-08
- Priority: P1
- Current Evidence: 旧证据无指纹（44 stale JSON + 128 PNG）。
- Observed Problem: 无法自动判断旧证据是否对应当前工作树。
- Root Cause / Suspected Cause: 早期证据未包含 head/workingTreeHash。
- Desired Final State: 所有证据生成器自动写入指纹；manifest 自动校验。
- Acceptance Gate: EVIDENCE_FRESHNESS_GATE fresh=全部、stale=0。

---

## P2-DEFERRED

- 09 Visual Pass 3 / XMCL polish（含人工视觉评分）
- 10 Accessibility / Tutorial
- 11 Download Center 高级交互
- 12 Crash Analyzer 更多真实场景
- 13 旧证据批量刷新（仅当需要回溯时）

每个 P2 项在实施时需补充与 P0/P1 相同的上下文模板（ID/Priority/Status/Evidence/Problem/Root Cause/Files/Repro/Desired State/Acceptance Gate/Do Not Regress）。
