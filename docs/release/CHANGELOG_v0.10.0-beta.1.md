# SH Launcher v0.10.0 Beta 1 更新日志

## 主要更新

- 全新的 UI 3.0 与启动器页面结构（Home / Library / Instance / Discover / Downloads / Accounts / Settings）。
- Minecraft / Forge / NeoForge / Fabric / Quilt 版本动态发现，支持大量历史 Loader Build 搜索与选择。
- 新增整合包一键导入 Beta：自动识别 Minecraft、加载器、Loader 版本与 Java 并自动建立运行环境；
  Modrinth .mrpack 与 CurseForge ZIP 已通过真实导入→启动到主菜单 E2E。
- 改进 Mod 前置依赖检测与自动修复（KotlinForForge、Expandability 等真实闭环）。
- 改进 Java、实例和启动流程；缺失前置未补齐时阻止启动，不再允许绕过。
- 改进启动窗口稳定性（主窗口真实可见确认后才关闭启动小窗）。
- 改进下载与崩溃诊断。
- 修复“上次游玩时间显示为未知”的问题，最近游戏现在显示真实相对时间。

## 已知限制

- 部分复杂或非标准整合包仍可能存在兼容性问题。
- Microsoft 正版账号登录暂未作为正式功能开放。
- 远程联机功能暂未开放。
- 部分高 Windows 显示缩放比例可能存在个别布局问题。
- Beta 阶段复杂 Mod 组合仍可能存在兼容性问题。
