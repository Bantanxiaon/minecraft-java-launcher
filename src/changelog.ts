import { highlightsFor } from "./versionHighlights";

export type ChangelogEntry = {
  version: string;
  label: string;
  items: string[];
};

const KNOWN_VERSIONS = [
  "0.4.1",
  "0.4.0",
  "0.3.0",
  "0.2.0",
  "0.1.4",
];

function labelFor(version: string): string {
  return /^0\./.test(version) ? `v${version} Beta` : `v${version} 正式版`;
}

export function changelogEntries(): ChangelogEntry[] {
  return KNOWN_VERSIONS.map((version) => ({
    version,
    label: labelFor(version),
    items: highlightsFor(version),
  })).filter((entry) => entry.items.length > 0);
}
