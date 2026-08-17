import type { LauncherError } from "./types";

export const navItems = [
  "主页",
  "游戏库",
  "发现",
  "下载",
  "账户",
  "设置",
];

export const loaderOptions = [
  "vanilla",
  "fabric",
  "forge",
  "neoforge",
  "quilt",
];

export const loaderLabel = (loader: string) =>
  ({
    vanilla: "Vanilla",
    fabric: "Fabric",
    forge: "Forge",
    neoforge: "NeoForge",
    quilt: "Quilt",
  })[loader] ?? loader;

const versionParts = (value: string) =>
  (value.match(/\d+/g) ?? []).map((part) => Number(part));

const compareVersions = (left: number[], right: number[]) => {
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference) return Math.sign(difference);
  }
  return 0;
};

export const gameVersionMatches = (
  requirement: string,
  actual: string,
): boolean => {
  const rule = requirement.trim();
  const current = versionParts(actual);
  if (!rule || rule === "*" || /^\$\{.+\}$/.test(rule) || !current.length) {
    return !rule || rule === "*" || /^\$\{.+\}$/.test(rule);
  }
  if (/^[[(].*,.*[\])]$/.test(rule)) {
    const lowerInclusive = rule.startsWith("[");
    const upperInclusive = rule.endsWith("]");
    const [lower, upper] = rule.slice(1, -1).split(",", 2);
    const lowerOrder = lower ? compareVersions(current, versionParts(lower)) : 1;
    const upperOrder = upper ? compareVersions(current, versionParts(upper)) : -1;
    return (
      (!lower || lowerOrder > 0 || (lowerInclusive && lowerOrder === 0)) &&
      (!upper || upperOrder < 0 || (upperInclusive && upperOrder === 0))
    );
  }
  if (/^\[[^,]+\]$/.test(rule))
    return compareVersions(current, versionParts(rule)) === 0;
  if (/\s/.test(rule))
    return rule.split(/\s+/).every((part) => gameVersionMatches(part, actual));
  const comparison = rule.match(/^(>=|<=|>|<|=)(.+)$/);
  if (comparison) {
    const order = compareVersions(current, versionParts(comparison[2]));
    return comparison[1] === ">="
      ? order >= 0
      : comparison[1] === "<="
        ? order <= 0
        : comparison[1] === ">"
          ? order > 0
          : comparison[1] === "<"
            ? order < 0
            : order === 0;
  }
  if (/[x*]/i.test(rule)) {
    const tokens = rule.split(/[.\-_]/);
    const wildcard = tokens.findIndex((part) => /^(x|\*)$/i.test(part));
    const expected = tokens
      .slice(0, wildcard)
      .map(Number);
    return expected.length > 0 && expected.every((part, index) => current[index] === part);
  }
  if (rule.startsWith("~")) {
    const base = versionParts(rule.slice(1));
    const upper = [...base];
    const index = upper.length > 1 ? 1 : 0;
    upper[index] += 1;
    upper.length = index + 1;
    return compareVersions(current, base) >= 0 && compareVersions(current, upper) < 0;
  }
  return compareVersions(current, versionParts(rule)) === 0;
};

export const inspectionSupportsGame = (
  requirements: string[],
  gameVersion: string,
) =>
  requirements.length === 0 ||
  requirements.some((requirement) => gameVersionMatches(requirement, gameVersion));

export const errorText = (error: unknown, fallback: string) =>
  error instanceof Error
    ? error.message
    : ((error as LauncherError)?.message ?? fallback);
