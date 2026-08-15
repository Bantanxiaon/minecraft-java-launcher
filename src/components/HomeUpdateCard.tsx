import { useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Download, Sparkles, X } from "lucide-react";

const updaterEnabled = import.meta.env.VITE_SH_UPDATES_ENABLED === "true";
const LAST_UPDATE_KEY = "sh-launcher-last-update";

type LastUpdate = {
  version: string;
  notes?: string;
  at: string;
};

export function HomeUpdateCard() {
  const pendingUpdate = useRef<Update | null>(null);
  const [update, setUpdate] = useState<Update>();
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<number>();
  const [status, setStatus] = useState<string>();
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
    if (!isTauri() || !updaterEnabled) return;
    let cancelled = false;
    setChecking(true);
    void check({ timeout: 20_000 })
      .then((found) => {
        if (cancelled) return;
        setUpdate(found ?? undefined);
        setStatus(found ? "发现新版本" : "当前已是最新版本");
      })
      .catch(() => {
        if (!cancelled) setStatus("暂时无法连接更新服务");
      })
      .finally(() => {
        if (!cancelled) setChecking(false);
      });
    return () => {
      cancelled = true;
      void pendingUpdate.current?.close();
    };
  }, []);

  async function installUpdate() {
    const target = update ?? pendingUpdate.current;
    if (!target || installing) return;
    setInstalling(true);
    setStatus("正在下载并验证更新…");
    let downloaded = 0;
    let total: number | undefined;
    try {
      await target.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength;
          setProgress(total ? 0 : undefined);
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total) setProgress(Math.min(100, Math.round(downloaded * 100 / total)));
        } else {
          setProgress(100);
        }
      }, { timeout: 120_000 });
      localStorage.setItem(
        LAST_UPDATE_KEY,
        JSON.stringify({
          version: target.version,
          notes: target.body ?? "",
          at: new Date().toISOString(),
        } satisfies LastUpdate),
      );
      setStatus("更新已安装，正在重新打开启动器…");
      await relaunch();
    } catch {
      setStatus("更新没有安装成功，旧版本仍可正常使用。");
      setInstalling(false);
    }
  }

  function dismissChangelog() {
    setShowChangelog(false);
    setLastUpdate(undefined);
    localStorage.removeItem(LAST_UPDATE_KEY);
  }

  return (
    <>
      {showChangelog && lastUpdate ? (
        <div className="update-modal-backdrop" role="dialog" aria-modal="true" aria-label="更新完成">
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
            <button className="primary" type="button" onClick={dismissChangelog}>
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
            <small>{status === "发现新版本" ? "点击立即更新，游戏和存档不会受影响。" : status}</small>
            {update.body ? (
              <p className="home-update-notes">{update.body.split(/\r?\n/).filter(Boolean).slice(0, 3).join(" · ")}</p>
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
    </>
  );
}
