# RELEASE STATE — SH Launcher v0.10.0-beta.1

生成时间：2026-08-17T10:50:00Z

- Release Version: 0.10.0-beta.1
- Tag: v0.10.0-beta.1
- GitHub Release: prerelease=true
- Release Commit: 3f4a01e08730d33944349d6211c3dba143e7a4df（docs 最终提交见 Tag）
- Working Tree Hash (source state): 0c2963d3c57c49836046f3c261c79f6ebf192b398e3e41fe6706cb18e8276ead
- Binary: docs/evidence/release/release-binary.json
- Binary SHA-256: 6bba526694b5eb51a303b089b9c81bd7ae12c5bacc106e5adbf1aab350383697
- Signing: updater Minisign 已签名（私钥位于私有目录，公钥与内置 pubkey 一致）；Windows Authenticode 无证书，未签名
- Updater artifacts: latest.json + .sig 已生成并发布（GitHub Release + main@release-assets/latest.json）
- End-to-End 验证: v0100_beta1_signed_installer_verifies_against_embedded_pubkey PASS
- 更新通道说明: GitHub “latest” 对 prerelease 不生效，已把 v0.9.5 Release 的 latest.json 资产同步为 beta.1
  （CDN 传播可能延迟）；指定 tag 端点与 jsdelivr main 端点即时生效。
  已发布二进制使用旧端点列表，传播完成后 beta.1 即可通过 latest 通道发现后续版本；
  未来构建将使用 tauri.conf.json 中更新后的指定 tag 端点。

## Enabled Features

Home / Library / Create Instance / Minecraft Versions / Forge / Fabric / NeoForge / Quilt / Dynamic Loader Builds / Dependency Auto Repair / Discover / Downloads / Offline Accounts / Modpack Detection / Modpack One-click Import (BETA, Modrinth + CurseForge E2E PASS)

## Disabled / Deferred

- Microsoft Login: DEFERRED / DISABLED
- Remote LAN / Servers / e4mc: DISABLED
- Generic ZIP / MMC / HMCL / MCBBS Import: EXPERIMENTAL
- World Create full matrix: DEFERRED
- High DPI 175/200%, Downloads raw SHA/Job ID, Crash confidence polish, BUILD_GATE harness, evidence freshness, visual gates: DEFERRED（见 docs/deferred/DEFERRED_WORK_GPT_5_6_SOL.md）

## Known Issues

docs/release/KNOWN_ISSUES_v0.10.0-beta.1.md

## Release Critical Gates

- PRODUCTION_BUILD: PASS（app.exe 0.10.0-beta.1；NSIS 安装包已生成；tauri build 最后 updater 签名步骤因无私钥退出 1，非产品构建失败）
- STARTUP_WINDOW: PASS
- DEPENDENCY_REPAIR_E2E: PASS
- FORGE_CORE_SMOKE: PASS
- MODPACK_RUNTIME_DETECTION: PASS
- MODPACK_INSTALLATION_E2E: PASS
- MULTIPLAYER_ISOLATION: PASS
- DATA_SAFETY: PASS
- SECRET_SCAN: PASS
- VERSION_CONSISTENCY: PASS
- BINARY_VERIFICATION: PASS
