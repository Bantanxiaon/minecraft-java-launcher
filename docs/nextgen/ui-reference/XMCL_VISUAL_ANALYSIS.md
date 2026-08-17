# XMCL 主视觉分析（前置研究，非实现）

> 研究源：Voxelum/x-minecraft-launcher（XMCL）公开文档与 UI 结构，执行时间 2026-08-17。
> 本文件仅为 UI3 设计方向的前置分析；不复制 XMCL 商标、品牌资产或源码。

## 1. 总体气质

- 深色为默认基准，支持 Light / System 双模式；语义色独立存储。
- 扁平、克制的卡片层级：主要使用 surface 分层而非描边堆叠。
- 左 sidebar + 顶部栏 + 内容区；sidebar 可自定义，包含启动器级入口与账户区。
- 实例（Instance）是核心对象：首页/库都以“当前实例 + 启动”为最高优先级。
- 下载/设置/账户均以清晰分组、低噪点排版呈现，避免企业后台仪表盘感。

## 2. 页面组织映射到 SH UI3

| XMCL 组织方式 | SH UI3 落点 |
|---|---|
| 左侧导航（图标+文字，选中高亮） | `AppShell` sidebar：6 个一级入口 |
| 当前实例 Hero + 开始按钮 | `HomePage` home-hero（实例名/版本/加载器/Java/状态 + 主 CTA） |
| 实例卡片（封面图、名称、版本、状态） | `InstanceLibraryPage` library-card（UI3 重排） |
| 实例详情分区（内容/设置/日志等） | `InstancePage` 7 个 Tab（概览/模组/资源包/光影/存档/日志/设置） |
| 下载中心（任务、进度、来源） | `DownloadsPage`（真实任务记录、速度、ETA、失败恢复） |
| 设置分组（通用/Java/下载/存储/更新/高级） | `SettingsPage` 6 个 Tab |
| 账户管理 | `AccountsPage`（只显示真实能力：离线 / 外置登录） |
| 深色体系（bg/surface/elevated/strong、语义色） | `tokens.css`（规范 §8 色板） |
| 动效节奏（hover 100–140ms、页面 180–260ms） | `motion.css` + `prefers-reduced-motion` |
| 圆角体系 | `radius-sm/md/lg/xl = 6/10/14/18` |

## 3. 关键视觉规则（已进入 UI3 实现）

1. 首页第一优先级 = 当前实例 + 开始游戏；不堆叠同级 SaaS 卡片。
2. 一级导航最多 6 项，无“服务器/联机”正式入口。
3. Accent（#7569FF）只用于 CTA/选中/进度，不铺满全屏。
4. 不使用黑金、电竞霓虹、赛博朋克、大面积 RGB 渐变或廉价玻璃拟态。
5. 实例中心（Instance Overview/Mods/Resource Packs/Shaders/Worlds/Logs/Settings）全部真实可用。
6. 下载页为真实 Transfer Center（任务名/类型/来源/进度/速度/ETA/错误/恢复动作）。
7. 账户页只显示真实能力；Microsoft 未开放时显示明确非交互状态。
8. 设置拆分为 General / Game & Java / Download & Network / Storage / Update / Advanced。
