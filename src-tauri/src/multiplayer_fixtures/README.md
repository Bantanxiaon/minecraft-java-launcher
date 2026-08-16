# 联机日志 Fixture 来源说明

这些 fixture 用于联机模块日志解析器的回归测试。**消息正文全部逐字取自现代 e4mc 上游源码的 logger 调用**，不是凭空编造：

- 上游仓库：`vgskye/e4mc-minecraft-architectury`
- 取证提交：`5b0db932660638ebd49b0719050abf7dbcb9e5bb`（分支 `rererewrite`）
- 关键文件：
  - `common/src/main/java/link/e4mc/QuiclimeSession.java`
    - `LOGGER.info("broker req: {}", request)`
    - `LOGGER.info("broker resp: {}", response)`
    - `LOGGER.info("relaymap req: {}", request)`
    - `LOGGER.info("relaymap resp: {}", response)`
    - `LOGGER.info("using relay {}", relayInfo.id)`
    - `LOGGER.info("control channel open: {}", streamChannel)`
    - `LOGGER.info("probing capabilities")`
    - `LOGGER.info("control channel write complete")`
    - `LOGGER.info("notified server of our ticket")`
    - `LOGGER.info("Domain assigned: {}", domain)`
    - `LOGGER.error("error in e4mc", e)`
  - `common/src/main/java/link/e4mc/dialtone/DialtoneAmbientSession.java`
    - `LOGGER.info("Starting DialtoneAmbientSession!")`
  - `common/src/main/resources/assets/e4mc_minecraft/lang/en_us.json`
    - `text.e4mc_minecraft.domainAssigned`: `Local game hosted on domain [%s]`
    - `text.e4mc_minecraft.closeServer`: `Local game no longer publicly hosted`
  - `common/src/main/resources/assets/e4mc_minecraft/lang/zh_cn.json`
    - `text.e4mc_minecraft.domainAssigned`: `将本地游戏托管在域名[%s]上`
    - `text.e4mc_minecraft.closeServer`: `不再公开托管本地游戏`
- 配置默认值（`common/src/main/java/link/e4mc/Config.java`）：
  - broker：`https://broker.e4mc.link/getBestRelay`
  - relay 默认端口 `25575`，域名后缀 `.e4mc.link`

`Local game hosted on port <N>` 是 Minecraft 原版“对局域网开放”的日志行（vanilla 消息）。

**合成部分**：时间戳、线程名与 logger 前缀按各加载器的真实日志格式适配（Forge/NeoForge 为
`[e4mc/]:`，Fabric/Quilt 为 `[e4mc]:`）；示例域名 `sunset-abc.e4mc.link` 为格式合法的占位域名，
不指向任何真实主机。真实运行采集的日志到达后，应把这些占位值替换为实测样本。
