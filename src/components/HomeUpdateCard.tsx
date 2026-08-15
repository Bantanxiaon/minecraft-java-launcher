import { useEffect, useRef, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import type { Update } from "@tauri-apps/plugin-updater";
import { Download, Sparkles, X } from "lucide-react";
import { LAST_UPDATE_KEY } from "../updater";

type LastUpdate = {
  version: string;
  notes?: string;
  at: string;
};

type HomeUpdateCardProps = {
  update?: Update | null;
  checking?: boolean;
  checkError?: boolean;
  onRetry?: () => void;
};

export function HomeUpdateCard({
  update,
  checking = false,
  checkError = false,
  onRetry,
}: HomeUpdateCardProps) {
  const pendingUpdate = useRef<Update | null>(null);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<number>();
  const [lastUpdate, setLastUpdate] = useState<LastUpdate>();
  const [showChangelog, setShowChangelog] = useState(false);

  useEffect(() => {
    try {
      const raw = localStorage.getItem(LAST_UPDATE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as LastUpdate;
        if (parsed?.version) {
          setLastUpdate(parsed);
          setShowChangelog(true);
        }
      }
    } catch {
      localStorage.removeItem(LAST_UPDATE_KEY);
    }
    return () => {
      void pendingUpdate.current?.close();
    };
  }, []);

  const statusText = checking
    ? "正在检查更新…"
    : checkError
      ? "暂时无法连接更新服务"
      : update
        ? "发现新版本"
        : "当前已是最新版本";

  async function installUpdate() {
    const target = update ?? pendingUpdate.current;
    if (!target || installing) return;
    setInstalling(true);
    let downloaded = 0;
    let total: number | undefined;
    let lastError: unknown;
    for (let attempt = 1; attempt <= 3; attempt += 1) {
      try {
        await target.downloadAndInstall((event) => {
          if (event.event === "Started") {
            total = event.data.contentLength;
            setProgress(total ? 0 : undefined);
          } else if (event.event === "Progress") {
            downloaded += event.data.chunkLength;
            if (total)
              setProgress(
                Math.min(100, Math.round((downloaded * 100) / total)),
              );
          } else {
            setProgress(100);
          }
        }, { timeout: 300_000 });
        localStorage.setItem(
          LAST_UPDATE_KEY,
          JSON.stringify({
            version: target.version,
            notes: target.body ?? "",
            at: new Date().toISOString(),
          } satisfies LastUpdate),
        );
        await relaunch();
        return;
      } catch (error) {
        lastError = error;
        if (attempt < 3) {
          downloaded = 0;
          total = undefined;
          setProgress(undefined);
          await new Promise((resolve) => setTimeout(resolve, 1200 * attempt));
        }
      }
    }
    if (lastError !== undefined) {
      localStorage.setItem(
        "sh-launcher-update-error",
        String(lastError),
      );
    }
    setInstalling(false);
  }

  function dismissChangelog() {
    setShowChangelog(false);
    setLastUpdate(undefined);
    localStorage.removeItem(LAST_UPDATE_KEY);
  }

  return (
    <>
      {showChangelog && lastUpdate ? (
        <div
          className="update-modal-backdrop"
          role="dialog"
          aria-modal="true"
          aria-label="更新完成"
        >
          <div className="update-modal">
            <button
              className="update-modal-close"
              type="button"
              aria-label="关闭"
              onClick={dismissChangelog}
            >
              <X size={18} />
            </button>
            <div className="update-modal-icon">
              <Sparkles size={24} />
            </div>
            <h2>更新完成</h2>
            <p className="update-modal-version">SH启动器 {lastUpdate.version}</p>
            <div className="update-modal-body">
              {lastUpdate.notes
                ? lastUpdate.notes
                    .split(/\r?\n/)
                    .filter(Boolean)
                    .map((line, index) => <p key={index}>{line}</p>)
                : <p>本次更新已安装，可以正常使用。</p>}
            </div>
            <button
              className="primary"
              type="button"
              onClick={dismissChangelog}
            >
              开始使用
            </button>
          </div>
        </div>
      ) : null}

      {update ? (
        <div className="home-update-banner" role="status">
          <div className="home-update-icon">
            <Download size={20} />
          </div>
          <div className="home-update-copy">
            <strong>发现新版本 SH启动器 {update.version}</strong>
            <small>
              {statusText === "发现新版本"
                ? "点击立即更新，游戏和存档不会受影响。"
                : statusText}
            </small>
            {update.body ? (
              <p className="home-update-notes">
                {update.body
                  .split(/\r?\n/)
                  .filter(Boolean)
                  .slice(0, 3)
                  .join(" · ")}
              </p>
            ) : null}
          </div>
          <button
            className="primary"
            type="button"
            disabled={installing || checking}
            onClick={() => void installUpdate()}
          >
            {installing
              ? progress === undefined
                ? "下载中…"
                : `更新中 ${progress}%`
              : "立即更新"}
          </button>
        </div>
      ) : null}

      {checkError && !update && !checking ? (
        <div className="home-update-banner home-update-error" role="status">
          <div className="home-update-icon">
            <Sparkles size={20} />
          </div>
          <div className="home-update-copy">
            <strong>暂时无法连接更新服务</strong>
            <small>不影响启动游戏；网络恢复后可以重新检查。</small>
          </div>
          {onRetry ? (
            <button
              className="primary"
              type="button"
              onClick={onRetry}
            >
              重新检查
            </button>
          ) : null}
        </div>
      ) : null}
    </>
  );
}
