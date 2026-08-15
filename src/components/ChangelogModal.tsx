import { X } from "lucide-react";
import { changelogEntries } from "../changelog";

export function ChangelogModal({ onClose }: { onClose: () => void }) {
  const entries = changelogEntries();
  return (
    <div
      className="update-modal-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label="更新日志"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="changelog-modal">
        <button
          className="update-modal-close"
          type="button"
          aria-label="关闭"
          onClick={onClose}
        >
          <X size={18} />
        </button>
        <h2>更新日志</h2>
        <p className="changelog-subtitle">每次版本更新的“版本亮点”都会在这里汇总。</p>
        <div className="changelog-list">
          {entries.map((entry) => (
            <section key={entry.version}>
              <h3>{entry.label}</h3>
              <ul>
                {entry.items.map((item, index) => (
                  <li key={index}>{item}</li>
                ))}
              </ul>
            </section>
          ))}
        </div>
        <button className="primary" type="button" onClick={onClose}>
          关闭
        </button>
      </div>
    </div>
  );
}
