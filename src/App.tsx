import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  Account,
  BackupItem,
  ContentItem,
  DownloadProgress,
  ImportedLocalPack,
  ImportedModpack,
  Instance,
  JavaRuntime,
  LauncherSettings,
  ModInspection,
  ModUpdateInfo,
  ModpackInspection,
  RemovedContent,
  VersionManifest,
  VersionSummary,
  DownloadJob,
  CrashReport,
  ExportResult,
  GameLog,
  OnlineProject,
  ServerEntry,
  ModpackArchive,
} from "./types";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import { ServersPage } from "./pages/ServersPage";
import { SettingsPage } from "./pages/SettingsPage";
import { StoragePage } from "./pages/StoragePage";
import { AccountsPage } from "./pages/AccountsPage";
import { InstanceLibraryPage } from "./pages/InstanceLibraryPage";
import { InstanceDetailPage } from "./pages/InstanceDetailPage";
import { HomeUpdateCard } from "./components/HomeUpdateCard";
import { SplashView } from "./components/SplashScreen";
import { VersionHighlightsModal } from "./components/VersionHighlightsModal";
import { ChangelogModal } from "./components/ChangelogModal";
import { TutorialModal } from "./components/TutorialModal";
import { OnboardingGuide } from "./components/OnboardingGuide";
import { ErrorModal } from "./components/ErrorModal";
import { IncompatibleModsModal } from "./components/IncompatibleModsModal";
import { GlobalProgressBar } from "./components/GlobalProgressBar";
import { DownloadDetailsModal } from "./components/DownloadDetailsModal";
import { checkForUpdate, updaterEnabled } from "./updater";
import type { Update } from "./updater";
import type {
  BootHealthReport,
} from "./types/splash";
import { APP_VERSION, RELEASE_CHANNEL_LABEL } from "./version";
import { highlightsFor } from "./versionHighlights";
import {
  ArchiveContentPage,
  ComingSoonPage,
  ModpacksPage,
  ModsPage,
  WorldsPage,
} from "./pages/ContentPages";
import {
  errorText,
  inspectionSupportsGame,
  loaderLabel,
  loaderOptions,
  navItems,
} from "./ui";
import grassBlock from "./assets/grass-block.png";
import {
  CircleUserRound,
  Compass,
  Download,
  FolderOpen,
  Gamepad2,
  House,
  LibraryBig,
  Play,
  Puzzle,
  Settings,
  ShieldCheck,
  Coffee,
  CheckCircle2,
  CircleAlert,
  Minus,
  Square,
  X,
} from "lucide-react";
import "./App.css";
import "./overrides.css";
import ui2Css from "./ui2.css?inline";
import "./ui/tokens.css";
import "./ui/shell.css";

function formatBytes(value: number): string {
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(2)} GB`;
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KB`;
  return `${value} B`;
}

function javaMajorForGameVersion(gameVersion?: string): number | undefined {
  if (!gameVersion) return undefined;
  const parts = gameVersion.split(".").map((part) => Number(part));
  if (parts.some((part) => !Number.isFinite(part))) return undefined;
  if (parts[0] >= 2) return 21;
  if (parts[0] !== 1) return undefined;
  if ((parts[1] ?? 0) <= 16) return 8;
  if ((parts[1] ?? 0) === 17) return 17;
  if ((parts[1] ?? 0) === 18 || (parts[1] ?? 0) === 19) return 17;
  if ((parts[1] ?? 0) === 20) return (parts[2] ?? 0) >= 5 ? 21 : 17;
  return 21;
}

const CHINESE_SEARCH_ALIASES: Record<string, string[]> = {
  暮色森林: ["twilight forest"],
  匠魂: ["tinkers construct"],
  机械动力: ["create"],
  血魔法: ["blood magic"],
  植物魔法: ["botania"],
  神秘时代: ["thaumcraft"],
  星系: ["galacticraft"],
  应用能源: ["applied energistics"],
  沉浸工程: ["immersive engineering"],
  农夫乐事: ["farmer's delight"],
  饰品: ["curios"],
  枪械: ["tacz", "gun"],
  拔刀剑: ["slash blade"],
  冰与火: ["ice and fire"],
  暮色: ["twilight forest"],
  幸运方块: ["lucky block"],
  高清修复: ["optifine"],
  光影: ["shaders", "iris"],
  小地图: ["minimap", "journeymap", "xaero"],
  背包: ["backpack"],
  存储: ["storage", "refined storage"],
  食物: ["food", "farmer's delight"],
  科技: ["technology", "mekanism", "create"],
  冒险: ["adventure"],
  魔法: ["magic", "botania", "blood magic"],
  整合包: ["modpack"],
};

function expandSearchQueries(query: string): string[] {
  const trimmed = query.trim();
  if (!trimmed) return [""];
  const queries = [trimmed];
  for (const [chinese, english] of Object.entries(CHINESE_SEARCH_ALIASES)) {
    if (trimmed.includes(chinese)) {
      for (const alias of english) {
        if (!queries.includes(alias)) queries.push(alias);
      }
      break;
    }
  }
  return queries.slice(0, 2);
}

type BootProblem = {
  instanceName?: string;
  severity: "warn" | "error";
  text: string;
};

function DesktopTitleBar() {
  const runWindowAction = async (
    action: "minimize" | "maximize" | "close",
  ) => {
    if (!isTauri()) return;
    const window = getCurrentWindow();
    if (action === "minimize") await window.minimize();
    if (action === "maximize") await window.toggleMaximize();
    if (action === "close") {
      const gameRunning =
        document.documentElement.dataset.gameRunning === "true";
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
    <div
      className="desktop-titlebar"
      onMouseDown={dragWindow}
      onDoubleClick={() => void runWindowAction("maximize")}
    >
      <span className="desktop-title" data-tauri-drag-region>
        SH启动器
      </span>
      <div className="window-controls">
        <button
          aria-label="最小化"
          title="最小化"
          onClick={() => void runWindowAction("minimize")}
        >
          <Minus size={16} />
        </button>
        <button
          aria-label="最大化或还原"
          title="最大化或还原"
          onClick={() => void runWindowAction("maximize")}
        >
          <Square size={13} />
        </button>
        <button
          className="window-close"
          aria-label="关闭"
          title="关闭"
          onClick={() => void runWindowAction("close")}
        >
          <X size={17} />
        </button>
      </div>
    </div>
  );
}

export default function App() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [selectedAccountId, setSelectedAccountId] = useState<number>();
  const [servers, setServers] = useState<ServerEntry[]>([]);
  const [modpackArchives, setModpackArchives] = useState<ModpackArchive[]>([]);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [selectedInstanceId, setSelectedInstanceId] = useState<number>();
  const [openInstanceId, setOpenInstanceId] = useState<number>();
  const [versions, setVersions] = useState<VersionSummary[]>([]);
  const [javaRuntimes, setJavaRuntimes] = useState<JavaRuntime[]>([]);
  const [selectedJavaPath, setSelectedJavaPath] = useState<string>();
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [message, setMessage] = useState("");
  const [showDownloadDetails, setShowDownloadDetails] = useState(false);
  const [bootUpdate, setBootUpdate] = useState<Update | null | undefined>(
    undefined,
  );
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateCheckError, setUpdateCheckError] = useState(false);
  const [bootProblems, setBootProblems] = useState<BootProblem[]>([]);
  const [incompatibleGroups, setIncompatibleGroups] = useState<
    Array<{
      instanceId: number;
      instanceName: string;
      mods: Array<{ fileName: string; reason: string }>;
    }>
  >([]);
  const [modProblemMaps, setModProblemMaps] = useState<
    Record<number, Record<string, string>>
  >({});
  const [showChangelog, setShowChangelog] = useState(false);
  const [showTutorial, setShowTutorial] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [showHighlights, setShowHighlights] = useState(false);
  const [errorModal, setErrorModal] = useState<{
    title: string;
    lines: string[];
    actionLabel?: string;
    action?: () => void;
    secondaryLabel?: string;
    onSecondary?: () => void;
  } | null>(null);
  const bootCancelledRef = useRef(false);
  const [activeNav, setActiveNav] = useState("主页");
  const activeNavRef = useRef(activeNav);
  const DISCOVER_TABS = ["模组", "整合包", "资源包", "光影", "存档"] as const;
  const [, setDiscoverTab] = useState<(typeof DISCOVER_TABS)[number]>("模组");
  const [showInstanceForm, setShowInstanceForm] = useState(false);
  const [instanceName, setInstanceName] = useState("");
  const [gameVersion, setGameVersion] = useState("");
  const [instanceLoader, setInstanceLoader] = useState("vanilla");
  const [downloadProgress, setDownloadProgress] = useState<
    Record<number, number>
  >({});
  const [clientReady, setClientReady] = useState<Record<number, boolean>>({});
  const [loaderVersions, setLoaderVersions] = useState<
    Record<number, string[]>
  >({});
  const [loaderSelections, setLoaderSelections] = useState<
    Record<number, string>
  >({});
  const [modInspection, setModInspection] = useState<ModInspection>();
  const [modSourcePath, setModSourcePath] = useState("");
  const [modQueue, setModQueue] = useState<
    Array<{ path: string; inspection: ModInspection }>
  >([]);
  const [modInstanceId, setModInstanceId] = useState<number>();
  const [modItems, setModItems] = useState<ContentItem[]>([]);
  const [modUpdates, setModUpdates] = useState<ModUpdateInfo[]>([]);
  const [removedBackups, setRemovedBackups] = useState<BackupItem[]>([]);
  const [packInspection, setPackInspection] = useState<ModpackInspection>();
  const [packSourcePath, setPackSourcePath] = useState("");
  const [dragging, setDragging] = useState(false);
  const [settings, setSettings] = useState<LauncherSettings>({
    downloadConcurrency: 16,
    closeLauncherAfterGameStart: false,
    language: "zh-CN",
    defaultMemoryMb: 4096,
    microsoftClientId: "",
    backupWorldsBeforeLaunch: false,
    uiTheme: "modern",
  });
  const [archiveItems, setArchiveItems] = useState<ContentItem[]>([]);
  const [worldItems, setWorldItems] = useState<ContentItem[]>([]);
  const [downloadJobs, setDownloadJobs] = useState<DownloadJob[]>([]);
  const [crashReports, setCrashReports] = useState<CrashReport[]>([]);
  const [gameLogs, setGameLogs] = useState<GameLog[]>([]);
  const [gameLogText, setGameLogText] = useState("");
  const [onlineModQuery, setOnlineModQuery] = useState("");
  const [onlinePackQuery, setOnlinePackQuery] = useState("");
  const [onlineModLoader, setOnlineModLoader] = useState("");
  const [onlineModVersion, setOnlineModVersion] = useState("");
  const [onlineModProjects, setOnlineModProjects] = useState<OnlineProject[]>([]);
  const [onlinePackProjects, setOnlinePackProjects] = useState<OnlineProject[]>([]);
  const [microsoftLoginAvailable, setMicrosoftLoginAvailable] = useState(false);
  const [gameRunning, setGameRunning] = useState(false);
  const [isSplash, setIsSplash] = useState(false);
  const current = accounts.find((account) => account.id === selectedAccountId) ?? accounts[0];
  const activeDownloadJobs = downloadJobs.filter(
    (job) => job.status === "downloading",
  );
  const aggregateDownloadPercent = activeDownloadJobs.length
    ? Math.round(
        (activeDownloadJobs.reduce(
          (sum, job) => sum + job.progressBytes,
          0,
        ) /
          Math.max(
            1,
            activeDownloadJobs.reduce(
              (sum, job) => sum + (job.totalBytes ?? job.progressBytes),
              0,
            ),
          )) *
          100,
      )
    : undefined;

  useEffect(() => {
    activeNavRef.current = activeNav;
  }, [activeNav]);

  useEffect(() => {
    if (isTauri()) {
      setIsSplash(getCurrentWindow().label === "splash");
    }
  }, []);

  useEffect(() => {
    // 统一新 UI：不再保留经典界面主题；历史 "classic" 设置也一律使用新主题。
    document.documentElement.dataset.uiTheme = "modern";
    const existing = document.getElementById("ui2-theme");
    if (!existing) {
      const style = document.createElement("style");
      style.id = "ui2-theme";
      style.textContent = ui2Css;
      document.head.appendChild(style);
    }
  }, []);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let cancelled = false;
    const wait = (ms: number) =>
      new Promise<void>((resolve) => setTimeout(resolve, ms));
    void (async () => {
      const startedAt = Date.now();
      const MIN_SPLASH_MS = 350;
      try {
        await runBootChecks();
      } finally {
        const remaining = Math.max(0, MIN_SPLASH_MS - (Date.now() - startedAt));
        await wait(remaining);
        if (cancelled) return;
        try {
          if (localStorage.getItem("sh-onboarding-seen") !== "1") {
            setShowOnboarding(true);
          }
        } catch {
          setShowOnboarding(true);
        }
        const HIGHLIGHTS_SEEN_KEY = "sh-launcher-highlights-seen";
        try {
          if (
            localStorage.getItem(HIGHLIGHTS_SEEN_KEY) !== APP_VERSION &&
            highlightsFor(APP_VERSION).length
          ) {
            setShowHighlights(true);
          }
        } catch {
          // 存储不可用时本次启动直接弹出
          setShowHighlights(true);
        }
        // 窗口交接完全由 Rust 侧 StartupWindowCoordinator 负责：
        // 前端只在这里通知“bootstrap 就绪”，不直接操作窗口。
        if (isTauri()) {
          await invoke("startup_ready").catch(() => {});
        }
      }
    })();
    return () => {
      cancelled = true;
      bootCancelledRef.current = true;
    };
    // 启动检查只执行一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function runBootChecks() {
    const [
      accountsResult,
      accountStateResult,
      instancesResult,
      javaResult,
      settingsResult,
      loginResult,
      serversResult,
      archivesResult,
    ] = await Promise.allSettled([
      invoke<Account[]>("list_accounts"),
      invoke<{ activeAccountId?: number; defaultAccountId?: number }>(
        "get_account_state",
      ),
      invoke<Instance[]>("list_instances"),
      invoke<JavaRuntime[]>("detect_java_runtimes"),
      invoke<LauncherSettings>("get_settings"),
      invoke<boolean>("microsoft_login_available"),
      invoke<ServerEntry[]>("list_servers"),
      invoke<ModpackArchive[]>("list_modpack_archives"),
    ]);

    if (accountsResult.status === "fulfilled") {
      const savedAccounts = accountsResult.value;
      setAccounts(savedAccounts);
      const persisted =
        accountStateResult.status === "fulfilled"
          ? accountStateResult.value.activeAccountId ??
            accountStateResult.value.defaultAccountId
          : undefined;
      const restored = persisted && savedAccounts.some((account) => account.id === persisted)
        ? persisted
        : savedAccounts[0]?.id;
      setSelectedAccountId(restored);
    } else {
      setMessage(errorText(accountsResult.reason, "无法读取账户。"));
    }

    if (instancesResult.status === "fulfilled") {
      const savedInstances = instancesResult.value;
      setInstances(savedInstances);
      setModInstanceId(savedInstances[0]?.id);
      setSelectedInstanceId(
        savedInstances.find((instance) => instance.status === "ready")?.id ??
          savedInstances[0]?.id,
      );
    } else {
      setMessage(errorText(instancesResult.reason, "无法读取游戏库。"));
    }

    if (javaResult.status === "fulfilled") {
      const detectedJava = javaResult.value;
      setJavaRuntimes(detectedJava);
      setSelectedJavaPath(
        detectedJava.find((runtime) => runtime.is64Bit)?.path,
      );
    }

    if (settingsResult.status === "fulfilled") {
      setSettings(settingsResult.value);
    } else {
      setMessage(errorText(settingsResult.reason, "无法读取设置。"));
    }

    if (loginResult.status === "fulfilled") {
      setMicrosoftLoginAvailable(loginResult.value);
    }

    if (serversResult.status === "fulfilled") {
      setServers(serversResult.value);
    } else {
      setMessage(errorText(serversResult.reason, "无法读取服务器列表。"));
    }

    if (archivesResult.status === "fulfilled") {
      setModpackArchives(archivesResult.value);
    } else {
      setMessage(errorText(archivesResult.reason, "无法读取已下载整合包列表。"));
    }

    // 健康检查（完整 Mod 扫描）不阻塞首屏，Main 显示后后台执行。
    void runBackgroundHealth();

    if (!isTauri() || !updaterEnabled) {
    } else {
      // 更新检查在后台异步进行，不拖慢启动动画
      void runUpdateCheck();
    }
  }

  async function runBackgroundHealth() {
    if (!isTauri()) return;
    try {
      const report = await invoke<BootHealthReport>("boot_health_check");
      if (bootCancelledRef.current) return;
      const nameById = new Map(
        report.instances.map((item) => [item.id, item.name]),
      );
      const problems: BootProblem[] = [];
      for (const summary of report.mods) {
        const instanceName =
          nameById.get(summary.instanceId) ?? `实例 ${summary.instanceId}`;
        if (summary.missingDependencies.length) {
          problems.push({
            instanceName,
            severity: "warn",
            text: `缺少前置模组：${summary.missingDependencies
              .slice(0, 5)
              .join("、")}（启动时会自动补齐）`,
          });
        }
        if (summary.incompatibleMods.length) {
          problems.push({
            instanceName,
            severity: "error",
            text: `${summary.incompatibleMods.length} 个模组与当前加载器/游戏版本不兼容：${summary.incompatibleMods
              .slice(0, 4)
              .map((mod) => `${mod.fileName}（${mod.reason}）`)
              .join("；")}。可一键删除不兼容模组。`,
          });
        }
      }
      setBootProblems(problems);
      setIncompatibleGroups(
        report.mods
          .filter((summary) => summary.incompatibleMods.length)
          .map((summary) => ({
            instanceId: summary.instanceId,
            instanceName:
              nameById.get(summary.instanceId) ?? `实例 ${summary.instanceId}`,
            mods: summary.incompatibleMods,
          })),
      );
      const problemMaps: Record<number, Record<string, string>> = {};
      for (const summary of report.mods) {
        if (summary.problemMods.length) {
          problemMaps[summary.instanceId] = Object.fromEntries(
            summary.problemMods.map((mod) => [mod.fileName, mod.reason]),
          );
        }
      }
      setModProblemMaps(problemMaps);
    } catch {
      // 健康检查失败不阻塞启动器使用
    }
  }

  async function runUpdateCheck() {
    setUpdateChecking(true);
    setUpdateCheckError(false);
    try {
      const found = await checkForUpdate(12_000);
      if (bootCancelledRef.current) return;
      setBootUpdate(found ?? null);
      setUpdateCheckError(false);
    } catch {
      if (bootCancelledRef.current) return;
      setUpdateCheckError(true);
      setBootUpdate(null);
    } finally {
      setUpdateChecking(false);
    }
  }

  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    const disposers: Array<() => void> = [];
    const register = async () => {
      disposers.push(await listen("game-preparing", () => {
        setMessage("正在准备 Minecraft…");
      }));
      disposers.push(await listen("game-running", () => {
        setGameRunning(true);
        document.documentElement.dataset.gameRunning = "true";
      }));
      disposers.push(await listen<{ exitCode?: number }>("game-exited", (event) => {
        setGameRunning(false);
        delete document.documentElement.dataset.gameRunning;
        setMessage(`Minecraft 已正常退出${event.payload.exitCode == null ? "" : `（代码 ${event.payload.exitCode}）`}。`);
      }));
      disposers.push(await listen<{ exitCode?: number }>("game-crashed", (event) => {
        setGameRunning(false);
        delete document.documentElement.dataset.gameRunning;
        setMessage(`Minecraft 异常退出（代码 ${event.payload.exitCode ?? "未知"}），可在“下载”页查看崩溃建议。`);
      }));
      if (cancelled) disposers.splice(0).forEach((dispose) => dispose());
    };
    void register();
    return () => {
      cancelled = true;
      disposers.splice(0).forEach((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    setModUpdates([]);
    if (!isTauri() || !modInstanceId) {
      setModItems([]);
      return;
    }
    invoke<ContentItem[]>("list_content_items", {
      instanceId: modInstanceId,
      kind: "mod",
    })
      .then(setModItems)
      .catch((error: unknown) =>
        setMessage(errorText(error, "无法读取模组列表。")),
      );
  }, [modInstanceId]);

  useEffect(() => {
    const kind =
      activeNav === "资源包"
        ? "resourcepack"
        : activeNav === "光影"
          ? "shaderpack"
          : undefined;
    if (!isTauri() || !modInstanceId || !kind) {
      setArchiveItems([]);
      return;
    }
    invoke<ContentItem[]>("list_content_items", {
      instanceId: modInstanceId,
      kind,
    })
      .then(setArchiveItems)
      .catch((error: unknown) =>
        setMessage(errorText(error, "无法读取内容列表。")),
      );
  }, [activeNav, modInstanceId]);

  useEffect(() => {
    const kind = activeNav === "模组" ? "mod"
      : activeNav === "资源包" ? "resourcepack"
      : activeNav === "光影" ? "shaderpack"
      : activeNav === "存档" ? "world" : undefined;
    if (!isTauri() || !modInstanceId || !kind) {
      setRemovedBackups([]);
      return;
    }
    invoke<BackupItem[]>("list_removed_backups", { instanceId: modInstanceId, kind })
      .then(setRemovedBackups)
      .catch((error: unknown) => setMessage(errorText(error, "无法读取可恢复备份。")));
  }, [activeNav, modInstanceId]);

  useEffect(() => {
    if (!isTauri() || !modInstanceId || activeNav !== "存档") {
      setWorldItems([]);
      return;
    }
    invoke<ContentItem[]>("list_content_items", {
      instanceId: modInstanceId,
      kind: "world",
    })
      .then(setWorldItems)
      .catch((error: unknown) =>
        setMessage(errorText(error, "无法读取存档列表。")),
      );
  }, [activeNav, modInstanceId]);

  useEffect(() => {
    if (!isTauri()) return;
    const timer = window.setInterval(() => {
      invoke<DownloadJob[]>("list_download_jobs")
        .then(setDownloadJobs)
        .catch(() => {});
    }, 3000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let dispose: (() => void) | undefined;
    let cancelled = false;
    void listen<DownloadProgress>("download-progress", (event) => {
      const {
        instanceId,
        downloadedBytes,
        totalBytes,
        jobId,
        sourceUrl,
        fileName,
        speedBytesPerSecond,
        etaSeconds,
      } = event.payload;
      const percent = totalBytes
        ? Math.min(100, Math.round((downloadedBytes * 100) / totalBytes))
        : 0;
      setDownloadProgress((existing) => ({
        ...existing,
        [instanceId]: percent,
      }));
      if (jobId != null) {
        setDownloadJobs((existing) => {
          const updated = existing.map((job) =>
            job.id === jobId
              ? {
                  ...job,
                  progressBytes: downloadedBytes,
                  totalBytes: totalBytes ?? job.totalBytes,
                  bytesPerSecond: speedBytesPerSecond,
                  etaSeconds,
                }
              : job,
          );
          if (updated.some((job) => job.id === jobId)) return updated;
          const fallback: DownloadJob = {
            id: jobId,
            sourceUrl: sourceUrl ?? "",
            targetPath: fileName ? `…/${fileName}` : "下载文件",
            progressBytes: downloadedBytes,
            totalBytes,
            retryCount: 0,
            status: "downloading",
            createdAt: new Date().toISOString(),
            bytesPerSecond: speedBytesPerSecond,
            etaSeconds,
          };
          return [fallback, ...updated].slice(0, 100);
        });
      }
    }).then((unlisten) => {
      if (cancelled) unlisten();
      else dispose = unlisten;
    });
    return () => {
      cancelled = true;
      dispose?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let dispose: (() => void) | undefined;
    let cancelled = false;
    void getCurrentWebviewWindow()
      .onDragDropEvent((event) => {
        if (
          event.payload.type === "enter" &&
          ["模组", "整合包", "资源包", "光影", "存档"].includes(activeNav)
        )
          setDragging(true);
        if (event.payload.type === "leave") setDragging(false);
        if (event.payload.type !== "drop") return;
        setDragging(false);
        const path = event.payload.paths[0];
        if (!path) return;
        setBusy(true);
        setMessage("");
        if (activeNav === "模组") {
          Promise.all(
            event.payload.paths.map(async (candidate) => ({
              path: candidate,
              inspection: await invoke<ModInspection>("inspect_mod_jar", {
                path: candidate,
              }),
            })),
          )
            .then((queue) => {
              setModQueue(queue);
              setModInspection(queue[0]?.inspection);
              setModSourcePath(queue[0]?.path ?? "");
              if (queue.length > 1)
                setMessage(`已安全预检 ${queue.length} 个模组。`);
            })
            .catch((error: unknown) =>
              setMessage(errorText(error, "拖入的模组无法通过预检。")),
            )
            .finally(() => setBusy(false));
        } else if (activeNav === "整合包") {
          invoke<ModpackInspection>("inspect_modpack", { path })
            .then((inspection) => {
              setPackInspection(inspection);
              setPackSourcePath(path);
            })
            .catch((error: unknown) =>
              setMessage(errorText(error, "拖入的整合包无法通过预检。")),
            )
            .finally(() => setBusy(false));
        } else if (["资源包", "光影"].includes(activeNav) && modInstanceId) {
          const kind = activeNav === "资源包" ? "resourcepack" : "shaderpack";
          Promise.all(
            event.payload.paths.map((sourcePath) =>
              invoke<ContentItem>("install_content_archive", {
                instanceId: modInstanceId,
                kind,
                sourcePath,
              }),
            ),
          )
            .then((items) => {
              setArchiveItems((existing) => [
                ...items.filter(
                  (item) =>
                    !existing.some((candidate) => candidate.id === item.id),
                ),
                ...existing,
              ]);
              setMessage(`已导入 ${items.length} 个${activeNav}。`);
            })
            .catch((error: unknown) =>
              setMessage(errorText(error, `${activeNav}导入失败。`)),
            )
            .finally(() => setBusy(false));
        } else if (activeNav === "存档" && modInstanceId) {
          invoke<ContentItem>("import_world", {
            instanceId: modInstanceId,
            sourcePath: path,
          })
            .then((item) => {
              setWorldItems((existing) =>
                existing.some((candidate) => candidate.id === item.id)
                  ? existing
                  : [item, ...existing],
              );
              setMessage(`存档 ${item.fileName} 已导入。`);
            })
            .catch((error: unknown) =>
              setMessage(errorText(error, "存档导入失败。")),
            )
            .finally(() => setBusy(false));
        } else {
          setBusy(false);
        }
      })
      .then((unlisten) => {
        if (cancelled) unlisten();
        else dispose = unlisten;
      });
    return () => {
      cancelled = true;
      dispose?.();
    };
  }, [activeNav, modInstanceId]);

  function selectAccount(accountId: number) {
    setSelectedAccountId(accountId);
    if (isTauri()) {
      void invoke("set_active_account", { accountId }).catch(() => {});
    }
  }

  async function createProfile() {
    const displayName = draft.trim();
    if (!/^[A-Za-z0-9_]{3,16}$/.test(displayName)) {
      setMessage("名称须为 3–16 位 ASCII 字母、数字或下划线。");
      return;
    }
    if (!isTauri()) {
      setMessage("请在桌面应用中创建档案。");
      return;
    }
    setBusy(true);
    setMessage("");
    try {
      const account = await invoke<Account>("create_offline_account", {
        displayName,
      });
      setAccounts((existing) => [account, ...existing]);
      selectAccount(account.id);
      setDraft("");
    } catch (error) {
      setMessage(errorText(error, "创建档案失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function loginMicrosoft() {
    if (!isTauri()) {
      setMessage("请在桌面应用中登录 Microsoft。.");
      return;
    }
    setBusy(true);
    setMessage("正在打开系统浏览器完成 Microsoft 登录…");
    try {
      const account = await invoke<Account>("login_microsoft", {
        clientId: settings.microsoftClientId?.trim() ?? "",
      });
      setAccounts((existing) => [
        account,
        ...existing.filter((candidate) => candidate.id !== account.id),
      ]);
      selectAccount(account.id);
      setMessage("Microsoft 账户已安全保存到 Windows 凭据存储。 ");
    } catch (error) {
      setMessage(errorText(error, "Microsoft 登录失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function removeAccount(account: Account) {
    if (!window.confirm(`确定移除账户“${account.displayName}”吗？`)) return;
    setBusy(true);
    setMessage("正在安全移除账户…");
    try {
      await invoke("remove_account", { accountId: account.id });
      const remaining = accounts.filter((candidate) => candidate.id !== account.id);
      setAccounts(remaining);
      const nextId = remaining[0]?.id;
      setSelectedAccountId(nextId);
      if (isTauri()) {
        await invoke("set_active_account", {
          accountId: nextId ?? null,
        }).catch(() => {});
      }
      setMessage("账户已移除；Microsoft 凭据也已从 Windows 凭据管理器清理。 ");
    } catch (error) {
      setMessage(errorText(error, "移除账户失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function loginExternal(
    apiRoot: string,
    username: string,
    password: string,
  ) {
    if (!isTauri()) {
      setMessage("请在桌面应用中登录外置账户。");
      return;
    }
    setBusy(true);
    setMessage("正在连接外置登录服务器并缓存登录组件…");
    try {
      const account = await invoke<Account>("login_external", {
        apiRoot,
        username,
        password,
      });
      setAccounts((existing) => [
        account,
        ...existing.filter((candidate) => candidate.id !== account.id),
      ]);
      selectAccount(account.id);
      setMessage(`外置登录成功：${account.displayName}，凭据已安全保存。`);
    } catch (error) {
      setMessage(errorText(error, "外置登录失败。"));
      throw error;
    } finally {
      setBusy(false);
    }
  }

  async function addServer(
    name: string,
    address: string,
    port: number,
    description: string,
  ) {
    setBusy(true);
    try {
      const server = await invoke<ServerEntry>("add_server", {
        name,
        address,
        port,
        description,
      });
      setServers((existing) => [server, ...existing]);
      setMessage("服务器已添加。");
    } catch (error) {
      setMessage(errorText(error, "添加服务器失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function updateServer(
    server: ServerEntry,
    name: string,
    address: string,
    port: number,
    description: string,
  ) {
    setBusy(true);
    try {
      const updated = await invoke<ServerEntry>("update_server", {
        serverId: server.id,
        name,
        address,
        port,
        description,
      });
      setServers((existing) =>
        existing.map((candidate) =>
          candidate.id === updated.id ? updated : candidate,
        ),
      );
      setMessage("服务器已更新。");
    } catch (error) {
      setMessage(errorText(error, "更新服务器失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function removeServer(server: ServerEntry) {
    if (!window.confirm(`确定删除服务器“${server.name}”吗？`)) return;
    setBusy(true);
    try {
      await invoke("remove_server", { serverId: server.id });
      setServers((existing) => existing.filter((candidate) => candidate.id !== server.id));
      setMessage("服务器已删除。");
    } catch (error) {
      setMessage(errorText(error, "删除服务器失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function saveLauncherSettings() {
    setBusy(true);
    setMessage("");
    try {
      setSettings(
        await invoke<LauncherSettings>("save_settings", { settings }),
      );
      setMessage("设置已保存到 D 盘数据库。");
    } catch (error) {
      setMessage(errorText(error, "设置保存失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function installManagedJava(major: number) {
    if (!isTauri()) return;
    setDownloading(true);
    setMessage(`正在下载并校验官方 OpenJDK ${major}…`);
    try {
      const runtime = await invoke<JavaRuntime>("install_managed_java", {
        major,
      });
      setJavaRuntimes((existing) => [
        runtime,
        ...existing.filter((candidate) => candidate.path !== runtime.path),
      ]);
      setSelectedJavaPath(runtime.path);
      setMessage(
        `Java ${runtime.version} 已安装到 D 盘受管理目录并通过 64 位运行自检。`,
      );
    } catch (error) {
      setMessage(errorText(error, "Java 安装失败。"));
    } finally {
      setDownloading(false);
    }
  }

  async function checkEnvironment() {
    if (!isTauri()) return;
    setBusy(true);
    setMessage("正在检测运行环境…");
    try {
      const detected = await invoke<JavaRuntime[]>("detect_java_runtimes");
      setJavaRuntimes(detected);
      const compatible = detected.filter((runtime) => runtime.is64Bit);
      if (compatible.length === 0) {
        setSelectedJavaPath(undefined);
        setMessage("未检测到可用的 64 位 Java。请点击“一键安装并验证 Java 21”。");
        return;
      }
      const preferred =
        compatible.find((runtime) => runtime.majorVersion === 21) ?? compatible[0];
      setSelectedJavaPath(preferred.path);
      setMessage(
        `环境检查通过：界面组件正常，并找到 ${compatible.length} 个 64 位 Java；已选择 Java ${preferred.majorVersion ?? preferred.version}。`,
      );
    } catch (error) {
      setMessage(errorText(error, "运行环境检查失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function searchOnline(projectType: "mod" | "modpack") {
    if (!isTauri()) return;
    const target = instances.find((instance) => instance.id === modInstanceId);
    if (projectType === "mod" && (!target || target.loaderType === "vanilla")) {
      setMessage("请先选择一套已经启用模组功能的游戏配置。");
      return;
    }
    setBusy(true);
    setMessage("正在同时搜索 Modrinth 与 CurseForge…");
    try {
      const query =
        projectType === "mod" ? onlineModQuery : onlinePackQuery;
      const queries = expandSearchQueries(query);
      const gameVersion =
        projectType === "mod" && target
          ? onlineModVersion.trim() || target.gameVersion
          : undefined;
      const loader =
        projectType === "mod" && target
          ? onlineModLoader || target.loaderType
          : undefined;
      const batches = await Promise.all(
        queries.map((singleQuery) =>
          Promise.allSettled([
            invoke<OnlineProject[]>("search_modrinth_projects", {
              query: singleQuery,
              projectType,
              ...(gameVersion ? { gameVersion } : {}),
              ...(loader ? { loader } : {}),
            }),
            invoke<OnlineProject[]>("search_curseforge_projects", {
              query: singleQuery,
              projectType,
              ...(gameVersion ? { gameVersion } : {}),
              ...(loader ? { loader } : {}),
            }),
          ]),
        ),
      );
      const merged: OnlineProject[] = [];
      const seen = new Set<string>();
      for (const batch of batches) {
        for (const result of batch) {
          if (result.status !== "fulfilled") continue;
          for (const project of result.value) {
            const key = `${project.source}:${project.projectId}`;
            if (seen.has(key)) continue;
            seen.add(key);
            merged.push(project);
          }
        }
      }
      merged.sort((left, right) => right.downloads - left.downloads);
      const limited = merged.slice(0, 40);
      if (projectType === "mod") setOnlineModProjects(limited);
      else setOnlinePackProjects(limited);
      setMessage(
        limited.length
          ? `找到 ${limited.length} 个兼容项目（Modrinth + CurseForge）。`
          : "没有找到兼容项目，请换个关键词或检查网络。",
      );
    } catch (error) {
      setMessage(errorText(error, "在线搜索失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function installOnlineMod(project: OnlineProject) {
    if (!isTauri() || !modInstanceId) return;
    setDownloading(true);
    setMessage(`正在下载、校验并安装 ${project.title}…`);
    try {
      const target = instances.find((instance) => instance.id === modInstanceId);
      const item =
        project.source === "curseforge"
          ? await invoke<ContentItem>("install_curseforge_project", {
              instanceId: modInstanceId,
              projectId: project.projectId,
              gameVersion: target?.gameVersion ?? "",
              loader: target?.loaderType ?? "forge",
            })
          : await invoke<ContentItem>("install_modrinth_mod", {
              instanceId: modInstanceId,
              projectId: project.projectId,
            });
      setModItems((existing) => [item, ...existing.filter((value) => value.id !== item.id)]);
      setMessage(
        `${project.title} 已从 ${project.source === "curseforge" ? "CurseForge" : "Modrinth"} 下载完成，文件完整且适合当前模组环境。`,
      );
    } catch (error) {
      setMessage(errorText(error, "在线模组安装失败。"));
    } finally {
      setDownloading(false);
    }
  }

  async function translateSearchText(
    text: string,
  ): Promise<string | undefined> {
    if (!isTauri()) return undefined;
    try {
      const translated = await invoke<string | null>("translate_search_text", {
        text,
      });
      return translated ?? undefined;
    } catch {
      return undefined;
    }
  }

  async function installOnlinePack(project: OnlineProject) {
    if (!isTauri()) return;
    setDownloading(true);
    setMessage(`正在下载、校验并导入 ${project.title}…`);
    try {
      if (project.source === "curseforge") {
        const gameVersion = project.versions[0];
        const loaderType = project.loaderType ?? "forge";
        if (!gameVersion) {
          throw new Error("CurseForge 未返回该整合包的游戏版本，无法创建独立实例。");
        }
        const instance = await invoke<Instance>("create_instance_profile", {
          name: project.title,
          gameVersion,
          loaderType,
        });
        const packPath = await invoke<string>("download_curseforge_modpack", {
          instanceId: instance.id,
          projectId: project.projectId,
          gameVersion,
          loader: loaderType,
        });
        const inspection = await invoke<ModpackInspection>("inspect_modpack", {
          path: packPath,
        });
        await invoke<ImportedLocalPack>("import_local_pack", {
          instanceId: instance.id,
          sourcePath: packPath,
        });
        const ready = await finishNewInstanceImport(instance, {
          sourcePath: packPath,
          inspection,
        });
        setMessage(
          `整合包“${ready.name}”已从 CurseForge 下载并创建为独立实例（Minecraft ${ready.gameVersion} · ${loaderLabel(ready.loaderType)}），游戏与加载器已自动安装完成。`,
        );
      } else {
        const result = await invoke<ImportedModpack>("install_modrinth_modpack", {
          projectId: project.projectId,
        });
        const ready = await finishNewInstanceImport(result.instance, {
          projectId: project.projectId,
        });
        setMessage(
          `${project.title} 已创建为独立实例“${ready.name}”（Minecraft ${ready.gameVersion} · ${loaderLabel(ready.loaderType)}），共下载 ${result.downloadedFiles} 个文件，游戏与加载器已自动安装完成。`,
        );
      }
    } catch (error) {
      setMessage(errorText(error, "在线整合包安装失败。"));
    } finally {
      setDownloading(false);
    }
  }

  async function refreshDiagnostics() {
    setBusy(true);
    setMessage("");
    try {
      const [jobs, crashes, logs] = await Promise.all([
        invoke<DownloadJob[]>("list_download_jobs"),
        invoke<CrashReport[]>("list_crash_reports"),
        invoke<GameLog[]>("list_game_logs"),
      ]);
      setDownloadJobs(jobs);
      setCrashReports(crashes);
      setGameLogs(logs);
    } catch (error) {
      setMessage(errorText(error, "无法读取诊断信息。"));
    } finally {
      setBusy(false);
    }
  }

  async function readGameLog(log: GameLog, level: string, query: string) {
    setBusy(true);
    try {
      setGameLogText(await invoke<string>("read_game_log", {
        instanceId: log.instanceId,
        fileName: log.fileName,
        level,
        query,
      }));
    } catch (error) {
      setMessage(errorText(error, "无法读取游戏日志。"));
    } finally {
      setBusy(false);
    }
  }

  async function exportDiagnostics() {
    setBusy(true);
    setMessage("");
    try {
      const path = await invoke<string>("export_diagnostic_report");
      setMessage(`脱敏诊断报告已导出：${path}`);
    } catch (error) {
      setMessage(errorText(error, "诊断报告导出失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function cancelDownloads() {
    if (!isTauri()) return;
    await invoke("cancel_active_downloads");
    setMessage("已请求取消；正在写入的分块完成后会停止，临时文件可断点续传。");
  }

  async function importArchives(kind: "resourcepack" | "shaderpack") {
    if (!modInstanceId) {
      setMessage("请先选择要使用哪套游戏配置。");
      return;
    }
    const selected = await open({
      multiple: true,
      directory: false,
      filters: [
        {
          name:
            kind === "resourcepack"
              ? "Minecraft Resource Pack"
              : "Minecraft Shader Pack",
          extensions: ["zip"],
        },
      ],
    });
    const paths = typeof selected === "string" ? [selected] : selected;
    if (!paths?.length) return;
    setBusy(true);
    setMessage("");
    try {
      const items: ContentItem[] = [];
      for (const sourcePath of paths)
        items.push(
          await invoke<ContentItem>("install_content_archive", {
            instanceId: modInstanceId,
            kind,
            sourcePath,
          }),
        );
      setArchiveItems((existing) => [
        ...items.filter(
          (item) => !existing.some((candidate) => candidate.id === item.id),
        ),
        ...existing,
      ]);
      setMessage(`已安全导入 ${items.length} 个文件。`);
    } catch (error) {
      setMessage(errorText(error, "内容导入失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function toggleArchive(item: ContentItem) {
    setBusy(true);
    setMessage("");
    try {
      const updated = await invoke<ContentItem>("set_content_enabled", {
        contentId: item.id,
        enabled: !item.enabled,
      });
      setArchiveItems((existing) =>
        existing.map((candidate) =>
          candidate.id === updated.id ? updated : candidate,
        ),
      );
    } catch (error) {
      setMessage(errorText(error, "无法更改内容状态。"));
    } finally {
      setBusy(false);
    }
  }

  async function removeArchive(item: ContentItem) {
    setBusy(true);
    setMessage("");
    try {
      const removed = await invoke<RemovedContent>("remove_content_to_backup", {
        contentId: item.id,
      });
      setArchiveItems((existing) =>
        existing.filter((candidate) => candidate.id !== item.id),
      );
      if (modInstanceId) setRemovedBackups(await invoke<BackupItem[]>("list_removed_backups", { instanceId: modInstanceId, kind: item.kind }));
      setMessage(`已移至可恢复备份：${removed.backupPath}`);
    } catch (error) {
      setMessage(errorText(error, "无法移除内容。"));
    } finally {
      setBusy(false);
    }
  }

  async function chooseAndImportWorld(directory: boolean) {
    if (!modInstanceId) {
      setMessage("请先选择要使用哪套游戏配置。");
      return;
    }
    const selected = await open(
      directory
        ? { multiple: false, directory: true }
        : {
            multiple: false,
            directory: false,
            filters: [{ name: "Minecraft World", extensions: ["zip"] }],
          },
    );
    if (typeof selected !== "string") return;
    setBusy(true);
    setMessage("");
    try {
      const item = await invoke<ContentItem>("import_world", {
        instanceId: modInstanceId,
        sourcePath: selected,
      });
      setWorldItems((existing) => [item, ...existing]);
      setMessage(`存档 ${item.fileName} 已安全导入。`);
    } catch (error) {
      setMessage(errorText(error, "存档导入失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function removeWorld(item: ContentItem) {
    setBusy(true);
    setMessage("");
    try {
      const removed = await invoke<RemovedContent>("remove_world_to_backup", {
        contentId: item.id,
      });
      setWorldItems((existing) =>
        existing.filter((candidate) => candidate.id !== item.id),
      );
      if (modInstanceId) setRemovedBackups(await invoke<BackupItem[]>("list_removed_backups", { instanceId: modInstanceId, kind: "world" }));
      setMessage(`存档已移至可恢复备份：${removed.backupPath}`);
    } catch (error) {
      setMessage(errorText(error, "无法移除存档。"));
    } finally {
      setBusy(false);
    }
  }

  async function installCurseforgeUrl(url: string) {
    if (!isTauri() || !modInstanceId) return;
    setDownloading(true);
    setMessage("正在从 CurseForge 解析并安装…");
    try {
      const item = await invoke<ContentItem>("install_curseforge_url", {
        instanceId: modInstanceId,
        url,
      });
      setModItems((existing) => [
        item,
        ...existing.filter((candidate) => candidate.id !== item.id),
      ]);
      setMessage(`已从 CurseForge 安装：${item.fileName}`);
    } catch (error) {
      setMessage(errorText(error, "从 CurseForge 安装失败。"));
    } finally {
      setDownloading(false);
    }
  }

  async function deleteWorldPermanently(item: ContentItem) {
    if (
      !window.confirm(
        `确定彻底删除存档“${item.fileName}”吗？此操作不可恢复。`,
      )
    )
      return;
    if (!window.confirm("再次确认：将永久删除该存档文件夹和记录，无法恢复。"))
      return;
    setBusy(true);
    setMessage("");
    try {
      const deleted = await invoke<{ id: number; path: string }>(
        "delete_world_permanently",
        { contentId: item.id },
      );
      setWorldItems((existing) =>
        existing.filter((candidate) => candidate.id !== item.id),
      );
      setMessage(`存档已彻底删除：${deleted.path}`);
    } catch (error) {
      setMessage(errorText(error, "无法删除存档。"));
    } finally {
      setBusy(false);
    }
  }

  async function deleteIncompatibleMods() {
    setBusy(true);
    setMessage("");
    try {
      const targets = incompatibleGroups.map((group) => ({
        instanceId: group.instanceId,
        fileNames: group.mods.map((mod) => mod.fileName),
      }));
      const removed = await invoke<number>("remove_incompatible_mods", {
        targets,
      });
      setIncompatibleGroups([]);
      setBootProblems((existing) =>
        existing.filter((problem) => !problem.text.includes("不兼容")),
      );
      if (modInstanceId) {
        setModItems(
          await invoke<ContentItem[]>("list_content_items", {
            instanceId: modInstanceId,
            kind: "mod",
          }),
        );
      }
      setMessage(
        `已删除 ${removed} 个不兼容模组（可在模组页备份区恢复）。`,
      );
    } catch (error) {
      setMessage(errorText(error, "删除不兼容模组失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function cleanLauncherCache() {
    if (
      !window.confirm(
        "确定清理下载缓存和临时文件吗？不会删除任何游戏、模组、整合包或存档；缓存会在下次需要时重新下载。",
      )
    )
      return;
    setBusy(true);
    setMessage("正在清理缓存…");
    try {
      const result = await invoke<{ removedFiles: number; freedBytes: number }>(
        "clean_launcher_cache",
      );
      setMessage(
        `缓存清理完成：移除 ${result.removedFiles} 个缓存/临时文件，释放约 ${formatBytes(result.freedBytes)}。`,
      );
    } catch (error) {
      setMessage(errorText(error, "缓存清理失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function backupWorld(item: ContentItem) {
    setBusy(true);
    setMessage("正在备份存档…");
    try {
      const result = await invoke<RemovedContent>("backup_world", { contentId: item.id });
      setMessage(`存档备份完成：${result.backupPath}`);
    } catch (error) {
      setMessage(errorText(error, "存档备份失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function duplicateWorld(item: ContentItem) {
    setBusy(true);
    setMessage("正在复制存档…");
    try {
      const duplicate = await invoke<ContentItem>("duplicate_world", { contentId: item.id });
      setWorldItems((existing) => [duplicate, ...existing]);
      setMessage(`存档副本已创建：${duplicate.fileName}`);
    } catch (error) {
      setMessage(errorText(error, "复制存档失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function exportWorld(item: ContentItem) {
    if (!isTauri()) return;
    const destination = await save({
      defaultPath: `${item.fileName}.zip`,
      filters: [{ name: "Minecraft 存档", extensions: ["zip"] }],
    });
    if (!destination) return;
    setBusy(true);
    setMessage("正在导出存档…");
    try {
      const result = await invoke<ExportResult>("export_world", { contentId: item.id, destination });
      setMessage(`存档已导出：${result.path}（${result.files} 个文件）`);
    } catch (error) {
      setMessage(errorText(error, "导出存档失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function openInstanceDirectory(instanceId: number | undefined, section: string) {
    if (!isTauri() || !instanceId) {
      setMessage("请先选择一套游戏配置。");
      return;
    }
    try {
      const path = await invoke<string>("open_instance_directory", { instanceId, section });
      setMessage(`已打开：${path}`);
    } catch (error) {
      setMessage(errorText(error, "无法打开文件夹。"));
    }
  }

  async function openInstanceForm() {
    setShowInstanceForm(true);
    setMessage("");
    if (!isTauri() || versions.length) return;
    setBusy(true);
    try {
      const manifest = await invoke<VersionManifest>("fetch_version_manifest", {
        includeSnapshots: false,
      });
      setVersions(manifest.versions);
      setGameVersion(manifest.latest.release);
    } catch (error) {
      setMessage(errorText(error, "无法读取官方版本清单。"));
    } finally {
      setBusy(false);
    }
  }

  async function createInstance() {
    if (!isTauri()) {
      setMessage("请在桌面程序中创建游戏配置。");
      return;
    }
    setBusy(true);
    setMessage("");
    try {
      const instance = await invoke<Instance>("create_instance_profile", {
        name: instanceName.trim(),
        gameVersion: gameVersion.trim(),
        loaderType: instanceLoader,
      });
      setInstances((existing) => [instance, ...existing]);
      setSelectedInstanceId(instance.id);
      setModInstanceId(instance.id);
      setInstanceName("");
      setShowInstanceForm(false);
    } catch (error) {
      setMessage(errorText(error, "创建游戏配置失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function cloneInstance(instance: Instance) {
    setBusy(true);
    setMessage("正在复制实例…");
    try {
      const copySaves = window.confirm(
        `要同时复制“${instance.name}”的存档（saves）吗？\n\n确定 = 连同存档一起复制\n取消 = 只复制游戏、模组与配置`,
      );
      const cloned = await invoke<Instance>("clone_instance", {
        instanceId: instance.id,
        name: `${instance.name} 副本`,
        copySaves,
      });
      setInstances((existing) => [cloned, ...existing]);
      setMessage(`实例已复制：${cloned.name}`);
    } catch (error) {
      setMessage(errorText(error, "复制实例失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function renameInstance(instance: Instance) {
    const name = window.prompt("新的实例名称", instance.name)?.trim();
    if (!name || name === instance.name) return;
    setBusy(true);
    setMessage("正在重命名实例…");
    try {
      const updated = await invoke<Instance>("rename_instance", { instanceId: instance.id, name });
      setInstances((existing) => existing.map((candidate) => candidate.id === updated.id ? updated : candidate));
      setMessage(`实例已重命名为 ${updated.name}`);
    } catch (error) {
      setMessage(errorText(error, "重命名实例失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function updateInstanceMemory(instance: Instance, memoryMb: number) {
    setBusy(true);
    setMessage("");
    try {
      const updated = await invoke<Instance>("update_instance_memory", {
        instanceId: instance.id,
        memoryMb,
      });
      setInstances((existing) =>
        existing.map((item) => (item.id === updated.id ? updated : item)),
      );
      setMessage(`实例“${updated.name}”内存已改为 ${updated.memoryMb} MB。`);
    } catch (error) {
      setMessage(errorText(error, "修改实例内存失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function deleteInstance(instance: Instance) {
    if (!window.confirm(`确定移除实例“${instance.name}”吗？实例会先进入可恢复备份。`)) return;
    if (!window.confirm("请再次确认：存档、模组和配置都会从实例列表移除。")) return;
    setBusy(true);
    setMessage("正在安全移除实例…");
    try {
      const removed = await invoke<RemovedContent>("delete_instance_to_backup", { instanceId: instance.id });
      setInstances((existing) => existing.filter((candidate) => candidate.id !== instance.id));
      setSelectedInstanceId((selected) => selected === instance.id ? undefined : selected);
      setMessage(`实例已移至备份：${removed.backupPath}`);
    } catch (error) {
      setMessage(errorText(error, "移除实例失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function repairInstance(instance: Instance) {
    setBusy(true);
    setMessage("正在校验并修复实例文件…");
    try {
      let repaired = await installClientFiles(instance);
      if (repaired.loaderType !== "vanilla") {
        repaired = await installInstanceLoaderFiles(repaired);
      }
      setMessage(`实例 ${repaired.name} 已完成校验和修复。`);
    } catch (error) {
      setMessage(errorText(error, "实例修复失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function installClientFiles(instance: Instance): Promise<Instance> {
    setDownloadProgress((existing) => ({ ...existing, [instance.id]: 0 }));
    let available = versions;
    if (!available.length) {
      const manifest = await invoke<VersionManifest>(
        "fetch_version_manifest",
        { includeSnapshots: true },
      );
      available = manifest.versions;
      setVersions(available);
    }
    const version = available.find(
      (candidate) => candidate.id === instance.gameVersion,
    );
    if (!version) throw new Error("官方版本列表中没有找到这套游戏所选的版本。");
    await invoke("install_vanilla_client", {
      instanceId: instance.id,
      versionUrl: version.url,
      versionSha1: version.sha1,
    });
    const updated: Instance = {
      ...instance,
      status: instance.loaderType === "vanilla" ? "ready" : "loader_missing",
    };
    setClientReady((existing) => ({ ...existing, [instance.id]: true }));
    setInstances((existing) =>
      existing.map((candidate) =>
        candidate.id === instance.id ? updated : candidate,
      ),
    );
    return updated;
  }

  async function ensureJavaForGame(
    gameVersion: string,
  ): Promise<JavaRuntime | undefined> {
    const required = javaMajorForGameVersion(gameVersion);
    if (!required) return undefined;
    const installed = javaRuntimes.find(
      (runtime) => runtime.is64Bit && runtime.majorVersion === required,
    );
    if (installed) {
      if (!selectedJavaPath) setSelectedJavaPath(installed.path);
      return installed;
    }
    setMessage(`正在自动下载并安装此游戏版本需要的 Java ${required}…`);
    const runtime = await invoke<JavaRuntime>("install_managed_java", {
      major: required,
    });
    setJavaRuntimes((existing) =>
      existing.some((item) => item.path === runtime.path)
        ? existing
        : [...existing, runtime],
    );
    setSelectedJavaPath(runtime.path);
    return runtime;
  }

  async function recordModpackArchive(input: {
    sourceKind: "local" | "modrinth";
    filePath?: string | null;
    projectId?: string | null;
    fileName: string;
    name?: string | null;
    version?: string | null;
    gameVersion?: string | null;
    loaderType?: string | null;
    format: string;
    instanceId?: number | null;
  }) {
    try {
      const archive = await invoke<ModpackArchive>("record_modpack_archive", {
        sourceKind: input.sourceKind,
        filePath: input.filePath ?? null,
        projectId: input.projectId ?? null,
        fileName: input.fileName,
        name: input.name ?? null,
        version: input.version ?? null,
        gameVersion: input.gameVersion ?? null,
        loaderType: input.loaderType ?? null,
        format: input.format,
        sizeBytes: null,
        instanceId: input.instanceId ?? null,
      });
      setModpackArchives((existing) => [
        archive,
        ...existing.filter((candidate) => candidate.id !== archive.id),
      ]);
    } catch {
      // 记录失败不影响已导入的实例
    }
  }

  async function finishNewInstanceImport(
    instance: Instance,
    options: {
      sourcePath?: string | null;
      inspection?: ModpackInspection | null;
      projectId?: string | null;
    } = {},
  ): Promise<Instance> {
    setInstances((existing) => [
      instance,
      ...existing.filter((candidate) => candidate.id !== instance.id),
    ]);
    setSelectedInstanceId(instance.id);
    setModInstanceId(instance.id);
    const java = await ensureJavaForGame(instance.gameVersion);
    let ready = instance;
    if (["missing", "base_missing"].includes(ready.status)) {
      setMessage("正在自动下载并校验游戏文件…");
      ready = await installClientFiles(ready);
    }
    if (ready.loaderType !== "vanilla" && ready.status !== "ready") {
      setMessage(
        `正在自动安装兼容的 ${loaderLabel(ready.loaderType)} 模组环境…`,
      );
      ready = await installInstanceLoaderFiles(ready, java?.path);
    }
    setInstances((existing) =>
      existing.map((candidate) =>
        candidate.id === ready.id ? ready : candidate,
      ),
    );
    await recordModpackArchive({
      sourceKind: options.projectId ? "modrinth" : "local",
      filePath: options.sourcePath ?? null,
      projectId: options.projectId ?? null,
      fileName: options.inspection?.fileName ?? ready.name,
      name: options.inspection?.name ?? (options.projectId ? ready.name : null),
      version: options.inspection?.version ?? null,
      gameVersion: ready.gameVersion,
      loaderType: ready.loaderType,
      format:
        options.inspection?.format ?? (options.projectId ? "modrinth" : "zip"),
      instanceId: ready.id,
    });
    return ready;
  }

  async function installClient(instance: Instance) {
    if (!isTauri()) return;
    setBusy(true);
    setMessage("正在查找电脑里已有的游戏文件，并检查缺少的部分…");
    try {
      await installClientFiles(instance);
      setMessage("游戏文件已经检查完成，可以继续启动。");
    } catch (error) {
      setDownloadProgress((existing) => {
        const next = { ...existing };
        delete next[instance.id];
        return next;
      });
      setMessage(errorText(error, "游戏安装失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function terminateRunningGame() {
    if (!selectedInstance) return;
    if (!window.confirm("游戏窗口没有响应时才需要强制结束。确定继续吗？")) return;
    setMessage("正在结束没有响应的游戏…");
    try {
      await invoke("terminate_game", { instanceId: selectedInstance.id });
      setMessage("已请求系统结束游戏进程。存档中的未保存进度可能丢失。");
    } catch (error) {
      setMessage(errorText(error, "无法结束游戏。"));
    }
  }

  async function chooseExistingGameDirectory() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected !== "string") return;
    setSettings((currentSettings) => ({
      ...currentSettings,
      gameDirectory: selected,
    }));
    setMessage("已选择已有游戏目录，保存设置后安装时会优先复用其中的文件。");
  }

  async function installInstanceLoaderFiles(
    instance: Instance,
    javaPathOverride?: string,
  ): Promise<Instance> {
    let available = loaderVersions[instance.id];
    if (!available?.length) {
      available = await invoke<string[]>("list_loader_versions", {
        loaderType: instance.loaderType,
        gameVersion: instance.gameVersion,
      });
      if (!available.length) {
        throw new Error("没有找到与当前游戏版本兼容的模组运行环境。");
      }
      setLoaderVersions((existing) => ({
        ...existing,
        [instance.id]: available,
      }));
    }
    const loaderVersion = loaderSelections[instance.id] ?? available[0];
    setLoaderSelections((existing) => ({
      ...existing,
      [instance.id]: loaderVersion,
    }));
    const java = javaPathOverride
      ? ({ path: javaPathOverride } as JavaRuntime)
      : selectedJava;
    if (["forge", "neoforge"].includes(instance.loaderType) && !java) {
      throw new Error("安装 Forge/NeoForge 前需要可用的 64 位 Java。");
    }
    const updated = await invoke<Instance>(
      ["forge", "neoforge"].includes(instance.loaderType)
        ? "install_java_loader"
        : "install_profile_loader",
      {
        instanceId: instance.id,
        loaderVersion,
        ...(java ? { javaPath: java.path } : {}),
      },
    );
    setInstances((existing) =>
      existing.map((candidate) =>
        candidate.id === updated.id ? updated : candidate,
      ),
    );
    return updated;
  }

  async function launchSelectedInstance(
    targetInstance?: Instance,
    force = false,
    server?: ServerEntry,
    accountId?: number,
  ) {
    const requestedInstance = targetInstance ?? selectedInstance;
    if (!requestedInstance) {
      setMessage("还没有游戏配置，请先新建一套游戏配置。");
      return;
    }
    setBusy(true);
    setMessage("正在准备游戏…");
    try {
      let launchAccount =
        accountId ? accounts.find((account) => account.id === accountId) ?? current : current;
      if (!launchAccount) {
        setMessage("正在创建本机测试档案…");
        launchAccount = await invoke<Account>("create_offline_account", {
          displayName: "Player",
        });
        setAccounts((existing) => [
          launchAccount as Account,
          ...existing.filter((account) => account.id !== launchAccount?.id),
        ]);
      }
      let readyInstance = requestedInstance;
      if (["missing", "base_missing"].includes(readyInstance.status)) {
        setMessage("正在自动检查并补齐游戏文件…");
        readyInstance = await installClientFiles(readyInstance);
      }
      if (
        readyInstance.loaderType !== "vanilla" &&
        readyInstance.status !== "ready"
      ) {
        setMessage(
          `正在自动安装兼容的 ${loaderLabel(readyInstance.loaderType)} 模组环境…`,
        );
        readyInstance = await installInstanceLoaderFiles(readyInstance);
      }
      if (readyInstance.status !== "ready") {
        throw new Error("这套游戏配置还没有准备完成。");
      }
      const java = await ensureJavaForGame(readyInstance.gameVersion);
      const javaPath = java?.path ?? selectedJava?.path;
      if (!javaPath) {
        setMessage("没有找到可用的 64 位 Java，请先到设置里点击“一键检查并安装”。");
        return;
      }
      setMessage("文件和 Java 已就绪，正在启动 Minecraft…");
      const result = await invoke<{ processId: number; logPath: string }>(
        "launch_instance",
        {
          instanceId: readyInstance.id,
          accountId: launchAccount.id,
          javaPath,
          force,
          serverAddress: server?.address ?? null,
          serverPort: server?.port ?? null,
          serverId: server?.id ?? null,
        },
      );
      setMessage(
        `游戏进程已启动（PID ${result.processId}），日志：${result.logPath}`,
      );
      if (server) {
        try {
          setServers(await invoke<ServerEntry[]>("list_servers"));
        } catch {
          // 刷新失败不影响游戏
        }
      }
      if (settings.closeLauncherAfterGameStart) {
        await invoke("exit_launcher");
      }
    } catch (error) {
      const text = errorText(error, "游戏启动失败。");
      setMessage(text);
      const isModIssue = /模组|前置|加载器|依赖/i.test(text);
      setErrorModal({
        title: "启动前检查未通过",
        lines: text
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter(Boolean),
        ...(isModIssue
          ? {
              actionLabel: "自动补齐前置",
              action: () => {
                setErrorModal(null);
                void repairDependencies(requestedInstance);
              },
              secondaryLabel: "仍要启动",
              onSecondary: () => {
                setErrorModal(null);
                void launchSelectedInstance(requestedInstance, true);
              },
            }
          : {}),
      });
    } finally {
      setBusy(false);
    }
  }

  async function repairDependencies(instance: Instance) {
    setBusy(true);
    setMessage("正在自动补齐缺失的前置模组…");
    try {
      await invoke<string>("repair_missing_mod_dependencies", {
        instanceId: instance.id,
      });
      setMessage("前置模组已补齐，请再次点击“开始游戏”。");
    } catch (error) {
      setMessage(errorText(error, "自动补齐失败，可稍后重试或仍要启动。"));
    } finally {
      setBusy(false);
    }
  }

  async function installInstanceLoader(instance: Instance) {
    setBusy(true);
    setMessage(`正在自动选择并安装 ${loaderLabel(instance.loaderType)}…`);
    try {
      const updated = await installInstanceLoaderFiles(instance);
      setMessage(`${loaderLabel(updated.loaderType)} 模组环境已安装并校验。`);
    } catch (error) {
      setMessage(errorText(error, "模组运行环境安装失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function inspectMod() {
    if (!isTauri()) {
      setMessage("请在桌面应用中选择模组文件。");
      return;
    }
    const selected = await open({
      multiple: true,
      directory: false,
      filters: [{ name: "Minecraft Mod", extensions: ["jar"] }],
    });
    const paths = typeof selected === "string" ? [selected] : selected;
    if (!paths?.length) return;
    setBusy(true);
    setMessage("");
    try {
      const queue = await Promise.all(
        paths.map(async (path) => ({
          path,
          inspection: await invoke<ModInspection>("inspect_mod_jar", { path }),
        })),
      );
      setModQueue(queue);
      setModInspection(queue[0]?.inspection);
      setModSourcePath(queue[0]?.path ?? "");
      if (queue.length > 1) setMessage(`已安全预检 ${queue.length} 个模组。`);
    } catch (error) {
      setModInspection(undefined);
      setModSourcePath("");
      setModQueue([]);
      setMessage(errorText(error, "模组预检失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function inspectPack() {
    if (!isTauri()) {
      setMessage("请在桌面应用中选择整合包。");
      return;
    }
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Minecraft Modpack", extensions: ["mrpack", "zip"] }],
    });
    if (typeof selected !== "string") return;
    setBusy(true);
    setMessage("");
    try {
      setPackInspection(
        await invoke<ModpackInspection>("inspect_modpack", { path: selected }),
      );
      setPackSourcePath(selected);
    } catch (error) {
      setPackInspection(undefined);
      setPackSourcePath("");
      setMessage(errorText(error, "整合包预检失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function importPack(gameVersion?: string, loaderType?: string) {
    if (!packSourcePath || !packInspection) return;
    setDownloading(true);
    setMessage("");
    try {
      const ready = await importPackAsNewInstance(
        packSourcePath,
        packInspection,
        null,
        gameVersion,
        loaderType,
      );
      setMessage(
        ready
          ? `整合包已创建为独立实例“${ready.name}”，游戏版本 ${ready.gameVersion}、Java 与 ${loaderLabel(ready.loaderType)} 环境已自动配好，可以直接开始游戏。`
          : "整合包内容已导入到所选游戏配置。",
      );
    } catch (error) {
      setMessage(
        errorText(error, "整合包导入失败；已经下载的内容仍保留在单独目录中，可以稍后继续。"),
      );
    } finally {
      setDownloading(false);
    }
  }

  async function importPackAsNewInstance(
    sourcePath: string,
    inspection: ModpackInspection,
    projectId: string | null,
    genericGameVersion?: string,
    genericLoaderType?: string,
  ): Promise<Instance | null> {
    if (["modrinth", "mmc", "hmcl", "mcbbs"].includes(inspection.format)) {
      const command =
        inspection.format === "modrinth"
          ? "import_modrinth_pack"
          : inspection.format === "mmc"
            ? "import_mmc_pack"
            : "import_override_pack";
      const imported = await invoke<ImportedModpack>(command, { sourcePath });
      return await finishNewInstanceImport(imported.instance, {
        sourcePath,
        inspection,
        projectId,
      });
    }
    if (inspection.format === "curseforge") {
      const gameVersion = inspection.gameVersion;
      const loaderType = inspection.loaderType;
      if (!gameVersion || !loaderType) {
        throw new Error(
          "这个 CurseForge 整合包未声明游戏版本或加载器，无法自动创建独立实例；请手动选择现有实例导入。",
        );
      }
      const instance = await invoke<Instance>("create_instance_profile", {
        name:
          inspection.name ??
          inspection.fileName.replace(/\.(zip|mrpack)$/i, ""),
        gameVersion,
        loaderType,
      });
      const imported = await invoke<ImportedLocalPack>("import_local_pack", {
        instanceId: instance.id,
        sourcePath,
      });
      if (imported.unresolvedRemoteFiles) {
        setMessage(
          `${imported.unresolvedRemoteFiles} 个模组下载失败（网络或已下架），导入继续；可在“下载”页查看原因。`,
        );
      }
      return await finishNewInstanceImport(instance, {
        sourcePath,
        inspection,
        projectId,
      });
    }
    if (inspection.format === "generic") {
      if (!genericGameVersion || !genericLoaderType) {
        throw new Error("通用整合包需要先填写游戏版本并选择加载器。");
      }
      const instance = await invoke<Instance>("create_instance_profile", {
        name:
          inspection.name ??
          inspection.fileName.replace(/\.(zip|mrpack)$/i, ""),
        gameVersion: genericGameVersion,
        loaderType: genericLoaderType,
      });
      const imported = await invoke<ImportedLocalPack>("import_local_pack", {
        instanceId: instance.id,
        sourcePath,
      });
      const notes: string[] = [];
      if (imported.skippedMods.length) {
        notes.push(
          `${imported.skippedMods.length} 个模组因不兼容被跳过：${imported.skippedMods
            .slice(0, 3)
            .join("；")}`,
        );
      }
      if (imported.unresolvedRemoteFiles) {
        notes.push(
          `${imported.unresolvedRemoteFiles} 个模组下载失败（网络或已下架），可在“下载”页查看原因`,
        );
      }
      if (imported.downloadedRemoteFiles) {
        notes.unshift(
          `已从 CurseForge 自动补齐 ${imported.downloadedRemoteFiles} 个模组`,
        );
      }
      if (imported.unresolvedRemoteFiles) {
        notes.unshift(
          `${imported.unresolvedRemoteFiles} 个模组下载失败（网络或已下架），导入继续；可在“下载”页查看原因。`,
        );
      }
      const ready = await finishNewInstanceImport(instance, {
        sourcePath,
        inspection,
      });
      setMessage(
        `已导入 ${imported.importedFiles} 个本地文件，其中 ${imported.importedMods} 个模组。${
          notes.length ? notes.join("；") + "。" : ""
        }实例“${ready.name}”已自动配好游戏与加载器。`,
      );
      return ready;
    }
    throw new Error("不支持的整合包格式。");
  }

  async function importArchiveAsNewInstance(archive: ModpackArchive) {
    setDownloading(true);
    setMessage("");
    try {
      if (archive.projectId) {
        const imported = await invoke<ImportedModpack>(
          "install_modrinth_modpack",
          { projectId: archive.projectId },
        );
        const ready = await finishNewInstanceImport(imported.instance, {
          projectId: archive.projectId,
        });
        setMessage(
          `整合包已重新导入为独立实例“${ready.name}”，游戏与加载器已自动配好。`,
        );
        return;
      }
      if (!archive.filePath) {
        throw new Error("这条记录没有可用的整合包文件。");
      }
      const inspection = await invoke<ModpackInspection>("inspect_modpack", {
        path: archive.filePath,
      });
      const ready = await importPackAsNewInstance(
        archive.filePath,
        inspection,
        null,
      );
      if (ready) {
        setMessage(
          `整合包已导入为独立实例“${ready.name}”，游戏版本 ${ready.gameVersion}、Java 与加载器已自动配好。`,
        );
      }
    } catch (error) {
      setMessage(errorText(error, "从整合包库导入失败。"));
    } finally {
      setDownloading(false);
    }
  }

  async function removeModpackArchive(archive: ModpackArchive) {
    if (
      !window.confirm(
        `确定从整合包库移除“${archive.name ?? archive.fileName}”吗？不会影响已经创建的实例。`,
      )
    ) {
      return;
    }
    try {
      await invoke("remove_modpack_archive", { archiveId: archive.id });
      setModpackArchives((existing) =>
        existing.filter((candidate) => candidate.id !== archive.id),
      );
      setMessage("已从整合包库移除。");
    } catch (error) {
      setMessage(errorText(error, "移除失败。"));
    }
  }

  async function exportPack(instanceId: number, includeSaves: boolean) {
    if (!isTauri()) return;
    const instance = instances.find((candidate) => candidate.id === instanceId);
    if (!instance) {
      setMessage("请选择要导出的游戏配置。");
      return;
    }
    const destination = await save({
      defaultPath: `${instance.name}-SH整合包.zip`,
      filters: [{ name: "ZIP 整合包", extensions: ["zip"] }],
    });
    if (!destination) return;
    setBusy(true);
    setMessage("正在打包实例；账户和登录凭据不会写入整合包…");
    try {
      const result = await invoke<ExportResult>("export_instance_modpack", {
        instanceId,
        destination,
        includeSaves,
      });
      setMessage(`整合包已导出：${result.path}（${result.files} 个文件）`);
    } catch (error) {
      setMessage(errorText(error, "整合包导出失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function installMod() {
    if (!modInstanceId || (!modSourcePath && !modQueue.length)) return;
    const target = instances.find((instance) => instance.id === modInstanceId);
    if (
      !target ||
      modQueue.some(
        (candidate) =>
          candidate.inspection.loaderType !== target.loaderType ||
          !inspectionSupportsGame(
            candidate.inspection.gameVersionRequirements,
            target.gameVersion,
          ),
      )
    ) {
      setMessage("所选模组中有内容不适合当前游戏版本或模组运行环境，因此没有安装。 ");
      return;
    }
    setBusy(true);
    setMessage("");
    try {
      const queue = modQueue.length
        ? modQueue
        : [{ path: modSourcePath, inspection: modInspection! }];
      const provided = new Set([
        "minecraft",
        "java",
        "fabricloader",
        "fabric-loader",
        "quilt_loader",
        "quilt-loader",
        "forge",
        "neoforge",
      ]);
      const installedIds = new Set(
        modItems.flatMap((item) => {
          try {
            const metadata = JSON.parse(item.metadataJson ?? "{}") as {
              modId?: string;
            };
            return metadata.modId ? [metadata.modId.toLowerCase()] : [];
          } catch {
            return [];
          }
        }),
      );
      const incomingIds = new Set(
        queue
          .map((candidate) => candidate.inspection.modId?.toLowerCase())
          .filter((value): value is string => Boolean(value)),
      );
      const missing = new Set(
        queue
          .flatMap((candidate) => candidate.inspection.dependencies)
          .map((dependency) => dependency.toLowerCase())
          .filter(
            (dependency) =>
              !provided.has(dependency) &&
              !installedIds.has(dependency) &&
              !incomingIds.has(dependency),
          ),
      );
      if (missing.size) {
        setMessage(
          `没有安装：还缺少这些前置模组：${[...missing].join("、")}。请把前置一起拖进来，或先从在线搜索安装。`,
        );
        return;
      }
      const installed: ContentItem[] = [];
      for (const candidate of queue)
        installed.push(
          await invoke<ContentItem>("install_mod", {
            instanceId: modInstanceId,
            sourcePath: candidate.path,
          }),
        );
      setModItems((existing) => [
        ...installed.filter(
          (item) => !existing.some((candidate) => candidate.id === item.id),
        ),
        ...existing,
      ]);
      setMessage(`已校验并安装 ${installed.length} 个模组。`);
    } catch (error) {
      setMessage(errorText(error, "模组安装失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function toggleMod(item: ContentItem) {
    setBusy(true);
    setMessage("");
    try {
      const updated = await invoke<ContentItem>("set_mod_enabled", {
        contentId: item.id,
        enabled: !item.enabled,
      });
      setModItems((existing) =>
        existing.map((candidate) =>
          candidate.id === updated.id ? updated : candidate,
        ),
      );
      setMessage(
        updated.enabled ? "模组已启用。" : "模组已停用并移出加载目录。",
      );
    } catch (error) {
      setMessage(errorText(error, "无法更改模组状态。"));
    } finally {
      setBusy(false);
    }
  }

  async function removeMod(item: ContentItem) {
    setBusy(true);
    setMessage("");
    try {
      const removed = await invoke<RemovedContent>("remove_mod_to_backup", {
        contentId: item.id,
      });
      setModItems((existing) =>
        existing.filter((candidate) => candidate.id !== item.id),
      );
      if (modInstanceId) setRemovedBackups(await invoke<BackupItem[]>("list_removed_backups", { instanceId: modInstanceId, kind: "mod" }));
      setMessage(`模组已移至可恢复备份：${removed.backupPath}`);
    } catch (error) {
      setMessage(errorText(error, "无法移除模组。"));
    } finally {
      setBusy(false);
    }
  }

  async function restoreBackup(item: BackupItem) {
    if (!modInstanceId) return;
    setBusy(true);
    setMessage(`正在恢复 ${item.originalName}…`);
    try {
      const restored = await invoke<ContentItem>("restore_removed_backup", {
        instanceId: modInstanceId,
        kind: item.kind,
        backupName: item.backupName,
      });
      setRemovedBackups((existing) => existing.filter((candidate) => candidate.backupName !== item.backupName));
      if (item.kind === "mod") setModItems((existing) => [restored, ...existing]);
      else if (item.kind === "world") setWorldItems((existing) => [restored, ...existing]);
      else setArchiveItems((existing) => [restored, ...existing]);
      setMessage(`${item.originalName} 已恢复。`);
    } catch (error) {
      setMessage(errorText(error, "恢复备份失败，备份文件仍然保留。"));
    } finally {
      setBusy(false);
    }
  }

  async function checkModUpdates() {
    if (!isTauri() || !modInstanceId) return;
    setBusy(true);
    setMessage("正在检查与当前游戏版本兼容的模组更新…");
    try {
      const updates = await invoke<ModUpdateInfo[]>("check_mod_updates", {
        instanceId: modInstanceId,
      });
      setModUpdates(updates);
      const count = updates.filter((item) => item.updateAvailable).length;
      setMessage(count ? `找到 ${count} 个兼容更新。` : "已是当前实例可用的最新版本。");
    } catch (error) {
      setMessage(errorText(error, "检查模组更新失败。"));
    } finally {
      setBusy(false);
    }
  }

  async function updateMod(item: ContentItem) {
    setDownloading(true);
    setMessage(`正在安全更新 ${item.fileName}…`);
    try {
      const updated = await invoke<ContentItem>("update_modrinth_mod", {
        contentId: item.id,
      });
      setModItems((existing) => existing.map((candidate) => candidate.id === updated.id ? updated : candidate));
      setModUpdates((existing) => existing.map((candidate) => candidate.contentId === updated.id
        ? { ...candidate, installedVersion: candidate.latestVersion, updateAvailable: false }
        : candidate));
      setMessage("模组已更新，旧文件已放入可恢复备份。 ");
    } catch (error) {
      setMessage(errorText(error, "模组更新失败，原文件没有被覆盖。"));
    } finally {
      setDownloading(false);
    }
  }

  async function updateAllMods() {
    const pending = modUpdates.filter((item) => item.updateAvailable);
    if (!pending.length) return;
    setDownloading(true);
    setMessage(`正在更新 ${pending.length} 个模组…`);
    try {
      const updatedItems: ContentItem[] = [];
      for (const update of pending) {
        updatedItems.push(await invoke<ContentItem>("update_modrinth_mod", { contentId: update.contentId }));
      }
      setModItems((existing) => existing.map((candidate) => updatedItems.find((item) => item.id === candidate.id) ?? candidate));
      setModUpdates((existing) => existing.map((candidate) => pending.some((item) => item.contentId === candidate.contentId)
        ? { ...candidate, installedVersion: candidate.latestVersion, updateAvailable: false }
        : candidate));
      setMessage(`已更新 ${updatedItems.length} 个模组，旧文件均已备份。`);
    } catch (error) {
      setMessage(errorText(error, "批量更新中断；已完成的更新保留，未完成的原文件未变。"));
      if (modInstanceId) {
        const refreshed = await invoke<ContentItem[]>("list_content_items", { instanceId: modInstanceId, kind: "mod" });
        setModItems(refreshed);
      }
    } finally {
      setDownloading(false);
    }
  }

  const profileName = current?.displayName ?? "尚未创建档案";
  const selectedInstance =
    instances.find((instance) => instance.id === selectedInstanceId) ??
    instances[0];
  const selectedJava =
    javaRuntimes.find((runtime) => runtime.path === selectedJavaPath) ??
    javaRuntimes.find((runtime) => runtime.is64Bit);
  const navIcons = [House, LibraryBig, Compass, Download, CircleUserRound, Settings];
  const isDiscoverActive =
    DISCOVER_TABS.includes(activeNav as (typeof DISCOVER_TABS)[number]);
  if (isSplash) {
    return <SplashView />;
  }
  return (
    <div className="app-frame ui3-shell">
      <DesktopTitleBar />
      <main className="shell">
      <aside>
        <div className="brand">
          <img className="brand-art" src={grassBlock} alt="Minecraft grass block" />
          <span className="brand-copy">
            <strong>SH启动器</strong>
            <small>v{APP_VERSION} · {RELEASE_CHANNEL_LABEL}</small>
          </span>
        </div>
        <button
          className="changelog-sidebar-button"
          type="button"
          onClick={() => setShowChangelog(true)}
        >
          更新日志
        </button>
        <button
          className="changelog-sidebar-button"
          type="button"
          onClick={() => setShowTutorial(true)}
        >
          使用教程
        </button>
        <nav>
          {navItems.map((item, index) => {
            const Icon = navIcons[index] ?? Gamepad2;
            const active =
              item === "发现" ? isDiscoverActive : item === activeNav;
            return (
            <button
              className={active ? "active" : ""}
              onClick={() => {
                if (item === "发现") {
                  setDiscoverTab("模组");
                  setActiveNav("模组");
                } else {
                  setActiveNav(item);
                }
                setMessage("");
              }}
              key={item}
            >
              <Icon size={20} strokeWidth={1.8} />
              <span>{item}</span>
            </button>
            );
          })}
        </nav>
        <section className="account">
          <div className="avatar">
            {current ? profileName[0].toUpperCase() : <CircleUserRound size={22} />}
          </div>
          <div>
            <strong>{profileName}</strong>
            <small>
              {current
                ? current.accountType === "MICROSOFT"
                  ? "Microsoft 正版账户"
                  : current.accountType === "EXTERNAL"
                    ? "外置登录账户"
                    : "本地离线账户"
                : "需要设置"}
            </small>
          </div>
          {accounts.length > 1 ? (
            <select className="account-switcher" aria-label="切换账户" value={current?.id ?? ""} onChange={(event) => selectAccount(Number(event.target.value))}>
              {accounts.map((account) => <option key={account.id} value={account.id}>{account.displayName} · {account.accountType === "MICROSOFT" ? "正版" : account.accountType === "EXTERNAL" ? "外置" : "离线"}</option>)}
            </select>
          ) : null}
        </section>
      </aside>
      <section className="content">
        {isDiscoverActive ? (
          <div className="ui3-discover-tabs" role="tablist" aria-label="发现分类">
            {DISCOVER_TABS.map((tab) => (
              <button
                key={tab}
                type="button"
                role="tab"
                aria-selected={activeNav === tab}
                className={activeNav === tab ? "active" : ""}
                onClick={() => {
                  setDiscoverTab(tab);
                  setActiveNav(tab);
                }}
              >
                {tab}
              </button>
            ))}
          </div>
        ) : null}
        {activeNav === "主页" ? (
          <div className="ui3-home">
            <header>
              <div>
                <h1>
                  {current ? `你好，${profileName}` : "欢迎使用 SH启动器"}
                </h1>
                <p>本地数据仅保存在此设备上。</p>
              </div>
              <div className="header-actions">
                <button
                  className="quiet"
                  type="button"
                  onClick={() => setShowOnboarding(true)}
                >
                  开始游戏引导
                </button>
                <button
                  className="quiet"
                  type="button"
                  onClick={() => setShowTutorial(true)}
                >
                  使用教程
                </button>
              </div>
            </header>
            <section className="distribution-note" role="note">
              <strong>启动器不包含 Minecraft 游戏本体</strong>
              <span>
                创建游戏配置并确认后，启动器才会从 Mojang 官方网站下载游戏文件，
                并检查文件是否完整；请使用你合法取得的游戏许可。
              </span>
            </section>
            <HomeUpdateCard
              update={bootUpdate}
              checking={updateChecking}
              checkError={updateCheckError}
              onRetry={() => void runUpdateCheck()}
            />
            {bootProblems.length ? (
              <section className="boot-problems-card" role="alert">
                <div className="boot-problems-head">
                  <CircleAlert size={17} />
                  <strong>启动前发现问题</strong>
                </div>
                <ul>
                  {bootProblems.slice(0, 8).map((problem, index) => (
                    <li key={index} data-severity={problem.severity}>
                      {problem.instanceName ? (
                        <b>{problem.instanceName}：</b>
                      ) : null}
                      {problem.text}
                    </li>
                  ))}
                </ul>
                <p>建议先处理以上问题再开始游戏，避免启动失败。</p>
              </section>
            ) : null}
            <div className="layout">
              <section className="hero">
                <div className="instance-icon"><img src={grassBlock} alt="" /></div>
                <div className="hero-copy">
                  <p className="eyebrow">当前游戏配置</p>
                  <h2>{selectedInstance?.name ?? "尚未安装游戏"}</h2>
                  {instances.length > 1 ? (
                    <select
                      aria-label="当前游戏配置"
                      value={selectedInstance?.id ?? ""}
                      onChange={(event) =>
                        setSelectedInstanceId(Number(event.target.value))
                      }
                    >
                      {instances.map((instance) => (
                        <option key={instance.id} value={instance.id}>
                          {instance.name} · {loaderLabel(instance.loaderType)}
                        </option>
                      ))}
                    </select>
                  ) : null}
                  <div className="facts">
                    <span>
                      {selectedInstance
                        ? `${loaderLabel(selectedInstance.loaderType)} ${selectedInstance.gameVersion}`
                        : "选择 Minecraft 版本后开始安装"}
                    </span>
                    <span>
                      {selectedJava
                        ? `Java ${selectedJava.majorVersion ?? selectedJava.version} · 64 位`
                        : "未检测到兼容的 64 位 Java"}
                    </span>
                  </div>
                  <div className="hero-status-list">
                    <div><ShieldCheck size={18} /><span>完整性</span><strong>{selectedInstance?.status === "ready" ? "已验证" : "等待安装"}</strong><CheckCircle2 size={17} /></div>
                    <div><Coffee size={18} /><span>Java</span><strong>{selectedJava ? `Java ${selectedJava.majorVersion ?? selectedJava.version}` : "未检测"}</strong><CheckCircle2 size={17} /></div>
                    <div><FolderOpen size={18} /><span>游戏目录</span><strong>.minecraft</strong></div>
                  </div>
                </div>
                <div className="hero-actions">
                  <button
                    className="play"
                    disabled={busy || gameRunning}
                    onClick={() => void launchSelectedInstance()}
                  >
                    <Play size={20} fill="currentColor" />{gameRunning ? "游戏运行中" : "开始游戏"}
                  </button>
                  {gameRunning ? (
                    <button className="force-stop" onClick={() => void terminateRunningGame()}>
                      强制结束游戏
                    </button>
                  ) : null}
                </div>
                <p className="notice">
                  本软件仅是启动器，不捆绑游戏；已支持服务器列表、外置登录与快速加入。
                </p>
              </section>
              <aside className="activity">
                <section className="download-card">
                  <div className="side-card-title"><h3>下载任务</h3><span>⌃</span></div>
                  <strong className="download-state">{downloadJobs.some((job) => job.status === "downloading") ? "正在下载" : "下载任务"}</strong>
                  <p>{downloadJobs[0]?.targetPath?.split("\\").pop() ?? "暂无进行中的任务"}</p>
                  <div className="side-progress"><span style={{ width: `${selectedInstanceId ? (downloadProgress[selectedInstanceId] ?? 0) : 0}%` }} /></div>
                  <div className="side-progress-meta"><span>{selectedInstanceId ? `${downloadProgress[selectedInstanceId] ?? 0}%` : "—"}</span><span>{downloadJobs.length} 个任务</span></div>
                  <div className="side-actions"><button disabled={!busy}>Ⅱ</button><button disabled={!busy}>×</button></div>
                </section>
                <section className="recent-card">
                  <div className="side-card-title"><h3>最近活动</h3><button className="link-button">查看全部</button></div>
                  <div className="recent-item"><Download size={20} /><span><strong>启动器已就绪</strong><small>{selectedInstance?.name ?? "尚未创建实例"}</small></span><time>刚刚</time></div>
                  <div className="recent-item"><Play size={20} /><span><strong>开始游戏</strong><small>{selectedInstance?.gameVersion ?? "等待配置"}</small></span><time>—</time></div>
                  <div className="recent-item"><Puzzle size={20} /><span><strong>模组管理</strong><small>支持拖拽导入</small></span><time>—</time></div>
                </section>
              </aside>
            </div>
            <section className="instances">
              <div>
                <h2>我的游戏配置</h2>
                <p>每一套版本、模组和存档分开保存，不会互相影响。</p>
              </div>
              <button
                className="new"
                onClick={() =>
                  showInstanceForm
                    ? setShowInstanceForm(false)
                    : void openInstanceForm()
                }
              >
                + 新建游戏配置
              </button>
            </section>
            {showInstanceForm ? (
              <section className="instance-form">
                <input
                  value={instanceName}
                  onChange={(event) => setInstanceName(event.target.value)}
                  placeholder="给这套游戏起个名字"
                />
                <select
                  aria-label="模组运行环境"
                  value={instanceLoader}
                  onChange={(event) => setInstanceLoader(event.target.value)}
                >
                  {loaderOptions.map((loader) => (
                    <option key={loader} value={loader}>
                      {loaderLabel(loader)}
                    </option>
                  ))}
                </select>
                {versions.length ? (
                  <select
                    aria-label="Minecraft 版本"
                    value={gameVersion}
                    onChange={(event) => setGameVersion(event.target.value)}
                  >
                    {versions.map((version) => (
                      <option key={version.id} value={version.id}>
                        {version.id}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    value={gameVersion}
                    onChange={(event) => setGameVersion(event.target.value)}
                    placeholder={busy ? "正在读取官方版本…" : "Minecraft 版本"}
                  />
                )}
                <button
                  disabled={busy || !instanceName.trim() || !gameVersion}
                  onClick={() => void createInstance()}
                >
                  创建游戏配置
                </button>
                <small>
                  {instanceLoader === "vanilla"
                    ? "Vanilla 是纯原版，不使用模组运行环境。"
                    : `${loaderLabel(instanceLoader)} 是模组运行环境，创建后还需要点击安装。`}
                </small>
              </section>
            ) : null}
            <div className="rows">
              {instances.length ? (
                instances.map((instance) => (
                  <div key={instance.id}>
                    <span className="cube">◆</span>
                    <strong>{instance.name}</strong>
                    <small>
                      {loaderLabel(instance.loaderType)} {instance.gameVersion}{" "}
                      · {instance.memoryMb} MB
                    </small>
                    <em className="pending">
                      {instance.status === "loader_missing"
                        ? "模组环境待安装"
                        : instance.status === "ready"
                          ? "游戏文件已校验"
                          : downloadProgress[instance.id]
                            ? `${downloadProgress[instance.id]}%`
                            : "基础游戏待安装"}
                    </em>
                    <div className="instance-row-actions">
                      {loaderVersions[instance.id]?.length ? (
                        <select
                          className="loader-version"
                          aria-label={`${instance.name} 模组运行环境版本`}
                          value={loaderSelections[instance.id]}
                          onChange={(event) =>
                            setLoaderSelections((existing) => ({
                              ...existing,
                              [instance.id]: event.target.value,
                            }))
                          }
                        >
                          {loaderVersions[instance.id].map((version) => (
                            <option value={version} key={version}>
                              {version}
                            </option>
                          ))}
                        </select>
                      ) : null}
                      <button
                        className="install-client"
                        disabled={busy}
                        onClick={() => void installClient(instance)}
                      >
                        {instance.status === "ready"
                          ? "校验 / 修复"
                          : instance.status === "loader_missing" ||
                              clientReady[instance.id]
                            ? "校验基础游戏"
                            : "检查并补齐游戏"}
                      </button>
                      {instance.loaderType !== "vanilla" &&
                      (instance.status === "loader_missing" ||
                        clientReady[instance.id]) ? (
                        <button
                          className="install-loader"
                          disabled={busy}
                          onClick={() => void installInstanceLoader(instance)}
                        >
                          {loaderVersions[instance.id]?.length
                            ? "安装模组环境"
                            : "选择环境版本"}
                        </button>
                      ) : null}
                    </div>
                  </div>
                ))
              ) : (
                <div>
                  <span className="cube muted">◆</span>
                  <strong>还没有游戏配置</strong>
                  <small>选择游戏版本和模组运行环境后创建</small>
                  <em className="pending">待配置</em>
                </div>
              )}
            </div>
            <section className="onboard">
              <label htmlFor="profile">创建本地玩家名称</label>
              <div>
                <input
                  id="profile"
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void createProfile();
                  }}
                  placeholder="3–16 位：字母、数字或下划线"
                />
                <button disabled={busy} onClick={() => void createProfile()}>
                  {busy ? "保存中…" : "保存"}
                </button>
              </div>
              {message ? (
                <p className="form-message" role="alert">
                  {message}
                </p>
              ) : null}
            </section>
            {showHighlights ? (
              <VersionHighlightsModal
                onClose={() => {
                  setShowHighlights(false);
                  try {
                    localStorage.setItem(
                      "sh-launcher-highlights-seen",
                      APP_VERSION,
                    );
                  } catch {
                    // 忽略存储失败
                  }
                }}
              />
            ) : null}
            {showChangelog ? (
              <ChangelogModal onClose={() => setShowChangelog(false)} />
            ) : null}
            {showTutorial ? (
              <TutorialModal onClose={() => setShowTutorial(false)} />
            ) : null}
            {showOnboarding ? (
              <OnboardingGuide
                onClose={() => {
                  setShowOnboarding(false);
                  try {
                    localStorage.setItem("sh-onboarding-seen", "1");
                  } catch {
                    // 忽略存储失败
                  }
                }}
              />
            ) : null}
          </div>
        ) : activeNav === "联机" ? (
          <ServersPage
            servers={servers}
            instances={instances}
            accounts={accounts}
            selectedInstanceId={selectedInstanceId}
            selectedAccountId={current?.id}
            busy={busy}
            message={message}
            onAddServer={addServer}
            onUpdateServer={updateServer}
            onRemoveServer={removeServer}
            javaPath={selectedJava?.path}
            onJoin={(server, instanceId, accountId) => {
              const instance = instances.find((candidate) => candidate.id === instanceId);
              if (!instance) {
                setMessage("请先选择一套已就绪的游戏配置。");
                return;
              }
              setSelectedInstanceId(instanceId);
              selectAccount(accountId);
              void launchSelectedInstance(instance, false, server, accountId);
            }}
            onQuickJoin={(address, instanceId, accountId) => {
              const instance = instances.find(
                (candidate) => candidate.id === instanceId,
              );
              if (!instance) {
                setMessage("请先选择一套已就绪的游戏配置。");
                return;
              }
              setSelectedInstanceId(instanceId);
              selectAccount(accountId);
              void launchSelectedInstance(
                instance,
                false,
                {
                  id: 0,
                  name: "快速加入",
                  address,
                  port: 25565,
                  description: "",
                  createdAt: "",
                },
                accountId,
              );
            }}
          />
        ) : activeNav === "存储" ? (
          <StoragePage />
        ) : activeNav === "游戏库" || activeNav === "实例" ? (
          openInstanceId ? (
            (() => {
              const detailInstance = instances.find(
                (instance) => instance.id === openInstanceId,
              );
              if (!detailInstance) return null;
              return (
                <InstanceDetailPage
                  instance={detailInstance}
                  javaLabel={
                    selectedJava
                      ? `Java ${selectedJava.majorVersion ?? selectedJava.version} · 64 位`
                      : "未检测到 64 位 Java"
                  }
                  onBack={() => setOpenInstanceId(undefined)}
                  onLaunch={(instance) => {
                    setSelectedInstanceId(instance.id);
                    void launchSelectedInstance(instance);
                  }}
                  onRepair={(instance) => void repairInstance(instance)}
                  onOpenFolder={(instance) => void openInstanceDirectory(instance.id, "game")}
                  onMemoryChange={(instance, memoryMb) =>
                    void updateInstanceMemory(instance, memoryMb)
                  }
                />
              );
            })()
          ) : (
            <InstanceLibraryPage
            instances={instances}
            onCreate={() => { setActiveNav("主页"); setShowInstanceForm(true); }}
            onPlay={(instance) => { setSelectedInstanceId(instance.id); void launchSelectedInstance(instance); }}
            onClone={(instance) => void cloneInstance(instance)}
            onRename={(instance) => void renameInstance(instance)}
            onMemoryChange={(instance, memoryMb) =>
              void updateInstanceMemory(instance, memoryMb)
            }
            onRepair={(instance) => void repairInstance(instance)}
            onDelete={(instance) => void deleteInstance(instance)}
            onOpen={(instance) => void openInstanceDirectory(instance.id, "game")}
            onOpenDetails={(instance) => setOpenInstanceId(instance.id)}
          />
          )
        ) : activeNav === "模组" ? (
          <ModsPage
            instances={instances}
            selectedId={modInstanceId}
            onSelect={setModInstanceId}
            items={modItems}
            inspection={modInspection}
            busy={busy}
            message={message}
            onPick={() => void inspectMod()}
            onInstall={() => void installMod()}
            onToggle={(item) => void toggleMod(item)}
            onRemove={(item) => void removeMod(item)}
            queuedCount={modQueue.length}
            dragging={dragging}
            onlineQuery={onlineModQuery}
            onlineProjects={onlineModProjects}
            onOnlineQuery={setOnlineModQuery}
            onOnlineSearch={() => void searchOnline("mod")}
            onOnlineInstall={(project) => void installOnlineMod(project)}
            onTranslate={translateSearchText}
            onInstallCurseforgeUrl={(url) => void installCurseforgeUrl(url)}
            onlineLoader={onlineModLoader}
            onlineVersion={onlineModVersion}
            onOnlineLoader={setOnlineModLoader}
            onOnlineVersion={setOnlineModVersion}
            problemMods={modProblemMaps[modInstanceId ?? -1] ?? {}}
            updates={modUpdates}
            onCheckUpdates={() => void checkModUpdates()}
            onUpdate={(item) => void updateMod(item)}
            onUpdateAll={() => void updateAllMods()}
            backups={removedBackups}
            onRestore={(item) => void restoreBackup(item)}
            onOpenFolder={() => void openInstanceDirectory(modInstanceId, "mods")}
          />
        ) : activeNav === "整合包" ? (
          <ModpacksPage
            inspection={packInspection}
            busy={busy}
            message={message}
            dragging={dragging}
            onPick={() => void inspectPack()}
            onImport={() => void importPack()}
            instances={instances}
            targetId={modInstanceId}
            onTarget={setModInstanceId}
            onlineQuery={onlinePackQuery}
            onlineProjects={onlinePackProjects}
            onOnlineQuery={setOnlinePackQuery}
            onOnlineSearch={() => void searchOnline("modpack")}
            onOnlineInstall={(project) => void installOnlinePack(project)}
            onTranslate={translateSearchText}
            onExport={(instanceId, includeSaves) => void exportPack(instanceId, includeSaves)}
            archives={modpackArchives}
            javaRuntimes={javaRuntimes}
            onImportArchive={(archive) => void importArchiveAsNewInstance(archive)}
            onRemoveArchive={(archive) => void removeModpackArchive(archive)}
            onInstallJava={(major) => void installManagedJava(major)}
          />
        ) : activeNav === "账户" ? (
          <AccountsPage
            accounts={accounts}
            selectedAccountId={current?.id}
            busy={busy}
            message={message}
            onSelect={selectAccount}
            onRemove={(account) => void removeAccount(account)}
            onCreateOffline={async (name) => {
              setDraft(name);
              await createProfile();
            }}
            onOpenSettings={() => setActiveNav("设置")}
          />
        ) : activeNav === "设置" ? (
          <SettingsPage
            settings={settings}
            busy={busy}
            message={message}
            onChange={setSettings}
            onSave={() => void saveLauncherSettings()}
            onChooseExistingGameDirectory={() => void chooseExistingGameDirectory()}
            javaRuntimes={javaRuntimes}
            selectedJavaPath={selectedJava?.path}
            onSelectJava={setSelectedJavaPath}
            onInstallJava={(major) => void installManagedJava(major)}
            onCheckEnvironment={() => void checkEnvironment()}
            onSetupRecommended={() => void installManagedJava(21)}
            onLoginMicrosoft={() => void loginMicrosoft()}
            onLoginExternal={loginExternal}
            microsoftLoginAvailable={microsoftLoginAvailable}
            accounts={accounts}
            selectedAccountId={current?.id}
            onSelectAccount={selectAccount}
            onRemoveAccount={(account) => void removeAccount(account)}
            onCleanCache={() => void cleanLauncherCache()}
          />
        ) : activeNav === "下载" ? (
          <DiagnosticsPage
            jobs={downloadJobs}
            crashes={crashReports}
            busy={busy}
            message={message}
            onRefresh={() => void refreshDiagnostics()}
            onExport={() => void exportDiagnostics()}
            onCancel={() => void cancelDownloads()}
            logs={gameLogs}
            logText={gameLogText}
            onReadLog={(log, level, query) => void readGameLog(log, level, query)}
          />
        ) : activeNav === "资源包" || activeNav === "光影" ? (
          <ArchiveContentPage
            title={activeNav}
            kind={activeNav === "资源包" ? "resourcepack" : "shaderpack"}
            instances={instances}
            targetId={modInstanceId}
            items={archiveItems}
            busy={busy}
            message={message}
            dragging={dragging}
            onTarget={setModInstanceId}
            onImport={() =>
              void importArchives(
                activeNav === "资源包" ? "resourcepack" : "shaderpack",
              )
            }
            onToggle={(item) => void toggleArchive(item)}
            onRemove={(item) => void removeArchive(item)}
            backups={removedBackups}
            onRestore={(item) => void restoreBackup(item)}
            onOpenFolder={() => void openInstanceDirectory(modInstanceId, activeNav === "资源包" ? "resourcepacks" : "shaderpacks")}
          />
        ) : activeNav === "存档" ? (
          <WorldsPage
            instances={instances}
            targetId={modInstanceId}
            items={worldItems}
            busy={busy}
            message={message}
            dragging={dragging}
            onTarget={setModInstanceId}
            onFolder={() => void chooseAndImportWorld(true)}
            onZip={() => void chooseAndImportWorld(false)}
            onBackup={(item) => void backupWorld(item)}
            onDuplicate={(item) => void duplicateWorld(item)}
            onExport={(item) => void exportWorld(item)}
            onRemove={(item) => void removeWorld(item)}
            onDeletePermanent={(item) => void deleteWorldPermanently(item)}
            backups={removedBackups}
            onRestore={(item) => void restoreBackup(item)}
            onOpenFolder={() => void openInstanceDirectory(modInstanceId, "saves")}
          />
        ) : (
          <ComingSoonPage title={activeNav} />
        )}
      </section>
      </main>
      <GlobalProgressBar
        visible={busy || downloading || activeDownloadJobs.length > 0}
        message={
          activeDownloadJobs.length
            ? `${activeDownloadJobs.length} 个下载目标 · 总进度 ${aggregateDownloadPercent ?? "—"}%`
            : busy || downloading
              ? "正在处理…"
              : "就绪"
        }
        progress={aggregateDownloadPercent}
        onClick={() => setShowDownloadDetails(true)}
      />
      {showDownloadDetails ? (
        <DownloadDetailsModal
          jobs={downloadJobs}
          instanceProgress={downloadProgress}
          instances={instances}
          onClose={() => setShowDownloadDetails(false)}
        />
      ) : null}
      {errorModal ? (
        <ErrorModal
          title={errorModal.title}
          lines={errorModal.lines}
          actionLabel={errorModal.actionLabel}
          onAction={errorModal.action}
          secondaryLabel={errorModal.secondaryLabel}
          onSecondary={errorModal.onSecondary}
          onClose={() => setErrorModal(null)}
        />
      ) : null}
      {incompatibleGroups.length ? (
        <IncompatibleModsModal
          groups={incompatibleGroups}
          onDelete={() => void deleteIncompatibleMods()}
          onClose={() => setIncompatibleGroups([])}
        />
      ) : null}
    </div>
  );
}
