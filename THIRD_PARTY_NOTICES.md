# Third Party Notices

SH Launcher 仅在本项目明确的参考白名单内做行为/UX/架构参考。GPL/BSL 代码禁止直接复制。

## 参考项目

| 项目 | 仓库 | 许可证 | 使用范围 |
| --- | --- | --- | --- |
| XMCL | https://github.com/Voxelum/x-minecraft-launcher | MIT | 视觉/行为/核心模块规范参考 |
| Fluent Launcher | https://github.com/Xcube-Studio/Natsurainko.FluentLauncher | MIT | Windows UX / 中国用户习惯参考 |
| Prism Launcher | https://github.com/PrismLauncher/PrismLauncher | GPL-3.0-only | 仅行为对照，不复制源码 |
| GDLauncher Carbon | https://github.com/gorilla-devs/GDLauncher-Carbon | BUSL-1.1 | 仅 UX/信息架构参考，不复制源码 |
| Modrinth | https://github.com/modrinth/code | 逐包审阅 | 仅行为/API 参考 |
| PCL Community Edition | https://github.com/PCL-Community/PCL-CE | 复杂 | 仅行为参考 |

## Rust / 前端依赖

依赖清单以 `src-tauri/Cargo.lock` 与 `package.json` 为准；供应链审计见 CI security 步骤。

> 本文件会随 NextGen 推进持续更新，直接复用的 MIT 代码必须在此登记来源文件与 commit。
