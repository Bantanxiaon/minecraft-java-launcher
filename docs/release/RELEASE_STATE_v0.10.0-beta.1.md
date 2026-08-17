# RELEASE STATE — SH Launcher v0.10.0-beta.1

生成时间：2026-08-17T10:50:00Z

- Release Version: 0.10.0-beta.1
- Tag: v0.10.0-beta.1
- GitHub Release: prerelease=true
- Release Commit: 3f4a01e08730d33944349d6211c3dba143e7a4df（docs 最终提交见 Tag）
- Working Tree Hash (source state): 0c2963d3c57c49836046f3c261c79f6ebf192b398e3e41fe6706cb18e8276ead
- Binary: docs/evidence/release/release-binary.json
- Binary SHA-256: d68b1d9f6dfb4630f519dc1c1906e0ec2956ab3fb98bb8931cc95360747aa014
- Signing: unsigned（无私钥，如实记录）
- Updater artifacts: 未生成（无 TAURI_SIGNING_PRIVATE_KEY）

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
