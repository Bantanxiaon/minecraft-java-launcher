import { X } from "lucide-react";
import { APP_VERSION, RELEASE_CHANNEL_LABEL } from "../version";
import { highlightsFor } from "../versionHighlights";

export function VersionHighlightsModal({ onClose }: { onClose: () => void }) {
  const items = highlightsFor(APP_VERSION);
  if (!items.length) return null;
  return (
    <div
      className="update-modal-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label="版本亮点"
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
        <h2>v{APP_VERSION} {RELEASE_CHANNEL_LABEL}</h2>
        <p className="changelog-subtitle">本次版本的亮点</p>
        <div className="changelog-list">
          <section>
            <ul>
              {items.map((item, index) => (
                <li key={index}>{item}</li>
              ))}
            </ul>
          </section>
        </div>
        <button className="primary" type="button" onClick={onClose}>
          开始使用
        </button>
      </div>
    </div>
  );
}
