export function TutorialModal({ onClose }: { onClose: () => void }) {
  return (
    <div className="error-modal-backdrop" role="dialog" aria-modal="true" aria-label="使用教程">
      <div className="changelog-modal tutorial-modal">
        <button
          className="btn btn-icon"
          aria-label="关闭"
          onClick={onClose}
        >
          ×
        </button>
        <h2>SH启动器 完整使用教程</h2>
        <p className="changelog-subtitle">
          覆盖开始游戏、整合包、模组、下载与设置；所有数据保存在 D 盘，不影响系统。
        </p>
        <div className="changelog-list">
          <section>
            <h3>1. 第一次使用：创建游戏配置</h3>
            <ul>
              <li>主页点“+ 新建游戏配置”，填写名称、选择模组运行环境（原版 / Fabric / Forge / NeoForge / Quilt）和 Minecraft 版本。</li>
              <li>点“创建游戏配置”后，再点“检查并补齐游戏”下载游戏本体；有加载器时点“安装模组环境”。</li>
              <li>Java 会自动按游戏版本安装/选择（1.16 及以下用 8，1.17–1.20.4 用 17，1.20.5 及以上用 21）。</li>
              <li>全部就绪后主页点“开始游戏”。</li>
            </ul>
          </section>
          <section>
            <h3>2. 导入整合包（每个包 = 一套独立实例）</h3>
            <ul>
              <li>到“整合包”页，把 .zip / .mrpack 拖进窗口，或点“选择整合包文件”。</li>
              <li>Modrinth / CurseForge / MultiMC / HMCL / MCBBS 会自动读取游戏版本和加载器，自动创建独立实例并安装游戏、加载器与 Java。</li>
              <li>通用 ZIP 没有清单时，选择游戏版本和加载器后同样会生成独立实例。</li>
              <li>“已下载整合包”列表里可以随时把某个包再次导入为新的独立实例，或移除记录。</li>
            </ul>
          </section>
          <section>
            <h3>3. 模组管理</h3>
            <ul>
              <li>在“模组”页先选好目标游戏配置，再拖入或选择本地 .jar；也可以在“在线搜索模组”里搜索。</li>
              <li>在线搜索同时查 Modrinth 和 CurseForge，结果自动汉化名称和简介；分类显示中文。</li>
              <li>安装前会自动检查加载器和游戏版本是否匹配，不兼容会明确标红并说明原因。</li>
              <li>已安装模组支持搜索、启停、移除（可恢复）和批量更新。</li>
            </ul>
          </section>
          <section>
            <h3>4. 账户与外置登录</h3>
            <ul>
              <li>“账户”页创建本地离线档案，启动游戏时使用该档案身份。</li>
              <li>外置登录（如 LittleSkin）在“设置 → 常规 → 添加外置登录”填写 authlib 地址、用户名和密码。</li>
              <li>Microsoft 正版登录暂未开放；开放后会在“账户”页直接登录。</li>
              <li>离线账户使用标准离线 UUID，可在单机与明确允许离线身份的服务器使用。</li>
            </ul>
          </section>
          <section>
            <h3>5. 下载与进度</h3>
            <ul>
              <li>下载期间可以随意切换页面，下载不会中断。</li>
              <li>底部进度条点击后打开“下载详情”，实时显示文件名、大小、速度、剩余时间和重试次数。</li>
              <li>失败的任务会自动保留断点，可重新下载。</li>
            </ul>
          </section>
          <section>
            <h3>6. 设置</h3>
            <ul>
              <li>游戏库可修改每套实例的运行内存（4/6/8/10/12/14/16 GB 或自定义）。</li>
              <li>“界面主题”支持深色 / 浅色 / 跟随系统。</li>
              <li>“清理缓存”只删除下载缓存和临时文件，不会动游戏、模组、整合包和存档。</li>
              <li>已有游戏目录可以复用 PCL / 官方启动器正在使用的 .minecraft。</li>
            </ul>
          </section>
          <section>
            <h3>7. 更新与日志</h3>
            <ul>
              <li>启动时自动检查更新；有新版会在主页提示，点一下即可在线升级（签名校验）。</li>
              <li>“更新日志”实时显示每个版本的内容。</li>
              <li>运行日志在 D:\MinecraftLauncherData\logs\launcher.log；遇到问题可以把这个文件发给作者排查。</li>
            </ul>
          </section>
        </div>
        <div className="dialog-actions">
          <button className="primary" type="button" onClick={onClose}>
            我知道了
          </button>
        </div>
      </div>
    </div>
  );
}
