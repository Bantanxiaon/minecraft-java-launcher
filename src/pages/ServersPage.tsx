export function ServersPage() {
  return (
    <>
      <header>
        <div>
          <h1>服务器</h1>
          <p>窗口已预留；当前没有配套服务器，联机功能暂缓开通。</p>
        </div>
        <span className="paused-label">暂缓开通</span>
      </header>
      <section className="server-window">
        <div className="server-symbol">◎</div>
        <h2>联机功能尚未开放</h2>
        <p>
          这里未来用于服务器列表、地址校验、延迟检测与连接状态。目前不会伪造服务器或提供无效连接入口。
        </p>
        <div className="server-fields">
          <input disabled placeholder="服务器地址" />
          <button disabled>连接服务器</button>
        </div>
        <small>不影响单人游戏配置、模组和本地存档管理。</small>
      </section>
    </>
  );
}
