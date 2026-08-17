// UI 3.0 internal router: production routes for SH Launcher.
// Navigation is intentionally limited to the six top-level destinations;
// multiplayer / servers surfaces are not part of this registry.

export type DiscoverTab = "mods" | "modpacks" | "resourcepacks" | "shaders";
export type InstanceTab =
  | "overview"
  | "mods"
  | "resourcepacks"
  | "shaders"
  | "worlds"
  | "logs"
  | "settings";
export type SettingsTab =
  | "general"
  | "game"
  | "download"
  | "storage"
  | "update"
  | "advanced";

export type AppRoute =
  | { name: "home" }
  | { name: "library" }
  | { name: "instance"; instanceId: number; tab?: InstanceTab }
  | { name: "discover"; tab?: DiscoverTab }
  | { name: "downloads" }
  | { name: "accounts" }
  | { name: "settings"; tab?: SettingsTab };

export type RouteName = AppRoute["name"];

export const TOP_LEVEL_NAV: Array<{
  name: RouteName;
  label: string;
  match: (route: AppRoute) => boolean;
}> = [
  { name: "home", label: "首页", match: (route) => route.name === "home" },
  {
    name: "library",
    label: "游戏库",
    match: (route) =>
      route.name === "library" ||
      (route.name === "instance" && route.tab === undefined),
  },
  {
    name: "discover",
    label: "发现",
    match: (route) => route.name === "discover",
  },
  {
    name: "downloads",
    label: "下载",
    match: (route) => route.name === "downloads",
  },
  {
    name: "accounts",
    label: "账户",
    match: (route) => route.name === "accounts",
  },
  {
    name: "settings",
    label: "设置",
    match: (route) => route.name === "settings",
  },
];

export const DISCOVER_TABS: Array<{ id: DiscoverTab; label: string }> = [
  { id: "mods", label: "模组" },
  { id: "modpacks", label: "整合包" },
  { id: "resourcepacks", label: "资源包" },
  { id: "shaders", label: "光影" },
];

export const INSTANCE_TABS: Array<{ id: InstanceTab; label: string }> = [
  { id: "overview", label: "概览" },
  { id: "mods", label: "模组" },
  { id: "resourcepacks", label: "资源包" },
  { id: "shaders", label: "光影" },
  { id: "worlds", label: "存档" },
  { id: "logs", label: "日志与诊断" },
  { id: "settings", label: "设置" },
];

export const SETTINGS_TABS: Array<{ id: SettingsTab; label: string }> = [
  { id: "general", label: "常规" },
  { id: "game", label: "游戏与 Java" },
  { id: "download", label: "下载与网络" },
  { id: "storage", label: "存储" },
  { id: "update", label: "更新" },
  { id: "advanced", label: "高级" },
];

export const routeToLabel = (route: AppRoute): string => {
  if (route.name === "instance") {
    const tab = INSTANCE_TABS.find((candidate) => candidate.id === route.tab);
    return tab ? tab.label : "实例";
  }
  if (route.name === "discover") {
    const tab = DISCOVER_TABS.find((candidate) => candidate.id === route.tab);
    return tab ? tab.label : "发现";
  }
  if (route.name === "settings") {
    const tab = SETTINGS_TABS.find((candidate) => candidate.id === route.tab);
    return tab ? tab.label : "设置";
  }
  return TOP_LEVEL_NAV.find((item) => item.name === route.name)?.label ?? "";
};
