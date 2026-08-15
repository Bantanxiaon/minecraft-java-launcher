type GlobalProgressBarProps = {
  visible: boolean;
  message: string;
  progress?: number;
  onClick?: () => void;
};

export function GlobalProgressBar({
  visible,
  message,
  progress,
  onClick,
}: GlobalProgressBarProps) {
  if (!visible) return null;
  return (
    <button
      className="global-progress-bar"
      type="button"
      role="status"
      aria-live="polite"
      title="点击查看下载详情"
      onClick={onClick}
    >
      <div className="global-progress-copy">
        <span className="global-progress-message">{message || "正在处理…"}</span>
        {progress !== undefined ? (
          <span className="global-progress-percent">
            {Math.max(0, Math.min(100, Math.round(progress)))}%
          </span>
        ) : null}
      </div>
      <div className="global-progress-track">
        <div
          className={
            progress === undefined
              ? "global-progress-fill global-progress-indeterminate"
              : "global-progress-fill"
          }
          style={
            progress === undefined
              ? undefined
              : { width: `${Math.max(0, Math.min(100, progress))}%` }
          }
        />
      </div>
    </button>
  );
}
