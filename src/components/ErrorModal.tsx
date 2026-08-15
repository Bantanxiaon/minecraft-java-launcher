import { X } from "lucide-react";

type ErrorModalProps = {
  title: string;
  lines: string[];
  actionLabel?: string;
  onAction?: () => void;
  secondaryLabel?: string;
  onSecondary?: () => void;
  onClose: () => void;
};

export function ErrorModal({
  title,
  lines,
  actionLabel,
  onAction,
  secondaryLabel,
  onSecondary,
  onClose,
}: ErrorModalProps) {
  return (
    <div
      className="update-modal-backdrop"
      role="alertdialog"
      aria-modal="true"
      aria-label={title}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="changelog-modal error-modal">
        <button
          className="update-modal-close"
          type="button"
          aria-label="关闭"
          onClick={onClose}
        >
          <X size={18} />
        </button>
        <h2>{title}</h2>
        <div className="error-modal-body">
          {lines.map((line, index) => (
            <p key={index}>{line}</p>
          ))}
        </div>
        <div className="error-modal-actions">
          {actionLabel && onAction ? (
            <button className="primary" type="button" onClick={onAction}>
              {actionLabel}
            </button>
          ) : null}
          {secondaryLabel && onSecondary ? (
            <button type="button" onClick={onSecondary}>
              {secondaryLabel}
            </button>
          ) : null}
          <button type="button" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}
