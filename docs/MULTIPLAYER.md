# Multiplayer V1（免费一键联机）

- Provider：e4mc（Modrinth 官方项目 `qANg5Jrr`），作为 Launcher Managed Content 精确安装，禁止模糊搜索首结果。
- 流程：选择实例 → `multiplayer_prepare`（缺则安装 e4mc 并写入 managed_content）→ `multiplayer_start`（以 force 模式启动游戏）→ 监视游戏日志识别“Local game hosted on port N”与 e4mc 邀请地址 → 前端显示邀请地址/结束联机。
- 结束：`multiplayer_stop` 取消监视并结束游戏；游戏退出时监视线程自动置 CLOSED。
- 安全：提示不要把地址公开发布；离线身份依赖标准 Offline UUID。
