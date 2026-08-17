# NextGen Gates（Release Audit 最终结果）

生成时间：2026-08-17 17:55 +08:00

HEAD：`4c908c80e9d3dd0b7ff6601d72d82624fa1f8cd8`（branch dev）

工作树指纹（修复后）：`ff561b832e8b4667d0605edb8c42df253d4436d41ff7c3562c1578259f19d14e`

修复前指纹：`3f31e0535964a659165f76441e0461aa309901524d3f259c4eb3457797938b55`

本轮按用户要求实施了修复：KotlinForForge LIBRARY JAR 识别、CurseForge 在线搜索回退、缺失前置未补齐阻止启动、Home 上次游玩真实时间显示。修复后全部 evidence 重新取证。

## Gate 表

| Gate | Result | Fresh Evidence? | Command | Evidence | Failure Reason |
| --- | --- | --- | --- | --- | --- |
| UI3_RUNTIME_GATE | PASS | 是（post-fix） | ui3-runtime-gate.mjs + CDP 生产 EXE | audit/ui-v4*, ui-runtime.json | - |
| UI_ACTION_GATE | PASS | 是（post-fix 重跑） | ui-action-gate.mjs | ui-action-gate.json（108 actions，broken=0 placeholder=0 untested=0） | - |
| UI_LAYOUT_INTEGRITY_GATE | FAIL | 是 | audit-ui-v4.mjs / ui-matrix | audit/ui-v4/audit.json、ui-v4-150、ui-matrix | 100% 1280x800：instance-mods“有问题”徽章换行 1 处；1280x800@200%：sidebar 重叠 4 处+越界 16 处；2560x1440@175%：重叠 2 处 |
| UI_INFORMATION_HIERARCHY_GATE | FAIL | 是 | CDP bodyText 泄漏扫描 | audit/ui-v4/audit.json（downloads） | 下载主列表直接显示 33 个 raw SHA-1（文件名即哈希）与 internal job ID（“任务 #8510”） |
| UI_VISUAL_SEMANTIC_GATE | FAIL | 是（客观） | ui-visual-semantic-gate.mjs | UI_VISUAL_REVIEW_V2.md 未完成人工评分 | 本会话模型不支持图像输入，禁止伪造 PASS → MANUAL_REVIEW_REQUIRED |
| BORDER_DENSITY_GATE | FAIL | 部分 | border-density-gate.mjs | 静态约束 PASS；人工验收未完成 | UI_VISUAL_REVIEW_V2.md 无 BORDER_DENSITY_GATE PASS 标注 |
| TYPOGRAPHY_RHYTHM_GATE | FAIL | 部分 | typography-rhythm-gate.mjs | token 约束 PASS；人工验收未完成 | UI_VISUAL_REVIEW_V2.md 无 TYPOGRAPHY_RHYTHM_GATE PASS 标注 |
| VISUAL_PASS_2 | FAIL | 部分 | 截图+OCR | audit/ui-v4/*.png、ui-matrix | MANUAL_REVIEW_REQUIRED；不能凭 avgRGB 代替人工/视觉模型 |
| LOADER_VERSION_DISCOVERY_GATE | PASS | 是 | loader-version-discovery-gate.mjs + live cargo test | loader-metadata/summary.json、audit/ui-v4 loaderStep | Forge 131/NeoForge 62/Fabric 251/Quilt 299（1.20.1）；UI Forge 1.18.2=158 项，含 40.2.21，搜索返回 1 项 |
| MODPACK_RUNTIME_DETECTION_GATE | PASS | 是 | modpack-runtime-detection-gate.mjs + inspect_modpack | audit/e2e-modpack.json inspect 段 | mrpack：1.20.1 / forge / 47.4.22 / Java 17 / confidence 1 / mods 1 / overrides 2 |
| MODPACK_INSTALLATION_E2E_GATE | FAIL | 是 | import_modrinth_pack（生产 EXE） | audit/e2e-modpack.json import 段 + DB/目录核验 | 内容提交成功（Patchouli+2 overrides，hash 正确），但实例 status=base_missing、loader_version=NULL、无 launcher-effective.json/1.20.1 profile，exact Forge 47.4.22 未安装，不能运行 |
| DEPENDENCY_DISCOVERY_GATE | PASS | 是 | dependency_resolver_regression live | dependency-regression.json | kotlinforforge=ordsPcFz 4.12.0；expandability（Forge 1.20.1）=CurseForge 465066 v9.0.4（修复后正确选择） |
| DEPENDENCY_REPAIR_E2E_GATE | PASS | 是 | repair_missing_mod_dependencies instanceId=2（生产 EXE） | dependency-regression.json repairE2E 段、dependency-repair-current.json | missing [expandability,kotlinforforge]→[]，mods 276→278；两个 artifact 与官方 CDN 哈希一致；修复后 Forge 启动 75s 存活并到主菜单 |
| MULTIPLAYER_ISOLATION_GATE | PASS | 是（post-fix） | run-multiplayer-isolation.ps1 | multiplayer-isolation-current.json、multiplayer-isolation-gate.json | UI 无联机入口；普通 Forge 启动 75s 存活；mods 哈希树前后一致（7d5dc3ca…）；e4mc=0/multiplayer_history=0/servers=0/sessions=0 |
| WORLD_CREATE_RUNTIME_GATE | FAIL | 是（forge 部分） | world-create-forge.ps1 + OCR | world-create/forge.json + forge-*.png | Forge：世界已创建/进服/稳定 300s，但 Save&Quit 后 JVM 120s+ 未退出（强制终止 exit -1），非 clean exit；Create New World 点击未被机器验证（游戏自动创建默认世界）；vanilla/fabric/neoforge 无 ready 实例 → NOT_RUN |
| MIXIN_DIAGNOSTICS_GATE | NOT_RUN | 是（单测） | cargo test crash_diagnosis::tests | crash_diagnosis.rs tests 5/5 | 未发生真实 MixinTransformerError，无法端到端验证诊断输出；单元测试覆盖 wrapper/root/config/class/target/owning JAR/confidence/repair/不伪造确定 |
| STARTUP_WINDOW_GATE | PASS | 是（post-fix） | startup-window-gate.mjs + launcher.log | startup-window.json | MainVisibleConfirmed→SplashClosed→Ready（09:41 UTC）；同日捕获 ShowAckButNotVisible→Failed 一次，看门狗有效 |
| BUILD_GATE | FAIL | 是 | build-gate.mjs（底层逐项直跑） | production-build.json、本表 | build-gate.mjs 在本机 spawnSync('pnpm.cmd') 无 shell:true 抛 EINVAL（harness 缺陷）；底层真实结果：lint 0（警告）、vitest 28/28、vite build OK、cargo fmt/clippy -D warnings OK、cargo test 156 passed/11 ignored |
| EVIDENCE_FRESHNESS_GATE | FAIL | - | 指纹扫描 | docs/evidence/nextgen/audit/*.json | 旧证据（ui-after-v2/v3 PNG、旧 GATES_NEXTGEN.md、旧 multiplayer/startup/loader 缓存等）无 head/workingTreeHash 指纹；新审计证据均已带指纹；release blockers 要求 stale=0 missing=0 未满足 |

## A. UI 真实截图结论

- 修复后 Home：最近游戏显示真实相对时间（12 分钟前 / 22 分钟前 / 24 分钟前 / 2 小时前），不再“时间未知”。
- 页面矩阵（生产 EXE + CDP）：home/tutorial/library/create-instance/loader-build/instance(overview/mods/worlds/diagnostics)/discover+search/downloads+detail/accounts/settings(general/game-java/download/storage)/dialog/error-state 已截图（audit/ui-v4、ui-v4-150）。
- 分辨率/DPI：25 组合（5 分辨率×5 DPI）全部采集 Home；1280×800@150% 全页面矩阵 0 overlap/0 overflow；1280×800@200% 与 2560×1440@175% 出现 sidebar 重叠。
- 泄漏：下载主列表显示 SHA-1 与内部任务 ID；其余页面未发现 Invalid Date/NaN/undefined/raw URL/org.spongepowered/debug 等（“HealthNaNFix Mod”是模组名，非泄漏）。

## B. Loader UI 实际版本数量

- Forge 1.20.1：131；NeoForge 1.20.1：62；Fabric 1.20.1：251；Quilt 1.20.1：299。
- UI（新建实例→Forge→1.18.2）：158 个 build，含 Recommended 40.3.0 / Latest 40.3.12；搜索“40.2.21”精确返回 1 项。

## C. Modpack Detection 结果

fixture mrpack（真实 Patchouli JAR + overrides，声明 Forge 47.4.22）检测通过：format=modrinth、Minecraft 1.20.1、Forge、loaderVersion 47.4.22、Java 17、confidence 1、mods 1、overrides 2。

## D. Modpack Installation E2E

导入成功但仅提交内容：mods/Patchouli-1.20.1-85-FORGE.jar（sha256 05f7b5d5…与官方一致）、config 与 resourcepacks overrides 落盘，pack_owned_files/content_items/instance_pack_source 正确；实例 status=base_missing、loader_version=NULL、无 1.20.1 profile/launcher-effective.json → 不可运行。**FAIL。**

## E. Dependency Repair E2E

修复后实例 2（Closing Song1.6.5）：
- before：missing=[expandability, kotlinforforge]，mods=276；
- repair 返回“前置模组已全部补齐”；
- after：missing=[]，mods=278；
- kotlinforforge-4.12.0-all.jar sha1=962fdb760409…（Modrinth 官方 CDN 一致，含 JarJar 内真实 IModLanguageProvider/mods.toml）；
- expandability-9.0.4.jar sha1=2ea3c2ec…（CurseForge 官方 edge CDN 一致，CF 465066/5301414）；
- 随后 Forge 正常启动 75s 存活并到主菜单（无 missing-dep 崩溃）。**PASS。**

## F. Multiplayer Isolation 当前工作区结果

生产 UI 无服务器/联机导航；普通 Forge 启动：mods 哈希树 278 文件前后一致（7d5dc3ca…），e4mc 行 0、multiplayer_history 0、servers 0、sessions 0；启动 75s 存活。**PASS。**

## G. World Create Matrix

- Vanilla / Fabric / NeoForge：NOT_RUN（无 ready 实例，且安装需要下载整包）。
- Forge 1.20.1（47.4.10，修复依赖后）：世界“新的世界”创建成功（level.dat+16 region+4 entities+4 poi，87 文件），集成服务器启动，玩家进入（进度/坟墓/死亡提示），稳定 300s；但 Save&Quit 后服务端保存完成而 JVM 120s+ 未退出，强制终止 exit=-1 → 非 clean exit；且“创建新世界”点击未被脚本验证（游戏自动创建默认世界）。**FAIL。**

## H. Mixin Root Cause 结果

未发生真实 MixinTransformerError，端到端 NOT_RUN。单元测试 5/5：wrapper=GameMixinCrash、root=Caused by 链、mixin config/class、target class、owning JAR 候选+confidence+evidence、证据不足时不伪造确定（repair 提示“不足以唯一定位”）。

## I. Startup Window

MainVisibleConfirmed→SplashClosed→Ready（10 秒），show ACK≠visible 看门狗有效（同日捕获 ShowAckButNotVisible→Failed）。**PASS。**

## J. Evidence Freshness

- 新审计证据（audit/、world-create/、dependency-regression.json、multiplayer-isolation*.json、startup-window.json、ui-runtime.json、loader-metadata/summary.json、ui-action-gate.json、ui-layout-audit.json）均含 head+workingTreeHash+generatedAt。
- 旧证据（ui-before/、ui-after-v2/v3、旧 GATES_NEXTGEN.md、旧 UI_VISUAL_REVIEW_V2.md、acceptance-world-crash.json 等）无指纹 → STALE，不参与 PASS。
- fresh/stale/missing 具体统计见 audit/evidence-freshness.json（生成于最终报告）。

## K. Release allowed?

**RELEASE_ALLOWED = false**

计数：TOTAL_PASS=8；TOTAL_FAIL=10；TOTAL_NOT_RUN=1；TOTAL_STALE（独立 gate）=0，但 stale 证据项 >0（见 freshness 统计）。

阻塞项：BUILD_GATE(harness)、UI_LAYOUT_INTEGRITY、UI_INFORMATION_HIERARCHY、UI_VISUAL_SEMANTIC、BORDER_DENSITY、TYPOGRAPHY_RHYTHM、VISUAL_PASS_2、MODPACK_INSTALLATION_E2E、WORLD_CREATE_RUNTIME、EVIDENCE_FRESHNESS、MIXIN_DIAGNOSTICS(NOT_RUN)。
