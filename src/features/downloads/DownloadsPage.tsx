import { useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  FileArchive,
  FileText,
  X,
} from "lucide-react";
import type { DownloadJob } from "../../types";
import { Badge, Button, Progress } from "../../ui/components";

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
    case "queued":
      return "排队中";
    default:
      return status;
  }
}

function jobTone(status: string): "info" | "success" | "danger" | "default" {
  if (status === "downloading" || status === "queued") return "info";
  if (status === "completed" || status === "verified") return "success";
  if (status === "failed") return "danger";
  return "default";
}

function taskIcon(status: string, fileName: string) {
  if (status === "failed") return <AlertTriangle size={16} />;
  if (status === "completed" || status === "verified")
    return <CheckCircle2 size={16} />;
  if (/\.(jar|zip|mrpack)$/i.test(fileName)) return <FileArchive size={16} />;
  return <FileText size={16} />;
}

function friendlyFileName(job: DownloadJob): string {
  const name = job.targetPath.split(/[\\/]/).pop() || "下载文件";
  if (name.length <= 44) return name;
  return `${name.slice(0, 20)}…${name.slice(-20)}`;
}

function formatDateTime(value?: string): string {
  if (!value) return "—";
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return "—";
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

export type DownloadsPageProps = {
  jobs: DownloadJob[];
  busy: boolean;
  message: string;
  onRefresh: () => void;
  onExport: () => void;
  onCancel: () => void;
};

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
      className="dialog-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label="下载详情"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="dialog">
        <div className="dialog-head">
          <div>
            <h2>{fileName}</h2>
            <p className="download-detail-subtitle">
              {jobStatusLabel(job.status)} · {formatBytes(job.progressBytes)} /{" "}
              {formatBytes(job.totalBytes)}
            </p>
          </div>
          <button
            className="btn btn-icon"
            type="button"
            aria-label="关闭"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </div>
        <Progress
          value={percent}
          indeterminate={percent === undefined && job.status === "downloading"}
        />
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
        <div className="dialog-actions">
          <Button onClick={onClose}>关闭</Button>
        </div>
      </div>
    </div>
  );
}

export function DownloadsPage({
  jobs,
  busy,
  message,
  onRefresh,
  onExport,
  onCancel,
}: DownloadsPageProps) {
  const [selectedJob, setSelectedJob] = useState<DownloadJob>();
  const active = jobs.filter((job) => job.status === "downloading").length;
  const queued = jobs.filter((job) => job.status === "queued").length;
  const failed = jobs.filter((job) => job.status === "failed").length;
  const completed = jobs.filter(
    (job) => job.status === "completed" || job.status === "verified",
  ).length;

  return (
    <div className="ui3-page-enter">
      <header className="ui3-page-header">
        <div>
          <h1>下载中心</h1>
          <p>下载任务、速度与预计剩余时间；完整来源与校验信息在详情中查看。</p>
        </div>
        <div className="download-summary-strip">
          {active ? <Badge tone="info">{active} 下载中</Badge> : null}
          {queued ? <Badge tone="info">{queued} 排队中</Badge> : null}
          {failed ? <Badge tone="danger">{failed} 失败</Badge> : null}
          {completed ? <Badge tone="success">{completed} 已完成</Badge> : null}
        </div>
      </header>
      <div className="ui3-row downloads-actions">
        <Button size="sm" disabled={!busy} onClick={onCancel}>
          取消当前下载
        </Button>
        <Button size="sm" disabled={busy} onClick={onRefresh}>
          刷新
        </Button>
        <Button size="sm" variant="primary" disabled={busy} onClick={onExport}>
          <Download size={14} />
          导出脱敏报告
        </Button>
      </div>
      <section className="download-center">
        <div className="download-center-grid">
          <div className="download-task-list">
            {jobs.length ? (
              jobs.slice(0, 100).map((job) => {
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
                    className="download-task-row"
                    onClick={() => setSelectedJob(job)}
                    title="查看下载详情"
                  >
                    <span className="download-task-icon">
                      {taskIcon(job.status, friendlyFileName(job))}
                    </span>
                    <div className="download-task-main">
                      <strong>{friendlyFileName(job)}</strong>
                      <small>
                        {jobStatusLabel(job.status)}
                        {job.bytesPerSecond
                          ? ` · ${formatBytes(job.bytesPerSecond)}/s`
                          : ""}
                        {job.status === "downloading"
                          ? ` · 剩余 ${formatEta(job.etaSeconds)}`
                          : ""}
                      </small>
                    </div>
                    <div className="download-task-progress">
                      <Progress
                        value={percent}
                        indeterminate={
                          percent === undefined && job.status === "downloading"
                        }
                      />
                      <small className="ui3-muted">
                        {formatBytes(job.progressBytes)} /{" "}
                        {formatBytes(job.totalBytes)}
                      </small>
                    </div>
                    <div className="download-task-meta">
                      <strong>
                        {percent === undefined ? "—" : `${percent}%`}
                      </strong>
                      <small>任务 #{job.id}</small>
                    </div>
                    <Badge tone={jobTone(job.status)}>
                      {jobStatusLabel(job.status)}
                    </Badge>
                  </button>
                );
              })
            ) : (
              <div className="empty-state">
                <Download size={26} />
                <h2>暂无下载任务</h2>
                <p>安装游戏、模组或整合包时，任务会显示在这里。</p>
              </div>
            )}
          </div>
          <aside className="download-task-side">
            <div className="ui3-section-head">
              <h2>说明</h2>
            </div>
            <p className="ui3-muted">
              主列表只显示任务、进度与速度；文件来源地址、校验值与目标路径仅在详情中展示。
              失败任务会保留断点，可重新下载。
            </p>
            {failed ? (
              <p className="pack-warning">
                有 {failed} 个任务失败，点击任务行可查看恢复动作。
              </p>
            ) : null}
          </aside>
        </div>
        {message ? (
          <p className="mod-message" role="status">
            {message}
          </p>
        ) : null}
      </section>
      {selectedJob ? (
        <DownloadDetailModal
          job={selectedJob}
          onClose={() => setSelectedJob(undefined)}
        />
      ) : null}
    </div>
  );
}
