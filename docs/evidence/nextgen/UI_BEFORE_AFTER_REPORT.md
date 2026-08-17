# SH Launcher UI 3.0 Before / After 报告

> 全部截图来自真实 Tauri 生产 EXE（`src-tauri/target/release/app.exe`，`tauri build --no-bundle`），
> 非浏览器 localhost / Storybook / Figma / mock HTML。

## 运行信息

- DEV_HEAD：`4c908c80e9d3dd0b7ff6601d72d82624fa1f8cd8`
- 版本：package/cargo/tauri.conf 均为 0.9.5；Tauri 2.11.5（Cargo.lock 解析）
- Before EXE：旧 UI（HEAD 生产构建）；After EXE：UI3 生产构建

## 截图像素证据（客观指标）

| 页面 | Before 平均 RGB | After 平均 RGB | Before 深色占比 | After 深色占比 | 变化 |
|---|---|---|---|---|---|
| home | 237,240,239（白） | 33,36,43（深） | 1.6% | 93.4% | 白→深，紫色点缀 4.3% |
| library | 见 ui-before/library.png | 见 ui-after/library.png | — | — | 卡片暗色化 |
| instance-overview | 见 ui-before/instance.png | 见 ui-after/instance-overview.png | — | — | 新 Hero + 健康状态 |
| downloads | 见 ui-before/downloads.png | 见 ui-after/downloads.png | — | — | 下载中心 + 诊断 |
| accounts | 见 ui-before/accounts.png | 见 ui-after/accounts.png | — | — | 账户卡暗色化 |
| settings | 见 ui-before/settings.png | 见 ui-after/settings-general.png | — | — | 6 Tab 拆分 |

## 逐项回答

| 问题 | 结论 |
|---|---|
| Shell 变了吗？ | 变。新 Titlebar + 228px Sidebar（6 个一级入口）+ 账户区 + 窗口控制；旧白底 shell 不再渲染 |
| Nav 变了吗？ | 变。首页/游戏库/发现/下载/账户/设置，无“联机/服务器”入口；选中态为蓝紫 accent |
| Home 变了吗？ | 变。当前实例 Hero + 开始游戏为第一优先级；最近游戏/当前下载/新建实例/更新/注意降为次级 |
| Library 变了吗？ | 变。卡片网格暗色化、封面、状态、启动/详情/修复/复制/重命名/移除 |
| Instance 变了吗？ | 变。7 个真实 Tab：概览/模组/资源包/光影/存档/日志与诊断/设置 |
| Downloads 变了吗？ | 变。真实下载中心：任务状态/进度/速度/ETA/错误恢复 + 来源健康 + 崩溃诊断 |
| Settings 变了吗？ | 变。6 Tab：常规/游戏与 Java/下载与网络/存储/更新/高级 |
| 卡片密度是否降低？ | 是。去掉大量同级白卡，改为信息分层 + 单一主 CTA |
| 旧白绿后台感是否消失？ | 是。首页白底占比 93.8%→1.9%，绿点占比 1.4%→0.2% |
| XMCL Launcher 气质是否明显？ | 是。深色 surface 分层、左导航、实例中心、克制 accent、圆角/动效体系 |

## 文本/布局适配修复（本会话二次迭代）

- 发现页：在线结果卡补齐 `catalog-card` 布局（图标/标题/作者/简介两行截断/分类/安装按钮），
  修复了纯文本堆叠问题；网格 `minmax(260px,1fr)` 随窗口自适应。
- 下载页：任务行恢复专属卡片样式（排除全局 button 样式干扰）、速度/ETA/来源/错误恢复对齐；
  下载详情弹窗补齐进度条、字段网格（dl）、任务状态与 indeterminate 动画。
- 崩溃列表、游戏日志输入框与日志预览补齐 UI3 样式。
- 新增 1200/1100/860px 三级响应式断点：页面边距、网格列数、实例 Hero、
  新建实例表单与整合包通用导入表单自动重排。

## 结论

`XMCL_VISUAL_DIRECTION = PASS`（before ≈ after 不成立，差异为整页级重做）。

## 截图清单

- Before：`docs/evidence/nextgen/ui-before/{splash,home,library,instance,downloads,accounts,settings}.png`
- After：`docs/evidence/nextgen/ui-after/{splash,home,library,instance-overview,instance-mods,instance-worlds,discover,downloads,accounts,settings-general,settings-java,settings-download,storage,diagnostics,error-state,dialog}.png`
- DPI：`docs/evidence/nextgen/ui-after/dpi-{1-00,1-25,1-50,1-75,2-00}-home.png`
