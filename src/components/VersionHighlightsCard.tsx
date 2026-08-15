import { Sparkles } from "lucide-react";
import { APP_VERSION, RELEASE_CHANNEL_LABEL } from "../version";
import { highlightsFor } from "../versionHighlights";

export function VersionHighlightsCard() {
  const items = highlightsFor(APP_VERSION);
  if (!items.length) return null;
  return (
    <section className="version-highlights" role="note" aria-label="版本亮点">
      <div className="version-highlights-head">
        <Sparkles size={17} />
        <strong>
          v{APP_VERSION} {RELEASE_CHANNEL_LABEL} 版本亮点
        </strong>
      </div>
      <ul>
        {items.map((item, index) => (
          <li key={index}>{item}</li>
        ))}
      </ul>
    </section>
  );
}
