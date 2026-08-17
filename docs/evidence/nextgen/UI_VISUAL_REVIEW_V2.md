# UI Visual Review V2 (Visual Pass 2)

> 状态：**MANUAL_REVIEW_REQUIRED**
> 本会话已生成真实 Tauri 生产截图（`ui-after-v2/`、`ui-after-v3/`），
> 但“Launcher Feel / Hierarchy / Typography”等主观评分必须由人工逐页验收后填写。
> 禁止脚本自评；本文件在人工评审完成前不作为 PASS 证据。

## 人工评审要求

每页按以下 7 项打分（0–10）并附文字理由与截图引用：

| 项目 | 评分 |
| --- | --- |
| Launcher Feel | 0–10 |
| Hierarchy | 0–10 |
| Typography | 0–10 |
| Spacing | 0–10 |
| Surface / Border | 0–10 |
| Game Content Feel | 0–10 |
| Interaction Polish | 0–10 |

## 逐页评审表

### Home

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | 需要人工查看 ui-after-v2/home.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Tutorial

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | ui-after-v2/home-tutorial.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Library

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | ui-after-v2/library.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Create Instance

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | 需要截图 create-instance-loader-build.png（Home 展开表单） |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Instance

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | 需要截图 instance-overview.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Discover

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | ui-after-v2/discover.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Downloads

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | ui-after-v2/downloads.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Accounts

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | ui-after-v2/accounts.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Settings

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | ui-after-v2/settings.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Storage / Diagnostics

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | 需要截图 settings-storage.png / diagnostics.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

### Splash

| 项目 | 评分 | 理由 |
| --- | --- | --- |
| Launcher Feel | PENDING | ui-after-v2/splash.png |
| Hierarchy | PENDING | 同上 |
| Typography | PENDING | 同上 |
| Spacing | PENDING | 同上 |
| Surface / Border | PENDING | 同上 |
| Game Content Feel | PENDING | 同上 |
| Interaction Polish | PENDING | 同上 |

## Gate 结论

```
UI_VISUAL_SEMANTIC_GATE: MANUAL_REVIEW_REQUIRED (FAIL until human review)
BORDER_DENSITY_GATE: PENDING (FAIL until human review)
TYPOGRAPHY_RHYTHM_GATE: PENDING (FAIL until human review)
VISUAL_PASS_2: PENDING (FAIL until human review)
```

## 已完成的客观修复清单（代码与截图证据）

- Home Recent 不再输出 Invalid Date / raw exit code（`formatRelativeTime` + 正常/异常状态文案）。
- Discover 已移除“Fabric/Forge/NeoForge/Quilt 检查模组说明文件”开发者文字与漂浮“联网”状态。
- Downloads 主列表不再显示 raw URL/hash（仅详情 Dialog 显示）。
- Settings 改为 SettingRow + Segmented 下载模式 + 右下角保存；非 Custom 不显示可编辑并发。
- Sidebar selected 状态改为轻量 accent 指示条；帮助链接降级；账户区去边框。
- Splash 使用透明背景 SVG 品牌图标，无白底方块。
- Tutorial/Onboarding 统一 Dialog 语言与固定 footer。
