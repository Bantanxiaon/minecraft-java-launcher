type GlobalProgressBarProps = {
  visible: boolean;
  message: string;
  progress?: number;
};

export function GlobalProgressBar({
  visible,
  message,
  progress,
}: GlobalProgressBarProps) {
  if (!visible) return null;
  return (
    <div className="global-progress-bar" role="status" aria-live="polite">
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
    </div>
  );
}
