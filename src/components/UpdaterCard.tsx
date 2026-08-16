import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

const updaterEnabled = import.meta.env.VITE_SH_UPDATES_ENABLED === "true";
const LAST_UPDATE_KEY = "sh-launcher-last-update";

export function UpdaterCard() {
  const pendingUpdate = useRef<Update | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<number>();
  const [version, setVersion] = useState<string>();
  const [notes, setNotes] = useState<string>();
  const [status, setStatus] = useState(
    updaterEnabled ? "可以检查 GitHub 上发布的新版本。" : "免费更新组件已就绪，首次发布到 GitHub 后启用。",
  );

  useEffect(() => () => {
    void pendingUpdate.current?.close();
  }, []);

  async function checkForUpdate() {
    if (!isTauri() || !updaterEnabled || checking || installing) return;
    setChecking(true);
    setStatus("正在检查新版本…");
    try {
      await pendingUpdate.current?.close();
      const update = await check({ timeout: 20_000 });
      pendingUpdate.current = update;
      if (!update) {
        setVersion(undefined);
        setNotes(undefined);
        setStatus("当前已经是最新版。");
        return;
      }
      setVersion(update.version);
      setNotes(update.body);
      setStatus(`发现 SH启动器 ${update.version}`);
    } catch {
      setStatus("暂时无法连接更新服务，不影响启动游戏；稍后可以重新检查。");
    } finally {
      setChecking(false);
    }
  }

  async function installUpdate() {
    const update = pendingUpdate.current;
    if (!update || installing) return;
    setInstalling(true);
    setStatus("正在下载并验证更新…");
    let downloaded = 0;
    let total: number | undefined;
    const unlisten = await listen<{
      downloaded: number;
      total: number;
      speed: number;
      url: string;
    }>("update-progress", (event) => {
      downloaded = event.payload.downloaded;
      if (event.payload.total) total = event.payload.total;
      if (total) setProgress(Math.min(100, Math.round((downloaded * 100) / total)));
    }).catch(() => () => {});
    try {
      localStorage.setItem(
        LAST_UPDATE_KEY,
        JSON.stringify({
          version: update.version,
          notes: update.body ?? "",
          at: new Date().toISOString(),
        }),
      );
      await invoke("install_update_fast");
      setStatus("更新已安装，正在重新打开启动器…");
      await relaunch();
    } catch {
      localStorage.removeItem(LAST_UPDATE_KEY);
      setStatus("更新没有安装成功，旧版本仍可正常使用。请稍后重试。");
      setInstalling(false);
    } finally {
      unlisten();
    }
  }

  return (
    <div className="updater-card">
      <div>
        <strong>启动器更新</strong>
        <small>{status}</small>
        {notes ? <small className="update-notes">{notes}</small> : null}
        {progress !== undefined ? <progress max={100} value={progress}>{progress}%</progress> : null}
      </div>
      {version ? (
        <button className="primary" type="button" disabled={installing} onClick={() => void installUpdate()}>
          {installing ? `更新中${progress === undefined ? "" : ` ${progress}%`}` : `更新到 ${version}`}
        </button>
      ) : (
        <button type="button" disabled={!updaterEnabled || checking || installing} onClick={() => void checkForUpdate()}>
          {checking ? "检查中…" : "检查更新"}
        </button>
      )}
    </div>
  );
}
