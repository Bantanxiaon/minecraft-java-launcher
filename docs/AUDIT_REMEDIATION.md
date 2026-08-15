# Audit Remediation（v0.8.0）

| 问题 | 级别 | 旧行为 | 新行为 | 文件 | 测试 |
|---|---|---|---|---|---|
| modId 盲猜 Modrinth slug | P0 | `kotlinforforge`→搜索失败，`bookshelf/prism` 可能选错平台项目 | 内置已核对 provider 别名；Fabric/Quilt provides 参与满足判断；模糊结果不自动安装 | src-tauri/src/lib.rs | missing_dependencies_reports_kotlinforforge_when_absent |
| 磁盘与 DB 双真相 | P0 | 无 reconcile | content_provenance + 存储扫描 | src-tauri/src/storage.rs | get_storage_overview（手动） |
| Offline UUID 错误 | P0 | SHA-256 前 32 hex | Java nameUUIDFromBytes 等价 MD5 v3 + legacy 字段 | src-tauri/src/lib.rs | offline_uuid_matches_java_name_uuid_from_bytes |
| display_name UNIQUE | P0 | 同名覆盖账户 | minecraft_uuid 唯一索引 | Migration v7 | duplicate_display_names_do_not_overwrite_account_identity |
| 删除实例不可恢复管理 | P1 | 仅移动目录 | deleted_instances 表 + 恢复/永久删除 | src-tauri/src/storage.rs | — |
| 下载取消全局开关 | P1 | 全局 AtomicBool | per-job CancellationToken + cancel_download_job | src-tauri/src/lib.rs | — |
| D 盘硬编码 | P1 | 强制 D 盘 | 旧数据沿用，无 D 盘用 LOCALAPPDATA | src-tauri/src/lib.rs | — |
| 一键联机 | P1 | 无 | e4mc 受管理内容 + 日志解析 | src-tauri/src/multiplayer.rs | parses_vanilla_lan_port / parses_e4mc_endpoint_lines |
