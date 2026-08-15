import { useState } from "react";
import { X } from "lucide-react";
import type { CrashReport, DownloadJob, GameLog } from "../types";

function formatBytes(value?: number): string {
  if (value == null || !Number.isFinite(value)) return "—";
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(2)} GB`;
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KB`;
  return `${value} B`;
}

function formatEta(value?: number): string {
  if (value == null || !Number.isFinite(value) || value < 0) return "—";
  if (value < 60) return `${Math.max(1, Math.round(value))} 秒`;
  if (value < 3600) return `${Math.round(value / 60)} 分钟`;
  return `${(value / 3600).toFixed(1)} 小时`;
}

function formatDateTime(value?: string): string {
  if (!value) return "—";
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  return parsed.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

function jobStatusLabel(status: string): string {
  switch (status) {
    case "verified":
      return "已校验";
    case "failed":
      return "失败";
    case "completed":
      return "已完成";
    case "downloading":
      return "下载中";
    default:
      return status;
  }
}

function DownloadDetailModal({
  job,
  onClose,
}: {
  job: DownloadJob;
  onClose: () => void;
}) {
  const percent = job.totalBytes
    ? Math.min(100, Math.round((job.progressBytes * 100) / job.totalBytes))
    : undefined;
  const fileName = job.targetPath.split(/[\\/]/).pop() || "下载文件";
  const rows: Array<[string, string]> = [
    ["文件名", fileName],
    ["状态", jobStatusLabel(job.status)],
    ["已下载", formatBytes(job.progressBytes)],
    ["总大小", formatBytes(job.totalBytes)],
    ["百分比", percent === undefined ? "—" : `${percent}%`],
    [
      "下载速度",
      job.bytesPerSecond ? `${formatBytes(job.bytesPerSecond)}/s` : "—",
    ],
    ["预计剩余时间", formatEta(job.etaSeconds)],
    ["重试次数", String(job.retryCount)],
    ["校验值", job.expectedHash ?? "—"],
    ["创建时间", formatDateTime(job.createdAt)],
    ["开始时间", formatDateTime(job.startedAt)],
    ["最近更新", formatDateTime(job.updatedAt)],
    ["目标路径", job.targetPath],
    ["来源地址", job.sourceUrl],
  ];
  return (
    <div
      className="update-modal-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label="下载详情"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="download-detail-modal">
        <button
          className="update-modal-close"
          type="button"
          aria-label="关闭"
          onClick={onClose}
        >
          <X size={18} />
        </button>
        <h2>下载详情</h2>
        <p className="download-detail-subtitle">{fileName}</p>
        <div className="download-detail-progress">
          <div className="splash-progress-track">
            <div
              className="splash-progress-fill"
              style={{ width: `${percent ?? 0}%` }}
            />
          </div>
          <div className="download-detail-progress-meta">
            <span>{percent === undefined ? "—" : `${percent}%`}</span>
            <span>
              {formatBytes(job.progressBytes)} / {formatBytes(job.totalBytes)}
            </span>
          </div>
        </div>
        <dl className="download-detail-grid">
          {rows.map(([label, value]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd title={value}>{value}</dd>
            </div>
          ))}
        </dl>
        {job.error ? (
          <div className="download-detail-error" role="alert">
            <strong>错误信息</strong>
            <p>{job.error}</p>
            <p>{job.recoveryAction ?? "请稍后重试。"}</p>
          </div>
        ) : null}
        <button className="primary" type="button" onClick={onClose}>
          关闭
        </button>
      </div>
    </div>
  );
}

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
  const [selectedJob, setSelectedJob] = useState<DownloadJob>();
  const [selectedLog, setSelectedLog] = useState("");
  const [level, setLevel] = useState("all");
  const [query, setQuery] = useState("");
  const readSelected = () => {
    const [instanceId, ...nameParts] = selectedLog.split(":");
    const target = logs.find(
      (log) =>
        log.instanceId === Number(instanceId) &&
        log.fileName === nameParts.join(":"),
    );
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
              <p>最多显示最近 100 项，点击任务查看全部详情</p>
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
                  <button
                    key={job.id}
                    type="button"
                    className="job-row"
                    onClick={() => setSelectedJob(job)}
                    title="点击查看下载详情"
                  >
                    <div>
                      <strong>{jobStatusLabel(job.status)}</strong>
                      <small>
                        {job.targetPath.split(/[\\/]/).pop() || "下载文件"}
                      </small>
                    </div>
                    <span>{percent === undefined ? "—" : `${percent}%`}</span>
                    <p>
                      已下载 {formatBytes(job.progressBytes)} /{" "}
                      {formatBytes(job.totalBytes)}
                      {job.bytesPerSecond
                        ? ` · ${formatBytes(job.bytesPerSecond)}/s`
                        : ""}
                      {job.status === "downloading"
                        ? ` · 剩余 ${formatEta(job.etaSeconds)}`
                        : ""}
                    </p>
                    <small>{job.sourceUrl}</small>
                    {job.error ? (
                      <p>
                        {job.error} · {job.recoveryAction ?? "请重试"}
                      </p>
                    ) : null}
                  </button>
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
        <div className="section-heading">
          <div>
            <h2>游戏日志</h2>
            <p>最多读取末尾 512 KB，登录凭据和个人路径会自动隐藏。</p>
          </div>
          <span>{logs.length}</span>
        </div>
        <div className="log-toolbar">
          <select
            aria-label="选择游戏日志"
            value={selectedLog}
            onChange={(event) => setSelectedLog(event.target.value)}
          >
            <option value="">选择日志</option>
            {logs.map((log) => (
              <option
                key={`${log.instanceId}:${log.fileName}`}
                value={`${log.instanceId}:${log.fileName}`}
              >
                {log.instanceName} · {log.fileName}
              </option>
            ))}
          </select>
          <select
            aria-label="日志等级"
            value={level}
            onChange={(event) => setLevel(event.target.value)}
          >
            <option value="all">全部等级</option>
            <option value="info">INFO</option>
            <option value="warn">WARN</option>
            <option value="error">ERROR</option>
            <option value="debug">DEBUG</option>
          </select>
          <input
            aria-label="搜索日志"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索日志内容"
            onKeyDown={(event) => {
              if (event.key === "Enter") readSelected();
            }}
          />
          <button disabled={busy || !selectedLog} onClick={readSelected}>
            读取
          </button>
        </div>
        <pre>{logText || "选择一份日志后点击“读取”。"}</pre>
      </section>
      {selectedJob ? (
        <DownloadDetailModal
          job={selectedJob}
          onClose={() => setSelectedJob(undefined)}
        />
      ) : null}
      {message ? (
        <p className="mod-message" role="status">
          {message}
        </p>
      ) : null}
    </>
  );
}
