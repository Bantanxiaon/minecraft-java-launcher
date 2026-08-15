import type { DownloadJob, Instance } from "../types";

function formatBytes(value?: number): string {
  if (!value) return "0 B";
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(2)} GB`;
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KB`;
  return `${value} B`;
}

function formatEta(seconds?: number): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return "—";
  if (seconds < 60) return `${Math.round(seconds)} 秒`;
  if (seconds < 3600) {
    const minutes = Math.floor(seconds / 60);
    return `${minutes} 分 ${Math.round(seconds % 60)} 秒`;
  }
  const hours = Math.floor(seconds / 3600);
  return `${hours} 小时 ${Math.round((seconds % 3600) / 60)} 分`;
}

function fileBaseName(path: string): string {
  const cleaned = path.replace(/\\/g, "/");
  return cleaned.split("/").pop() ?? path;
}

export function DownloadDetailsModal({
  jobs,
  instanceProgress,
  instances,
  onClose,
}: {
  jobs: DownloadJob[];
  instanceProgress: Record<number, number>;
  instances: Instance[];
  onClose: () => void;
}) {
  const active = jobs.filter((job) => job.status === "downloading");
  const others = jobs.filter((job) => job.status !== "downloading").slice(0, 50);
  const rows = [...active, ...others];
  const totalBytes = active.reduce(
    (sum, job) => sum + (job.totalBytes ?? job.progressBytes),
    0,
  );
  const downloadedBytes = active.reduce(
    (sum, job) => sum + job.progressBytes,
    0,
  );
  const totalPercent = totalBytes
    ? Math.min(100, Math.round((downloadedBytes * 100) / totalBytes))
    : undefined;
  const instanceRows = Object.entries(instanceProgress)
    .filter(([, percent]) => percent > 0 && percent < 100)
    .map(([id, percent]) => ({
      instanceId: Number(id),
      percent,
    }));
  return (
    <div className="error-modal-backdrop" role="dialog" aria-modal="true" aria-label="下载详情">
      <div className="download-detail-modal">
        <button className="download-detail-close" aria-label="关闭" onClick={onClose}>
          ×
        </button>
        <h2>下载详情</h2>
        <p className="download-detail-subtitle">
          {active.length
            ? `共 ${active.length} 个下载目标，总进度 ${totalPercent ?? "—"}%，速度与进度实时更新。`
            : "当前没有进行中的下载；下面是最新任务记录。"}
        </p>
        {totalPercent !== undefined ? (
          <div className="download-detail-progress">
            <div className="global-progress-track">
              <div
                className="global-progress-fill"
                style={{ width: `${totalPercent}%` }}
              />
            </div>
            <div className="download-detail-progress-meta">
              <span>全部任务 {totalPercent}%</span>
              <span>
                {formatBytes(downloadedBytes)} / {formatBytes(totalBytes)}
              </span>
            </div>
          </div>
        ) : null}
        <div className="download-detail-list">
          {rows.length ? rows.map((job) => {
            const percent = job.totalBytes
              ? Math.min(100, Math.round((job.progressBytes * 100) / job.totalBytes))
              : job.status === "downloading"
                ? undefined
                : 100;
            return (
              <div className="download-detail-row" key={job.id}>
                <div className="download-detail-head">
                  <strong>{fileBaseName(job.targetPath)}</strong>
                  <span className={`job-status ${job.status}`}>
                    {job.status === "downloading"
                      ? "下载中"
                      : job.status === "verified"
                        ? "已完成"
                        : job.status === "failed"
                          ? "失败"
                          : job.status}
                  </span>
                </div>
                <div className="download-detail-progress">
                  <div
                    className={
                      percent === undefined
                        ? "global-progress-fill global-progress-indeterminate"
                        : "global-progress-fill"
                    }
                    style={percent === undefined ? undefined : { width: `${percent}%` }}
                  />
                </div>
                <div className="download-detail-meta">
                  <span>
                    {formatBytes(job.progressBytes)}
                    {job.totalBytes ? ` / ${formatBytes(job.totalBytes)}` : ""}
                    {percent !== undefined ? `（${percent}%）` : ""}
                  </span>
                  <span>速度 {formatBytes(job.bytesPerSecond)}/s</span>
                  <span>剩余 {formatEta(job.etaSeconds)}</span>
                  <span>重试 {job.retryCount} 次</span>
                </div>
                {job.error ? (
                  <p className="download-detail-error" title={job.error}>
                    {job.error}
                    {job.recoveryAction ? ` · ${job.recoveryAction}` : ""}
                  </p>
                ) : null}
              </div>
            );
          }) : (
            <p className="download-detail-empty">还没有下载任务。</p>
          )}
          {instanceRows.length ? (
            <>
              <h3 className="download-detail-group-title">实例安装进度</h3>
              {instanceRows.map((row) => {
                const instance = instances.find(
                  (candidate) => candidate.id === row.instanceId,
                );
                return (
                  <div
                    className="download-detail-row"
                    key={`instance-${row.instanceId}`}
                  >
                    <div className="download-detail-head">
                      <strong>
                        {instance?.name ?? `实例 #${row.instanceId}`}
                      </strong>
                      <span className="job-status downloading">安装中</span>
                    </div>
                    <div className="download-detail-progress">
                      <div
                        className="global-progress-fill"
                        style={{
                          width: `${Math.max(0, Math.min(100, row.percent))}%`,
                        }}
                      />
                    </div>
                    <div className="download-detail-meta">
                      <span>{Math.round(row.percent)}%</span>
                      <span>游戏文件 / 加载器 / Java 安装</span>
                    </div>
                  </div>
                );
              })}
            </>
          ) : null}
        </div>
        <button className="primary" type="button" onClick={onClose}>
          关闭
        </button>
      </div>
    </div>
  );
}
