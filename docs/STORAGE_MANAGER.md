# Storage Manager

- 命令：`get_storage_overview`、`build_safe_cleanup_plan`、`execute_cleanup_plan`、`list_deleted_instances`、`restore_deleted_instance`、`permanently_delete_instance_backup`。
- 分类：实例、下载缓存、未完成下载、Java 运行环境/安装包、加载器安装包、日志、崩溃报告、世界备份、内容备份、已删除实例、临时文件、损坏备份。
- 安全分级：Safe（缓存/日志/临时）可直接清；Recoverable（备份）需确认；Destructive（已删除实例）强确认；InUse（运行中实例/当前 Java/进行中下载）禁止。
- 清理流程：先扫描生成带 fingerprint 的预览 → 执行前重新扫描并校验 fingerprint，不一致拒绝执行（防 TOCTOU）。
- 已删除实例：delete_instance_to_backup 记录 name/json/size 到 deleted_instances；恢复时按快照重建实例行并置 `missing` 状态，永久删除前二次确认且校验备份路径在 backups 根内。
