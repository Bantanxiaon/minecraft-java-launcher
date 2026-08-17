import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import shLogo from "../assets/sh-logo.svg";
import { Button } from "../ui/components";

type StartupErrorPayload = {
  classification?: string;
  message?: string;
  action?: string;
};

export function SplashView() {
  const [error, setError] = useState("");
  const [retrying, setRetrying] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    void listen<StartupErrorPayload>("startup-window-error", (event) => {
      if (disposed) return;
      setError(
        event.payload.message ??
          "主窗口启动失败，可点击“重试”重新尝试。",
      );
    }).then((unlisten) => {
      if (disposed) unlisten();
    });
    return () => {
      disposed = true;
    };
  }, []);

  async function retry() {
    setRetrying(true);
    setError("");
    try {
      await invoke("startup_ready");
    } catch {
      setError("重试失败，请关闭后重新打开启动器。");
    } finally {
      setRetrying(false);
    }
  }

  return (
    <main className="splash-root ui3-splash" data-tauri-drag-region>
      <section className="splash-card">
        <img className="splash-logo" src={shLogo} alt="SH Launcher" />
        <h1 className="splash-title">SH启动器</h1>
        <p className="splash-subtitle">Minecraft Java Edition</p>
        <div className="splash-track">
          <span style={{ width: error ? "100%" : "62%" }} />
        </div>
        {error ? (
          <div className="splash-error" role="alert">
            {error}
            <div style={{ marginTop: 10 }}>
              <Button
                variant="primary"
                size="sm"
                disabled={retrying}
                onClick={() => void retry()}
              >
                {retrying ? "重试中…" : "重试"}
              </Button>
            </div>
          </div>
        ) : (
          <p className="splash-status" role="status" aria-live="polite">
            正在启动…
          </p>
        )}
      </section>
    </main>
  );
}
