# 架构说明

## 运行边界

- React/TypeScript 负责桌面视图、表单、拖拽和命令调用；页面组件按诊断、服务器、设置和内容管理拆分。
- Rust/Tauri 负责所有受信任操作：D 盘目录、SQLite、网络与哈希、ZIP 安全、Java/游戏进程和日志。
- SQLite 保存账户元数据、非敏感设置、实例、安装状态、下载任务、内容索引和游玩记录，不保存明文令牌。
- release 构建将 `dist` 嵌入应用；`devUrl: localhost:5173` 只在开发模式使用。调试验收模块通过 `debug_assertions` 条件编译，不进入发行版。

```text
src/App.tsx                    UI 编排与跨页面状态
src/pages/                    页面组件
src/types.ts / src/ui.ts      共享类型与显示映射
src-tauri/src/lib.rs          核心命令、安装、导入与启动领域逻辑
src-tauri/src/diagnostics.rs  诊断与崩溃报告
src-tauri/src/acceptance.rs   仅 debug 编译的端到端验收驱动
```

## 性能与可靠性

- 复用网络客户端，连接超时与总超时受控；429/5xx/瞬时错误指数退避重试。
- 资源下载并发受设置限制，进度事件节流；页面仅在需要显示进度时更新状态。
- 下载先写 `.part`，校验通过后原子完成；取消后保留可续传数据。
- 加载器依赖先由启动器并发预取，再调用官方安装器，降低安装器串行网络失败率。
- 在线内容搜索只连接 Modrinth 官方 API，按项目类型/游戏版本/加载器构造 facets；安装文件仍须经过可信 CDN、大小和 SHA-1 检查。
- 依赖按最终路径去重，避免重复类进入类路径。
- 外部 ZIP/JAR 拒绝路径穿越、符号链接、异常条目数和解压炸弹。
- 所有实例和运行时相互隔离，Java 临时目录也位于实例的 D 盘缓存内。

## 启动流程

```text
选择实例 → 获取并校验官方版本元数据 → 校验/下载 client、libraries、assets、natives
→ 安装/合并加载器元数据 → 检查精确主版本且为 64 位的 Java
→ 生成参数数组 → 启动并捕获日志 → 记录退出码 → 生成可恢复诊断
```

启动器不包含游戏本体。Microsoft 登录未来应使用系统浏览器 Authorization Code + PKCE，并将凭据写入 Windows Credential Manager；在完整授权链路实现前不开放入口。
