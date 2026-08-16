# v0.8.1 零遗留核对表

## 本轮已实现（源码 + 测试 + 构建证据）

- [x] 下载：持久 HTTP client（既有 OnceLock 复用，保留）
- [x] 下载：任务级 3 秒滑动窗口测速（`download_perf::SpeedMeter` + 测试）
- [x] 下载：SHA-1 对象缓存命中零联网（`object_cache_path` / `reuse_object_cache`）
- [x] 下载：404 不重试、429 Retry-After、指数退避 + 抖动（`retry_delay` + 测试）
- [x] 下载：来源健康统计与 `download_diagnostics`
- [x] 启动：独立 Splash 窗口 + 主窗口隐藏/就绪衔接（tauri.conf + App.tsx）
- [x] 启动：移除 1700ms 强制等待、假进度与步骤列表；品牌居中、一次性 Logo 动画、reduced-motion
- [x] 启动：更新检查与完整 Mod 健康扫描移出关键路径（`runBackgroundHealth`）
- [x] 对账：`reconcile_scan` / `reconcile_apply`，SHA-256 重复 JAR 清理到备份 + fingerprint 防 TOCTOU
- [x] 安全解压：`fs_safe::extract_zip_securely` + 测试，Java 安装已接入
- [x] 旧版启动参数分词器：引号/转义/空格路径 + 测试
- [x] Supervisor（方案 B）：关闭后隐藏窗口，游戏退出后进程才退出；运行中关闭按钮自动隐藏
- [x] i18n：隐藏未完成的 English 入口
- [x] 移除 lottie 主视觉与依赖，前端包约减半
- [x] Resolver 身份链：可信别名 → SHA-1 hash 反查（Modrinth version_file）→ 精确搜索；同名多候选返回 AMBIGUOUS，未知 modId 不静默安装（“第 11 个未知 Mod”测试）
- [x] FsTransaction（move + LIFO 回滚）用于删除实例，DB 失败时回滚文件移动
- [x] staging 残留扫描/清理命令（list_staging_operations / cleanup_staging_operation）
- [x] mock HTTP 回归：404 不重试、下载回退/退避测试
- [x] 下载基准工具 `scripts/benchmark-download.mjs`（可本机执行并记录 PCL 对照）
- [x] 前端 Vitest 扩展到 6 项（版本范围/分词需求/highlights/loader）
- [x] 实例克隆：补齐 libraries/assets、内存、分辨率，复制后置为待校验状态（不继承 READY），运行中禁止复制

## 仍未完成（如实列出，不宣告零遗留）

- [ ] 整合包导入 staging/事务化与崩溃恢复未完成
- [ ] Content/Modpack UpdatePlan 未完成（Mod 更新仍是备份后替换）
- [ ] 全部解压路径未统一到 SecureArchiveExtractor（仅 Java 已接入，其余保留既有等价防护）
- [ ] Instance-centric UI（实例详情钻取式 IA）未完成
- [ ] 故障注入（DB commit 失败、导入 N/100 失败、clone DB 失败、migration 中断）
- [ ] 真实 70MB A/B benchmark 与 PCL 同机对照（需在真实机器执行；工具已就绪）
- [ ] 多显示器/100/125/150% DPI 实测（需真实 Windows 环境）
- [ ] 外部：无 Cloudflare R2 凭据，自有 CDN 未实际上传；真机多显示器/DPI 实测未执行
