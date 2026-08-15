# SH启动器

面向 Windows 10/11 x64 的 Minecraft: Java Edition 第三方桌面启动器。程序本身不包含 Minecraft 游戏文件；用户创建实例并确认安装后，启动器才从 Mojang 官方 HTTPS 服务下载所选版本。使用者需自行拥有合法的 Minecraft 授权。

## 已实现

- Vanilla、Fabric、Quilt、Forge、NeoForge 隔离实例的安装、校验与启动。
- 自动检测 64 位 Java；Java 17/21/25 优先从 Microsoft 官方 CDN 下载，Java 8 使用 Eclipse Temurin，全部执行 SHA-256 校验并安装到 D 盘。
- client、libraries、natives、assets 的哈希校验、断点文件、失败重试、并发下载与取消。
- 外部模组 JAR 多选/拖拽导入、兼容性检查、依赖/冲突提示、启停与可恢复移除。
- Modrinth `.mrpack`、CurseForge 导出 ZIP 和通用整合包导入；支持把自己的实例安全导出为 ZIP，默认不带存档且绝不带账户凭据。
- 模组与整合包页面可通过 Modrinth 官方 API 联网搜索，并按实例版本/加载器选择、校验和安装。
- 资源包、光影包、存档的选择/拖拽导入、校验、启停、备份、复制、导出、可恢复移除和文件夹入口。
- SQLite 持久化、下载任务、游戏运行状态事件、脱敏日志筛选、崩溃分析及诊断包。
- 运行数据固定在 `D:\MinecraftLauncherData`，不把游戏数据写入 C 盘。
- 可选择电脑里已有的 `.minecraft` 目录，优先复用通过校验的游戏文件，避免重复联网下载。
- 支持签名校验的一键云更新；免费 GitHub Releases 发布流程见 `docs/免费云更新说明.md`。

联机/服务器页面按当前产品要求保留并标注“暂缓开通”。Microsoft OAuth + PKCE 的完整后端已经保留，但当前发行包没有可合法取得的应用编号，因此普通用户界面只显示“暂未开通”，不会要求填写 Client ID 或绑卡。Offline Profile 仅适用于单机、LAN 或明确允许 `offline-mode` 的服务器，不会伪造正版身份或绕过验证。

## 开发与发行

需要 Node.js LTS、Rust stable、Visual Studio 2022 Build Tools（Desktop development with C++）和 WebView2 Runtime。

```powershell
npm.cmd install
npm.cmd run lint
npm.cmd run build
cd src-tauri
cargo test
cd ..
npm.cmd run tauri -- build --bundles nsis
```

`npm run tauri dev` 使用 `http://localhost:5173` 热更新，仅供开发。NSIS 安装包和 release EXE 使用内嵌的 `dist`，运行时不需要 localhost、Vite 或 Node.js 服务。

测试证据见 [docs/TEST_REPORT.md](docs/TEST_REPORT.md)，结构说明见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)，使用说明见 [docs/USER_GUIDE.md](docs/USER_GUIDE.md)。

本项目采用 MIT 许可证。本项目不是 Mojang Studios 或 Microsoft 的官方产品，也未获其认可或关联；Minecraft 名称及相关资产归其权利人所有。
