import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  CircleUserRound,
  Compass,
  Download,
  House,
  LibraryBig,
  Minus,
  Settings,
  Square,
  X,
} from "lucide-react";
import type { ReactNode } from "react";
import type { Account } from "../types";
import { TOP_LEVEL_NAV } from "./Router";
import type { AppRoute } from "./Router";
import shLogo from "../assets/sh-logo.svg";
import { currentLocale, t } from "../i18n";

const NAV_KEYS = {
  home: "nav.home",
  library: "nav.library",
  discover: "nav.discover",
  downloads: "nav.downloads",
  accounts: "nav.accounts",
  settings: "nav.settings",
} as const;

const NAV_ICONS = {
  home: House,
  library: LibraryBig,
  instance: LibraryBig,
  discover: Compass,
  downloads: Download,
  accounts: CircleUserRound,
  settings: Settings,
};

type AppShellProps = {
  route: AppRoute;
  onNavigate: (route: AppRoute) => void;
  account?: Account;
  accounts: Account[];
  onSelectAccount: (accountId: number) => void;
  version: string;
  channelLabel: string;
  gameRunning: boolean;
  onOpenChangelog: () => void;
  onOpenTutorial: () => void;
  children: ReactNode;
};

export function AppShell({
  route,
  onNavigate,
  account,
  accounts,
  onSelectAccount,
  version,
  channelLabel,
  gameRunning,
  onOpenChangelog,
  onOpenTutorial,
  children,
}: AppShellProps) {
  const locale = currentLocale();
  const profileName = account?.displayName ?? "尚未创建档案";

  const runWindowAction = async (
    action: "minimize" | "maximize" | "close",
  ) => {
    if (!isTauri()) return;
    const window = getCurrentWindow();
    if (action === "minimize") await window.minimize();
    if (action === "maximize") await window.toggleMaximize();
    if (action === "close") {
      if (gameRunning) {
        await invoke("hide_launcher_window");
        return;
      }
      try {
        await window.close();
      } catch {
        await invoke("hide_launcher_window");
      }
    }
  };

  const dragWindow = (event: React.MouseEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).closest("button")) return;
    if (!isTauri()) return;
    void getCurrentWindow().startDragging().catch(() => {});
  };

  return (
    <div className="app-frame">
      <div
        className="titlebar"
        onMouseDown={dragWindow}
        onDoubleClick={() => void runWindowAction("maximize")}
      >
        <div className="titlebar-drag" data-tauri-drag-region>
          <div className="titlebar-title">
            <strong>SH启动器</strong>
            <span>
              v{version} · {channelLabel}
            </span>
          </div>
        </div>
        <div className="titlebar-controls">
          <button
            type="button"
            className="window-control"
            aria-label="最小化"
            title="最小化"
            onClick={() => void runWindowAction("minimize")}
          >
            <Minus size={16} />
          </button>
          <button
            type="button"
            className="window-control"
            aria-label="最大化或还原"
            title="最大化或还原"
            onClick={() => void runWindowAction("maximize")}
          >
            <Square size={13} />
          </button>
          <button
            type="button"
            className="window-control close"
            aria-label="关闭"
            title="关闭"
            onClick={() => void runWindowAction("close")}
          >
            <X size={17} />
          </button>
        </div>
      </div>
      <div className="shell">
        <aside className="sidebar">
          <div className="sidebar-brand">
            <div className="sidebar-brand-logo">
              <img src={shLogo} alt="SH Launcher" />
            </div>
            <div className="sidebar-brand-copy">
              <strong>SH启动器</strong>
              <small>v{version}</small>
            </div>
          </div>
          <nav className="sidebar-nav" aria-label="主导航">
            {TOP_LEVEL_NAV.map((item) => {
              const Icon = NAV_ICONS[item.name];
              const active = item.match(route);
              return (
                <button
                  key={item.name}
                  type="button"
                  className={`nav-item ${active ? "active" : ""}`}
                  aria-current={active ? "page" : undefined}
                  onClick={() => {
                    onNavigate(
                      item.name === "home"
                        ? { name: "home" }
                        : item.name === "library"
                          ? { name: "library" }
                          : item.name === "discover"
                            ? { name: "discover" }
                            : item.name === "downloads"
                              ? { name: "downloads" }
                              : item.name === "accounts"
                                ? { name: "accounts" }
                                : { name: "settings" },
                    );
                  }}
                >
                  <Icon size={19} strokeWidth={1.9} />
                  <span>
                    {t(NAV_KEYS[item.name as keyof typeof NAV_KEYS], locale)}
                  </span>
                </button>
              );
            })}
          </nav>
          <div className="sidebar-footer" aria-label="帮助">
            <small className="sidebar-footer-label">帮助</small>
            <button
              type="button"
              className="nav-item"
              onClick={onOpenChangelog}
            >
              <span>更新日志</span>
            </button>
            <button
              type="button"
              className="nav-item"
              onClick={onOpenTutorial}
            >
              <span>使用教程</span>
            </button>
            <div className="sidebar-account">
              <div className="sidebar-account-avatar">
                {account ? profileName[0]?.toUpperCase() : <CircleUserRound size={17} />}
              </div>
              <div className="sidebar-account-copy">
                <strong>{profileName}</strong>
                <small>
                  {account
                    ? account.accountType === "MICROSOFT"
                      ? "Microsoft 正版账户"
                      : account.accountType === "EXTERNAL"
                        ? "外置登录账户"
                        : "本地离线账户"
                    : "需要设置"}
                </small>
              </div>
              {accounts.length > 1 ? (
                <select
                  className="account-switcher"
                  aria-label="切换账户"
                  value={account?.id ?? ""}
                  onChange={(event) => onSelectAccount(Number(event.target.value))}
                >
                  {accounts.map((candidate) => (
                    <option key={candidate.id} value={candidate.id}>
                      {candidate.displayName}
                    </option>
                  ))}
                </select>
              ) : null}
            </div>
          </div>
        </aside>
        <main className="content">{children}</main>
      </div>
    </div>
  );
}
