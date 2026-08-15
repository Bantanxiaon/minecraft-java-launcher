import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { changelogEntries, type ChangelogEntry } from "../changelog";

export function ChangelogModal({ onClose }: { onClose: () => void }) {
  const [entries, setEntries] = useState<ChangelogEntry[]>(changelogEntries());
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    void invoke<Array<{ version: string; label?: string; items?: string[] }>>(
      "fetch_remote_changelog",
    )
      .then((remote) => {
        if (cancelled || !Array.isArray(remote)) return;
        const merged = new Map<string, ChangelogEntry>();
        for (const entry of remote) {
          if (entry?.version && Array.isArray(entry.items)) {
            merged.set(entry.version, {
              version: entry.version,
              label: entry.label || `v${entry.version}`,
              items: entry.items.filter(Boolean),
            });
          }
        }
        for (const entry of changelogEntries()) {
          if (!merged.has(entry.version)) merged.set(entry.version, entry);
        }
        setEntries([...merged.values()]);
      })
      .catch(() => {
        // 拉取失败时保留本地日志
      });
    return () => {
      cancelled = true;
    };
  }, []);
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
