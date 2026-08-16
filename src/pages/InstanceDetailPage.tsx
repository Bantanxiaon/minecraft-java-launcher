import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Instance, InstanceHealth } from "../types";
import { loaderLabel } from "../ui";

type InstanceDetailPageProps = {
  instance: Instance;
  javaLabel: string;
  onBack: () => void;
  onLaunch: (instance: Instance) => void;
  onRepair: (instance: Instance) => void;
  onOpenFolder: (instance: Instance) => void;
  onMemoryChange: (instance: Instance, memoryMb: number) => void;
};

type ReconcileReportLoose = {
  instanceId: number;
  dbMissingOnDisk: string[];
  diskMissingInDb: string[];
  duplicateGroups: Array<{
    sha256: string;
    files: string[];
    keep: string;
    removableBytes: number;
  }>;
  fingerprint: string;
};

const MEMORY_PRESETS = [4096, 6144, 8192, 10240, 12288, 14336, 16384];

export function InstanceDetailPage({
  instance,
  javaLabel,
  onBack,
  onLaunch,
  onRepair,
  onOpenFolder,
  onMemoryChange,
}: InstanceDetailPageProps) {
  const [tab, setTab] = useState<"概览" | "设置" | "日志" | "对账">("概览");
  const [health, setHealth] = useState<InstanceHealth | null>(null);
  const [logs, setLogs] = useState<{ fileName: string; size: number; modifiedAt: number }[]>([]);
  const [logText, setLogText] = useState("");
  const [reconcile, setReconcile] = useState<ReconcileReportLoose | null>(null);
  const [message, setMessage] = useState("");

  const refreshHealth = useCallback(async () => {
    try {
      setHealth(await invoke<InstanceHealth>("instance_health", { instanceId: instance.id }));
    } catch (error) {
      setMessage(String(error));
    }
  }, [instance.id]);

  useEffect(() => {
    void refreshHealth();
  }, [refreshHealth]);

  async function loadLogs() {
    try {
      const all = await invoke<
        { instanceId: number; fileName: string; size: number; modifiedAt: number }[]
      >("list_game_logs");
      setLogs(all.filter((log) => log.instanceId === instance.id).slice(0, 60));
      setLogText("");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function readLog(fileName: string) {
    try {
      setLogText(
        await invoke<string>("read_game_log", {
          instanceId: instance.id,
          fileName,
          level: "",
          query: "",
        }),
      );
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function scanReconcile() {
    try {
      const report = await invoke<ReconcileReportLoose>("reconcile_scan", {
        instanceId: instance.id,
      });
      setReconcile(report);
      setMessage("");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function applyReconcile() {
    if (!reconcile) return;
    if (!window.confirm("对账将补充缺失记录并清理完全重复的 JAR（保留到备份），确认继续？")) return;
    try {
      const result = await invoke<{
        addedRecords: number;
        removedStaleRecords: number;
        deduplicatedFiles: number;
        freedBytes: number;
      }>("reconcile_apply", { instanceId: instance.id, fingerprint: reconcile.fingerprint });
      setMessage(
        `对账完成：新增 ${result.addedRecords}、清理过期记录 ${result.removedStaleRecords}、去重 ${result.deduplicatedFiles}、释放 ${Math.round(result.freedBytes / 1024 / 1024)} MB。`,
      );
      setReconcile(null);
      void refreshHealth();
    } catch (error) {
      setMessage(String(error));
    }
  }

  return (
    <>
      <header>
        <div>
          <h1>{instance.name}</h1>
          <p>
            Minecraft {instance.gameVersion} · {loaderLabel(instance.loaderType)} · {javaLabel}
          </p>
        </div>
        <button className="quiet" onClick={onBack}>返回游戏库</button>
      </header>
      <nav className="instance-tabs">
        {(["概览", "设置", "日志", "对账"] as const).map((item) => (
          <button
            key={item}
            className={tab === item ? "active" : ""}
            onClick={() => {
              setTab(item);
              setMessage("");
              if (item === "日志") void loadLogs();
              if (item === "对账") void scanReconcile();
            }}
          >
            {item}
          </button>
        ))}
      </nav>

      {tab === "概览" && health ? (
        <section className="pack-export-card">
          <h2>健康状态</h2>
          <ul className="instance-health-list">
            <li>{health.gameFilesOk ? "✓ 游戏文件完整" : "⚠ 游戏文件待安装"}</li>
            <li>{health.status === "ready" ? "✓ 实例就绪" : `状态：${health.status}`}</li>
            <li>{health.loaderType} {health.loaderVersion ?? ""} · 模组 {health.modCount} 个</li>
            {health.missingDependencies.length ? (
              <li className="danger">⚠ 缺失前置：{health.missingDependencies.join("、")}</li>
            ) : (
              <li>✓ 前置完整</li>
            )}
            {health.incompatibleMods.length ? (
              <li className="danger">⚠ 不兼容模组：{health.incompatibleMods.slice(0, 8).join("、")}</li>
            ) : null}
          </ul>
          <div className="server-form-actions">
            <button className="primary" onClick={() => onLaunch(instance)}>启动游戏</button>
            <button onClick={() => onRepair(instance)}>修复 / 校验</button>
            <button onClick={() => onOpenFolder(instance)}>打开文件夹</button>
          </div>
        </section>
      ) : tab === "概览" ? (
        <p className="mod-message">正在读取健康状态…</p>
      ) : null}

      {tab === "设置" ? (
        <section className="pack-export-card">
          <h2>实例设置</h2>
          <label className="library-memory">
            运行内存
            <select
              value={MEMORY_PRESETS.includes(instance.memoryMb) ? instance.memoryMb : "custom"}
              onChange={(event) => {
                if (event.target.value !== "custom") {
                  onMemoryChange(instance, Number(event.target.value));
                }
              }}
            >
              {MEMORY_PRESETS.map((mb) => (
                <option key={mb} value={mb}>{mb / 1024} GB</option>
              ))}
              <option value="custom">自定义</option>
            </select>
            <span>{instance.memoryMb} MB</span>
          </label>
          <p className="notice">Java：{javaLabel}（Java 选择与自动安装可在“设置 → Java”管理）。</p>
        </section>
      ) : null}

      {tab === "日志" ? (
        <section className="installed-mods">
          <div className="section-heading"><div><h2>游戏日志</h2><p>最近 {logs.length} 条</p></div></div>
          <div className="mod-rows">
            {logs.map((log) => (
              <div key={log.fileName}>
                <strong>{log.fileName}</strong>
                <span>{Math.round(log.size / 1024)} KB</span>
                <button onClick={() => void readLog(log.fileName)}>读取</button>
              </div>
            ))}
          </div>
          {logText ? <pre className="log-preview">{logText.slice(-20000)}</pre> : null}
        </section>
      ) : null}

      {tab === "对账" ? (
        <section className="pack-export-card">
          <h2>磁盘与数据库对账</h2>
          {reconcile ? (
            <>
              <p>
                数据库有但磁盘无：{reconcile.dbMissingOnDisk.length} · 磁盘有但数据库无：{reconcile.diskMissingInDb.length} · 完全重复组：{reconcile.duplicateGroups.length}
              </p>
              {reconcile.duplicateGroups.map((group) => (
                <p key={group.sha256} className="pack-warning">
                  重复（{group.sha256.slice(0, 8)}）：保留 {group.keep}，移除 {group.files.length - 1} 份，可释放 {Math.round(group.removableBytes / 1024 / 1024)} MB
                </p>
              ))}
              <div className="server-form-actions">
                <button className="primary" onClick={() => void applyReconcile()}>应用对账</button>
                <button onClick={() => void scanReconcile()}>重新扫描</button>
              </div>
            </>
          ) : (
            <p className="mod-message">点击“重新扫描”检查内容一致性。</p>
          )}
        </section>
      ) : null}
      {message ? <p className="form-message" role="status">{message}</p> : null}
    </>
  );
}
