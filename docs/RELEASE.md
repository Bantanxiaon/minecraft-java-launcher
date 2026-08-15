# Release

- 版本唯一来源：`scripts/sync-version.mjs <version>` 同步 package.json / Cargo.toml / tauri.conf.json。
- 流程：CI（lint/test/build + cargo fmt/clippy/test）→ 打 tag `vX.Y.Z` → release.yml 构建签名 NSIS → GitHub Release → 上传 latest.json → 推送 release-assets 到 main 供 jsDelivr 分发 → 发布后 purge jsDelivr latest manifest。
- 发布物：`SHLauncher_setup.exe`、`latest.json`、`.sig`、SHA256SUMS。不再发布“EXE 改名 .zip”的伪 ZIP。
