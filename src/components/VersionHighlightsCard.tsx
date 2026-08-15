import { useState } from "react";
import { Sparkles, X } from "lucide-react";
import { APP_VERSION, RELEASE_CHANNEL_LABEL } from "../version";
import { highlightsFor } from "../versionHighlights";

const DISMISS_KEY = "sh-launcher-highlights-dismissed";

export function VersionHighlightsCard({
  onOpenChangelog,
}: {
  onOpenChangelog?: () => void;
}) {
  const items = highlightsFor(APP_VERSION);
  const [dismissed, setDismissed] = useState(() => {
    try {
      return localStorage.getItem(DISMISS_KEY) === APP_VERSION;
    } catch {
      return false;
    }
  });
  if (dismissed || !items.length) return null;
  const dismiss = () => {
    setDismissed(true);
    try {
      localStorage.setItem(DISMISS_KEY, APP_VERSION);
    } catch {
      // 忽略存储失败，仅本次会话隐藏
    }
  };
  return (
    <section className="version-highlights" role="note" aria-label="版本亮点">
      <div className="version-highlights-head">
        <Sparkles size={17} />
        <strong>
          v{APP_VERSION} {RELEASE_CHANNEL_LABEL} 版本亮点
        </strong>
        <div className="version-highlights-actions">
          {onOpenChangelog ? (
            <button type="button" onClick={onOpenChangelog}>
              更新日志
            </button>
          ) : null}
          <button type="button" aria-label="关闭版本亮点" onClick={dismiss}>
            <X size={14} />
          </button>
        </div>
      </div>
      <ul>
        {items.map((item, index) => (
          <li key={index}>{item}</li>
        ))}
      </ul>
    </section>
  );
}
