import grassBlock from "../assets/grass-block.png";

export function SplashView() {
  return (
    <main className="splash-root" data-tauri-drag-region>
      <section className="splash-card">
        <img className="splash-logo" src={grassBlock} alt="SH Launcher" />
        <h1 className="splash-title">SH启动器</h1>
        <p className="splash-subtitle">Minecraft Java Edition</p>
        <p className="splash-status" role="status" aria-live="polite">
          正在启动…
        </p>
      </section>
    </main>
  );
}
