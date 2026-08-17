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
  OnlineProject,
  ModpackArchive,
} from "./types";
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
import type { BootHealthReport } from "./types/splash";
import { APP_VERSION, RELEASE_CHANNEL_LABEL } from "./version";
import { highlightsFor } from "./versionHighlights";
import { errorText, inspectionSupportsGame, loaderLabel } from "./ui";
import { AppShell } from "./app/AppShell";
import {
  ThemeProvider,
  ToastProvider,
  ToastStack,
} from "./app/providers";
import type { ThemeMode } from "./app/providers";
import type { AppRoute, DiscoverTab, SettingsTab } from "./app/Router";
import { HomePage } from "./features/home/HomePage";
import type {
  LoaderVersionRecord,
  PlayHistoryEntry,
} from "./features/home/HomePage";
import { LibraryPage } from "./features/library/LibraryPage";
import { InstancePage } from "./features/instance/InstancePage";
import type { ContentKind } from "./features/instance/InstancePage";
import { DiscoverPage } from "./features/discover/DiscoverPage";
import { DownloadsPage } from "./features/downloads/DownloadsPage";
import { AccountsPage } from "./features/accounts/AccountsPage";
import { SettingsPage } from "./features/settings/SettingsPage";
import "./ui/tokens.css";
import "./ui/globals.css";
import "./ui/components.css";
import "./ui/motion.css";
import "./ui/pages.css";
import "./ui/shell.css";
import "./ui/polish.css";

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

export default function App() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [selectedAccountId, setSelectedAccountId] = useState<number>();
  const [modpackArchives, setModpackArchives] = useState<ModpackArchive[]>([]);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [selectedInstanceId, setSelectedInstanceId] = useState<number>();
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
  const [route, setRoute] = useState<AppRoute>({ name: "home" });
  const [discoverTab, setDiscoverTab] = useState<DiscoverTab>("mods");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general");
  const [contentKind, setContentKind] = useState<ContentKind>(undefined);
  const [playHistory, setPlayHistory] = useState<PlayHistoryEntry[]>([]);
  const [showInstanceForm, setShowInstanceForm] = useState(false);
  const [instanceName, setInstanceName] = useState("");
  const [gameVersion, setGameVersion] = useState("");
  const [instanceLoader, setInstanceLoader] = useState("vanilla");
  const [downloadProgress, setDownloadProgress] = useState<
    Record<number, number>
  >({});
  const [loaderVersions, setLoaderVersions] = useState<
    Record<number, string[]>
  >({});
  const [loaderSelections, setLoaderSelections] = useState<
    Record<number, string>
  >({});
  const [loaderBuilds, setLoaderBuilds] = useState<LoaderVersionRecord[]>([]);
  const [selectedLoaderBuild, setSelectedLoaderBuild] = useState("");
  const [buildsLoading, setBuildsLoading] = useState(false);
  const [buildsCachedAt, setBuildsCachedAt] = useState<string>();
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
    if (isTauri()) {
      setIsSplash(getCurrentWindow().label === "splash");
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
      archivesResult,
      playHistoryResult,
    ] = await Promise.allSettled([
      invoke<Account[]>("list_accounts"),
      invoke<{ activeAccountId?: number; defaultAccountId?: number }>(
        "get_account_state",
      ),
      invoke<Instance[]>("list_instances"),
      invoke<JavaRuntime[]>("detect_java_runtimes"),
      invoke<LauncherSettings>("get_settings"),
      invoke<boolean>("microsoft_login_available"),
      invoke<ModpackArchive[]>("list_modpack_archives"),
      invoke<PlayHistoryEntry[]>("list_play_history"),
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

    if (archivesResult.status === "fulfilled") {
      setModpackArchives(archivesResult.value);
    } else {
      setMessage(errorText(archivesResult.reason, "无法读取已下载整合包列表。"));
    }

    if (playHistoryResult.status === "fulfilled") {
      setPlayHistory(playHistoryResult.value);
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
              .join("、")}（启动前会自动补齐）`,
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
    if (
      !isTauri() ||
      !showInstanceForm ||
      !gameVersion.trim() ||
      instanceLoader === "vanilla"
    ) {
      setLoaderBuilds([]);
      setSelectedLoaderBuild("");
      setBuildsCachedAt(undefined);
      return;
    }
    let cancelled = false;
    setBuildsLoading(true);
    invoke<{
      versions: LoaderVersionRecord[];
      fromCache: boolean;
      fetchedAt: string;
    }>("list_loader_builds", {
      loaderType: instanceLoader,
      gameVersion: gameVersion.trim(),
    })
      .then((result) => {
        if (cancelled) return;
        setLoaderBuilds(result.versions);
        setBuildsCachedAt(result.fetchedAt);
        const recommended =
          result.versions.find((build) => build.recommended) ??
          result.versions.find((build) => build.latest) ??
          result.versions[0];
        setSelectedLoaderBuild(
          (existing) => existing || recommended?.version || "",
        );
      })
      .catch(() => {
        if (cancelled) return;
        setLoaderBuilds([]);
        setSelectedLoaderBuild("");
      })
      .finally(() => {
        if (!cancelled) setBuildsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [showInstanceForm, gameVersion, instanceLoader]);

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
      contentKind === "resourcepack" || contentKind === "shaderpack"
        ? contentKind
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
  }, [contentKind, modInstanceId]);

  useEffect(() => {
    const kind = contentKind;
    if (!isTauri() || !modInstanceId || !kind) {
      setRemovedBackups([]);
      return;
    }
    invoke<BackupItem[]>("list_removed_backups", { instanceId: modInstanceId, kind })
      .then(setRemovedBackups)
      .catch((error: unknown) => setMessage(errorText(error, "无法读取可恢复备份。")));
  }, [contentKind, modInstanceId]);

  useEffect(() => {
    if (!isTauri() || !modInstanceId || contentKind !== "world") {
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
  }, [contentKind, modInstanceId]);

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
    const dropKind =
      route.name === "discover" && discoverTab === "modpacks"
        ? "modpack"
        : contentKind;
    void getCurrentWebviewWindow()
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter" && dropKind) setDragging(true);
        if (event.payload.type === "leave") setDragging(false);
        if (event.payload.type !== "drop") return;
        setDragging(false);
        const path = event.payload.paths[0];
        if (!path) return;
        setBusy(true);
        setMessage("");
        if (dropKind === "mod") {
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
        } else if (dropKind === "modpack") {
          invoke<ModpackInspection>("inspect_modpack", { path })
            .then((inspection) => {
              setPackInspection(inspection);
              setPackSourcePath(path);
            })
            .catch((error: unknown) =>
              setMessage(errorText(error, "拖入的整合包无法通过预检。")),
            )
            .finally(() => setBusy(false));
        } else if (
          (dropKind === "resourcepack" || dropKind === "shaderpack") &&
          modInstanceId
        ) {
          const kind = dropKind;
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
              setMessage(`已导入 ${items.length} 个${kind === "resourcepack" ? "资源包" : "光影"}。`);
            })
            .catch((error: unknown) =>
              setMessage(
                errorText(
                  error,
                  `${kind === "resourcepack" ? "资源包" : "光影"}导入失败。`,
                ),
              ),
            )
            .finally(() => setBusy(false));
        } else if (dropKind === "world" && modInstanceId) {
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
  }, [route.name, discoverTab, contentKind, modInstanceId]);

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
      const [jobs, crashes] = await Promise.all([
        invoke<DownloadJob[]>("list_download_jobs"),
        invoke<CrashReport[]>("list_crash_reports"),
      ]);
      setDownloadJobs(jobs);
      setCrashReports(crashes);
    } catch (error) {
      setMessage(errorText(error, "无法读取诊断信息。"));
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
      if (instanceLoader !== "vanilla" && selectedLoaderBuild) {
        setLoaderSelections((existing) => ({
          ...existing,
          [instance.id]: selectedLoaderBuild,
        }));
      }
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
      ready = await installInstanceLoaderFiles(
        ready,
        java?.path,
        options.inspection?.loaderVersion ?? undefined,
      );
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
    exactLoaderVersion?: string,
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
    const loaderVersion =
      exactLoaderVersion ??
      loaderSelections[instance.id] ??
      available[0];
    if (exactLoaderVersion && !available.includes(exactLoaderVersion)) {
      available = [exactLoaderVersion, ...available];
      setLoaderVersions((existing) => ({
        ...existing,
        [instance.id]: available,
      }));
    }
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
  ) {
    const requestedInstance = targetInstance ?? selectedInstance;
    if (!requestedInstance) {
      setMessage("还没有游戏配置，请先新建一套游戏配置。");
      return;
    }
    setBusy(true);
    setMessage("正在准备游戏…");
    try {
      let launchAccount = current;
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
        },
      );
      setMessage(
        `游戏进程已启动（PID ${result.processId}），日志：${result.logPath}`,
      );
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
      setMessage(errorText(error, "自动补齐失败，已阻止启动。请修复后重试。"));
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
        errorText(error, "整合包导入未完成，已撤销本次导入，没有提交不完整实例。"),
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
      const imported = await invoke<ImportedModpack>("import_curseforge_pack", {
        sourcePath,
      });
      return await finishNewInstanceImport(imported.instance, {
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

  const selectedInstance =
    instances.find((instance) => instance.id === selectedInstanceId) ??
    instances[0];
  const selectedJava =
    javaRuntimes.find((runtime) => runtime.path === selectedJavaPath) ??
    javaRuntimes.find((runtime) => runtime.is64Bit);
  const [themeMode, setThemeMode] = useState<ThemeMode>(() => {
    try {
      const saved = localStorage.getItem("sh-ui3-theme");
      if (saved === "light" || saved === "system") return saved;
    } catch {
      // 存储不可用时使用深色
    }
    return "dark";
  });

  const navigate = (next: AppRoute) => {
    setRoute(next);
    setMessage("");
    if (next.name === "discover") {
      if (next.tab) setDiscoverTab(next.tab);
      setContentKind(
        next.tab === "mods"
          ? "mod"
          : next.tab === "resourcepacks"
            ? "resourcepack"
            : next.tab === "shaders"
              ? "shaderpack"
              : undefined,
      );
    } else if (next.name === "instance") {
      setSelectedInstanceId(next.instanceId);
      setModInstanceId(next.instanceId);
      setContentKind(
        next.tab === "mods"
          ? "mod"
          : next.tab === "resourcepacks"
            ? "resourcepack"
            : next.tab === "shaders"
              ? "shaderpack"
              : next.tab === "worlds"
                ? "world"
                : undefined,
      );
    } else if (next.name === "settings") {
      if (next.tab) setSettingsTab(next.tab);
      setContentKind(undefined);
    } else {
      setContentKind(undefined);
    }
  };

  if (isSplash) {
    return <SplashView />;
  }

  const detailInstance =
    route.name === "instance"
      ? instances.find((instance) => instance.id === route.instanceId)
      : undefined;

  const content = (
    <>
      {route.name === "home" ? (
        <HomePage
          accountName={current?.displayName}
          selectedInstance={selectedInstance}
          instances={instances}
          onSelectInstance={setSelectedInstanceId}
          selectedJava={selectedJava}
          gameRunning={gameRunning}
          busy={busy}
          downloading={downloading}
          onLaunch={() => void launchSelectedInstance()}
          onTerminate={() => void terminateRunningGame()}
          onOpenLibrary={() => navigate({ name: "library" })}
          onOpenInstance={(instanceId) =>
            navigate({ name: "instance", instanceId })
          }
          bootProblems={bootProblems}
          update={bootUpdate}
          updateChecking={updateChecking}
          updateCheckError={updateCheckError}
          onRetryUpdate={() => void runUpdateCheck()}
          onOpenOnboarding={() => setShowOnboarding(true)}
          downloadJobs={downloadJobs}
          downloadProgress={downloadProgress}
          aggregateDownloadPercent={aggregateDownloadPercent}
          onOpenDownloads={() => navigate({ name: "downloads" })}
          playHistory={playHistory}
          versions={versions}
          showInstanceForm={showInstanceForm}
          instanceName={instanceName}
          gameVersion={gameVersion}
          instanceLoader={instanceLoader}
          onInstanceName={setInstanceName}
          onGameVersion={setGameVersion}
          onInstanceLoader={setInstanceLoader}
          onToggleInstanceForm={() => {
            if (showInstanceForm) setShowInstanceForm(false);
            else void openInstanceForm();
          }}
          onCreateInstance={() => void createInstance()}
          loaderBuilds={loaderBuilds}
          selectedLoaderBuild={selectedLoaderBuild}
          buildsLoading={buildsLoading}
          buildsCachedAt={buildsCachedAt}
          onLoaderBuildsChange={setSelectedLoaderBuild}
          onOpenModpacks={() => navigate({ name: "discover", tab: "modpacks" })}
        />
      ) : null}
      {route.name === "library" ? (
        <LibraryPage
          instances={instances}
          onCreate={() => {
            navigate({ name: "home" });
            void openInstanceForm();
          }}
          onPlay={(instance) => {
            setSelectedInstanceId(instance.id);
            void launchSelectedInstance(instance);
          }}
          onClone={(instance) => void cloneInstance(instance)}
          onRename={(instance) => void renameInstance(instance)}
          onMemoryChange={(instance, memoryMb) =>
            void updateInstanceMemory(instance, memoryMb)
          }
          onRepair={(instance) => void repairInstance(instance)}
          onDelete={(instance) => void deleteInstance(instance)}
          onOpen={(instance) => void openInstanceDirectory(instance.id, "game")}
          onOpenDetails={(instance) =>
            navigate({ name: "instance", instanceId: instance.id })
          }
        />
      ) : null}
      {route.name === "instance" && detailInstance ? (
        <InstancePage
          instance={detailInstance}
          javaLabel={
            selectedJava
              ? `Java ${selectedJava.majorVersion ?? selectedJava.version} · 64 位`
              : "未检测到 64 位 Java"
          }
          onBack={() => navigate({ name: "library" })}
          onSwitchInstance={(instanceId, tab) =>
            navigate({ name: "instance", instanceId, tab })
          }
          onOpenSettings={() => navigate({ name: "settings", tab: "game" })}
          onLaunch={(instance) => {
            setSelectedInstanceId(instance.id);
            void launchSelectedInstance(instance);
          }}
          onRepair={(instance) => void repairInstance(instance)}
          onClone={(instance) => void cloneInstance(instance)}
          onRename={(instance) => void renameInstance(instance)}
          onDelete={(instance) => void deleteInstance(instance)}
          onExport={(instanceId, includeSaves) =>
            void exportPack(instanceId, includeSaves)
          }
          onOpenFolder={(instanceId, section) =>
            void openInstanceDirectory(instanceId, section)
          }
          onMemoryChange={(instance, memoryMb) =>
            void updateInstanceMemory(instance, memoryMb)
          }
          onContentKindChange={setContentKind}
          busy={busy}
          message={message}
          downloadProgress={downloadProgress}
          instances={instances}
          modItems={modItems}
          modInspection={modInspection}
          modQueueCount={modQueue.length}
          dragging={dragging}
          onlineModQuery={onlineModQuery}
          onlineModProjects={onlineModProjects}
          modLoader={onlineModLoader}
          modVersion={onlineModVersion}
          problemMods={modProblemMaps[detailInstance.id] ?? {}}
          modUpdates={modUpdates}
          removedBackups={removedBackups}
          onPickMod={() => void inspectMod()}
          onInstallMod={() => void installMod()}
          onToggleMod={(item) => void toggleMod(item)}
          onRemoveMod={(item) => void removeMod(item)}
          onOnlineModQuery={setOnlineModQuery}
          onOnlineModSearch={() => void searchOnline("mod")}
          onOnlineModInstall={(project) => void installOnlineMod(project)}
          onTranslate={translateSearchText}
          onInstallCurseforgeUrl={(url) => void installCurseforgeUrl(url)}
          onOnlineModLoader={setOnlineModLoader}
          onOnlineModVersion={setOnlineModVersion}
          onCheckModUpdates={() => void checkModUpdates()}
          onUpdateMod={(item) => void updateMod(item)}
          onUpdateAllMods={() => void updateAllMods()}
          onRestoreBackup={(item) => void restoreBackup(item)}
          archiveItems={archiveItems}
          onToggleArchive={(item) => void toggleArchive(item)}
          onRemoveArchive={(item) => void removeArchive(item)}
          onImportArchive={(kind) => void importArchives(kind)}
          worldItems={worldItems}
          onImportWorldFolder={() => void chooseAndImportWorld(true)}
          onImportWorldZip={() => void chooseAndImportWorld(false)}
          onBackupWorld={(item) => void backupWorld(item)}
          onDuplicateWorld={(item) => void duplicateWorld(item)}
          onExportWorld={(item) => void exportWorld(item)}
          onRemoveWorld={(item) => void removeWorld(item)}
          onDeleteWorldPermanently={(item) => void deleteWorldPermanently(item)}
          crashes={crashReports}
          onRefreshDiagnostics={() => void refreshDiagnostics()}
        />
      ) : null}
      {route.name === "discover" ? (
        <DiscoverPage
          tab={discoverTab}
          onTab={setDiscoverTab}
          onContentKindChange={setContentKind}
          instances={instances}
          targetId={modInstanceId}
          onTarget={setModInstanceId}
          busy={busy}
          message={message}
          dragging={dragging}
          modItems={modItems}
          modInspection={modInspection}
          modQueueCount={modQueue.length}
          onlineModQuery={onlineModQuery}
          onlineModProjects={onlineModProjects}
          modLoader={onlineModLoader}
          modVersion={onlineModVersion}
          problemMods={modProblemMaps[modInstanceId ?? -1] ?? {}}
          modUpdates={modUpdates}
          removedBackups={removedBackups}
          onPickMod={() => void inspectMod()}
          onInstallMod={() => void installMod()}
          onToggleMod={(item) => void toggleMod(item)}
          onRemoveMod={(item) => void removeMod(item)}
          onOnlineModQuery={setOnlineModQuery}
          onOnlineModSearch={() => void searchOnline("mod")}
          onOnlineModInstall={(project) => void installOnlineMod(project)}
          onTranslate={translateSearchText}
          onInstallCurseforgeUrl={(url) => void installCurseforgeUrl(url)}
          onOnlineModLoader={setOnlineModLoader}
          onOnlineModVersion={setOnlineModVersion}
          onCheckModUpdates={() => void checkModUpdates()}
          onUpdateMod={(item) => void updateMod(item)}
          onUpdateAllMods={() => void updateAllMods()}
          onRestoreBackup={(item) => void restoreBackup(item)}
          onOpenModFolder={() => void openInstanceDirectory(modInstanceId, "mods")}
          packInspection={packInspection}
          onlinePackQuery={onlinePackQuery}
          onlinePackProjects={onlinePackProjects}
          archives={modpackArchives}
          javaRuntimes={javaRuntimes}
          onPickPack={() => void inspectPack()}
          onImportPack={(gameVersion, loaderType) =>
            void importPack(gameVersion, loaderType)
          }
          onOnlinePackQuery={setOnlinePackQuery}
          onOnlinePackSearch={() => void searchOnline("modpack")}
          onOnlinePackInstall={(project) => void installOnlinePack(project)}
          onExportPack={(instanceId, includeSaves) =>
            void exportPack(instanceId, includeSaves)
          }
          onImportArchiveInstance={(archive) =>
            void importArchiveAsNewInstance(archive)
          }
          onRemoveArchive={(archive) => void removeModpackArchive(archive)}
          onInstallJava={(major) => void installManagedJava(major)}
          archiveItems={archiveItems}
          onToggleArchive={(item) => void toggleArchive(item)}
          onRemoveArchiveItem={(item) => void removeArchive(item)}
          onImportArchive={(kind) => void importArchives(kind)}
          onOpenArchiveFolder={(kind) =>
            void openInstanceDirectory(modInstanceId, kind)
          }
        />
      ) : null}
      {route.name === "downloads" ? (
        <DownloadsPage
          jobs={downloadJobs}
          busy={busy}
          message={message}
          onRefresh={() => void refreshDiagnostics()}
          onExport={() => void exportDiagnostics()}
          onCancel={() => void cancelDownloads()}
        />
      ) : null}
      {route.name === "accounts" ? (
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
          onOpenSettings={() => navigate({ name: "settings", tab: "general" })}
        />
      ) : null}
      {route.name === "settings" ? (
        <SettingsPage
          tab={settingsTab}
          onTab={setSettingsTab}
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
          onExportDiagnostics={() => void exportDiagnostics()}
          themeMode={themeMode}
          onThemeMode={setThemeMode}
          version={APP_VERSION}
          channelLabel={RELEASE_CHANNEL_LABEL}
        />
      ) : null}
    </>
  );

  return (
    <ThemeProvider mode={themeMode} onModeChange={setThemeMode}>
      <ToastProvider>
        <AppShell
          route={route}
          onNavigate={navigate}
          account={current}
          accounts={accounts}
          onSelectAccount={selectAccount}
          version={APP_VERSION}
          channelLabel={RELEASE_CHANNEL_LABEL}
          gameRunning={gameRunning}
          onOpenChangelog={() => setShowChangelog(true)}
          onOpenTutorial={() => setShowTutorial(true)}
        >
          {content}
        </AppShell>
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
        <ToastStack />
      </ToastProvider>
    </ThemeProvider>
  );
}
