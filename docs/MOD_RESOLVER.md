# Mod Resolver

- 身份优先级：pack/provider metadata → 版本依赖 metadata → hash 反查 → 本地可信映射 → 限定搜索 → 模糊候选仅展示，不静默安装。
- 已核对的 TrustedMapping（modId → Modrinth project_id）：kotlinforforge=ordsPcFz、bookshelf=uy4Cnpcm、prism=1OE8wbN0、alexscaves=U6GY0xp0、irons_spellbooks=s4OWxYQQ、tacz=SzzJttH8、expandability=X5dUUm4k、fzzy_config=hYykXjDp、l2library=4Vh3BQ3F、goety=4ZVIxU8x。
- provides：Fabric `provides[]`、Quilt `quilt_loader.provides[]` 归入 installed ids，依赖满足按主 id 或 provides 别名。
- kotlinforforge 语言加载器：启动前真实检测 mods 目录文件名，缺失时报告并可从 Modrinth 官方项目补齐。
- 安装成功后写 content_provenance（provider/projectId/versionId/fileId/sourceUrl/sha1/sha256/installedAt）。
