# Updater 签名密钥

- 私钥：`TAURI_SIGNING_PRIVATE_KEY`（含密码 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`），仅存 GitHub Secrets，禁止提交仓库；本地离线备份见 `D:\SHLauncher-Private\sh-launcher-updater.key`。
- 公钥：写入 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。
- 轮换：不要直接换公钥，否则旧版本用户无法验证新签名；如需轮换，先用双公钥过渡窗口并验证旧客户端升级链路。
