# Test Report（2026-08-16，v0.8.0 本地执行）

- pnpm lint：通过（3 条既有告警：HomeUpdateCard ref 清理、App.tsx 2 处 unsafe-finally，均为历史代码，非本次引入）
- pnpm test（Vitest）：4/4 通过（gameVersionMatches 范围/运算符/模板占位符、loaderLabel）
- pnpm build（tsc + vite）：通过
- cargo fmt --check：通过
- cargo clippy --all-targets --all-features -- -D warnings：通过（0 告警）
- cargo test：44 通过，8 忽略（联网/真实包测试按需运行）
  - 关键回归：kotlinforforge 缺失报告、Offline UUID 与 Java fixture 一致、同名不同身份共存、e4mc/LAN 日志解析、迁移 v1–v8
- 真包测试（落幕曲 1.6.5，497MB）：CurseForge / Forge / 1.20.1 / 266 模组 / 5846 覆盖文件，识别 0.43 秒
- 联网测试：Modrinth 搜索、CurseForge 搜索（代理）、CurseForge 21.8MB 真实下载、authlib-injector 下载校验：全部通过
