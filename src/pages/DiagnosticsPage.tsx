import { useState } from "react";
import type { CrashReport, DownloadJob, GameLog } from "../types";

export function DiagnosticsPage({
  jobs,
  crashes,
  busy,
  message,
  onRefresh,
  onExport,
  onCancel,
  logs,
  logText,
  onReadLog,
}: {
  jobs: DownloadJob[];
  crashes: CrashReport[];
  busy: boolean;
  message: string;
  onRefresh: () => void;
  onExport: () => void;
  onCancel: () => void;
  logs: GameLog[];
  logText: string;
  onReadLog: (log: GameLog, level: string, query: string) => void;
}) {
  const [selectedLog, setSelectedLog] = useState("");
  const [level, setLevel] = useState("all");
  const [query, setQuery] = useState("");
  const readSelected = () => {
    const [instanceId, ...nameParts] = selectedLog.split(":");
    const target = logs.find((log) => log.instanceId === Number(instanceId) && log.fileName === nameParts.join(":"));
    if (target) onReadLog(target, level, query);
  };
  return (
    <>
      <header>
        <div>
          <h1>下载与诊断</h1>
          <p>真实任务记录、失败恢复动作和本地崩溃分析。</p>
        </div>
        <div className="header-actions">
          <button disabled={!busy} onClick={onCancel}>
            取消当前下载
          </button>
          <button disabled={busy} onClick={onRefresh}>
            刷新
          </button>
          <button className="primary" disabled={busy} onClick={onExport}>
            导出脱敏报告
          </button>
        </div>
      </header>
      <section className="diagnostic-grid">
        <div className="diagnostic-card">
          <div className="section-heading">
            <div>
              <h2>下载任务</h2>
              <p>最多显示最近 100 项</p>
            </div>
            <span>{jobs.length}</span>
          </div>
          {jobs.length ? (
            <div className="job-list">
              {jobs.map((job) => {
                const percent = job.totalBytes
                  ? Math.min(
                      100,
                      Math.round((job.progressBytes * 100) / job.totalBytes),
                    )
                  : undefined;
                return (
                  <div key={job.id}>
                    <div>
                      <strong>
                        {job.status === "verified"
                          ? "已校验"
                          : job.status === "failed"
                            ? "失败"
                            : "下载中"}
                      </strong>
                      <small>{job.sourceUrl}</small>
                    </div>
                    <span>{percent === undefined ? "—" : `${percent}%`}</span>
                    {job.error ? (
                      <p>
                        {job.error} · {job.recoveryAction ?? "请重试"}
                      </p>
                    ) : null}
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="empty-mods">暂无下载记录。</div>
          )}
        </div>
        <div className="diagnostic-card">
          <div className="section-heading">
            <div>
              <h2>崩溃分析</h2>
              <p>规则结果会标注置信度</p>
            </div>
            <span>{crashes.length}</span>
          </div>
          {crashes.length ? (
            <div className="crash-list">
              {crashes.map((crash) => (
                <div key={crash.id}>
                  <strong>{crash.suspectedCause}</strong>
                  <span>
                    {crash.confidence} · Exit {crash.exitCode ?? "?"}
                  </span>
                  <p>{crash.suggestion}</p>
                  <small>{crash.logPath}</small>
                </div>
              ))}
            </div>
          ) : (
            <div className="empty-mods">尚无崩溃记录。</div>
          )}
        </div>
      </section>
      <section className="game-log-card">
        <div className="section-heading"><div><h2>游戏日志</h2><p>最多读取末尾 512 KB，登录凭据和个人路径会自动隐藏。</p></div><span>{logs.length}</span></div>
        <div className="log-toolbar">
          <select aria-label="选择游戏日志" value={selectedLog} onChange={(event) => setSelectedLog(event.target.value)}>
            <option value="">选择日志</option>
            {logs.map((log) => <option key={`${log.instanceId}:${log.fileName}`} value={`${log.instanceId}:${log.fileName}`}>{log.instanceName} · {log.fileName}</option>)}
          </select>
          <select aria-label="日志等级" value={level} onChange={(event) => setLevel(event.target.value)}><option value="all">全部等级</option><option value="info">INFO</option><option value="warn">WARN</option><option value="error">ERROR</option><option value="debug">DEBUG</option></select>
          <input aria-label="搜索日志" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索日志内容" onKeyDown={(event) => { if (event.key === "Enter") readSelected(); }} />
          <button disabled={busy || !selectedLog} onClick={readSelected}>读取</button>
        </div>
        <pre>{logText || "选择一份日志后点击“读取”。"}</pre>
      </section>
      {message ? (
        <p className="mod-message" role="status">
          {message}
        </p>
      ) : null}
    </>
  );
}
