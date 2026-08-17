import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FolderOpen, Gamepad2, Play, Wrench } from "lucide-react";
import type {
  BackupItem,
  ContentItem,
  CrashReport,
  Instance,
  InstanceHealth,
  ModInspection,
  ModUpdateInfo,
  OnlineProject,
} from "../../types";
import { loaderLabel } from "../../ui";
import {
  ArchiveContentPage,
  ModsPage,
  WorldsPage,
} from "../../pages/ContentPages";
import { Badge, Button, Progress, Tabs } from "../../ui/components";
import { INSTANCE_TABS } from "../../app/Router";
import type { InstanceTab } from "../../app/Router";
import grassBlock from "../../assets/grass-block.png";

export type ContentKind = "mod" | "resourcepack" | "shaderpack" | "world" | undefined;

const MEMORY_PRESETS = [4096, 6144, 8192, 10240, 12288, 14336, 16384];

export type InstancePageProps = {
  instance: Instance;
  javaLabel: string;
  onBack: () => void;
  onSwitchInstance: (instanceId: number, tab?: InstanceTab) => void;
  onOpenSettings: () => void;
  onLaunch: (instance: Instance) => void;
  onRepair: (instance: Instance) => void;
  onClone: (instance: Instance) => void;
  onRename: (instance: Instance) => void;
  onDelete: (instance: Instance) => void;
  onExport: (instanceId: number, includeSaves: boolean) => void;
  onOpenFolder: (instanceId: number, section: string) => void;
  onMemoryChange: (instance: Instance, memoryMb: number) => void;
  onContentKindChange: (kind: ContentKind) => void;
  busy: boolean;
  message: string;
  downloadProgress: Record<number, number>;
  instances: Instance[];
  // Mods
  modItems: ContentItem[];
  modInspection?: ModInspection;
  modQueueCount: number;
  dragging: boolean;
  onlineModQuery: string;
  onlineModProjects: OnlineProject[];
  modLoader: string;
  modVersion: string;
  problemMods: Record<string, string>;
  modUpdates: ModUpdateInfo[];
  removedBackups: BackupItem[];
  onPickMod: () => void;
  onInstallMod: () => void;
  onToggleMod: (item: ContentItem) => void;
  onRemoveMod: (item: ContentItem) => void;
  onOnlineModQuery: (value: string) => void;
  onOnlineModSearch: () => void;
  onOnlineModInstall: (project: OnlineProject) => void;
  onTranslate: (text: string) => Promise<string | undefined>;
  onInstallCurseforgeUrl: (url: string) => void;
  onOnlineModLoader: (value: string) => void;
  onOnlineModVersion: (value: string) => void;
  onCheckModUpdates: () => void;
  onUpdateMod: (item: ContentItem) => void;
  onUpdateAllMods: () => void;
  onRestoreBackup: (item: BackupItem) => void;
  // Resource packs / shaders
  archiveItems: ContentItem[];
  onToggleArchive: (item: ContentItem) => void;
  onRemoveArchive: (item: ContentItem) => void;
  onImportArchive: (kind: "resourcepack" | "shaderpack") => void;
  // Worlds
  worldItems: ContentItem[];
  onImportWorldFolder: () => void;
  onImportWorldZip: () => void;
  onBackupWorld: (item: ContentItem) => void;
  onDuplicateWorld: (item: ContentItem) => void;
  onExportWorld: (item: ContentItem) => void;
  onRemoveWorld: (item: ContentItem) => void;
  onDeleteWorldPermanently: (item: ContentItem) => void;
  // Logs / diagnostics
  crashes: CrashReport[];
  onRefreshDiagnostics: () => void;
};

type GameLog = {
  instanceId: number;
  fileName: string;
  size: number;
  modifiedAt: number;
};

export function InstancePage({
  instance,
  javaLabel,
  onBack,
  onSwitchInstance,
  onOpenSettings,
  onLaunch,
  onRepair,
  onClone,
  onRename,
  onDelete,
  onExport,
  onOpenFolder,
  onMemoryChange,
  onContentKindChange,
  busy,
  message,
  downloadProgress,
  instances,
  modItems,
  modInspection,
  modQueueCount,
  dragging,
  onlineModQuery,
  onlineModProjects,
  modLoader,
  modVersion,
  problemMods,
  modUpdates,
  removedBackups,
  onPickMod,
  onInstallMod,
  onToggleMod,
  onRemoveMod,
  onOnlineModQuery,
  onOnlineModSearch,
  onOnlineModInstall,
  onTranslate,
  onInstallCurseforgeUrl,
  onOnlineModLoader,
  onOnlineModVersion,
  onCheckModUpdates,
  onUpdateMod,
  onUpdateAllMods,
  onRestoreBackup,
  archiveItems,
  onToggleArchive,
  onRemoveArchive,
  onImportArchive,
  worldItems,
  onImportWorldFolder,
  onImportWorldZip,
  onBackupWorld,
  onDuplicateWorld,
  onExportWorld,
  onRemoveWorld,
  onDeleteWorldPermanently,
  crashes,
  onRefreshDiagnostics,
}: InstancePageProps) {
  const [tab, setTab] = useState<InstanceTab>("overview");
  const [health, setHealth] = useState<InstanceHealth | null>(null);
  const [healthError, setHealthError] = useState("");
  const [logs, setLogs] = useState<GameLog[]>([]);
  const [logText, setLogText] = useState("");
  const [memory, setMemory] = useState(String(instance.memoryMb));

  const refreshHealth = useCallback(async () => {
    setHealthError("");
    try {
      setHealth(
        await invoke<InstanceHealth>("instance_health", {
          instanceId: instance.id,
        }),
      );
    } catch (error) {
      setHealthError(String(error));
    }
  }, [instance.id]);

  useEffect(() => {
    void refreshHealth();
  }, [refreshHealth, tab]);

  const loadLogs = useCallback(async () => {
    try {
      const all = await invoke<GameLog[]>("list_game_logs");
      setLogs(
        all
          .filter((log) => log.instanceId === instance.id)
          .toSorted((left, right) => right.modifiedAt - left.modifiedAt)
          .slice(0, 40),
      );
      setLogText("");
    } catch {
      setLogs([]);
    }
  }, [instance.id]);

  async function readLog(fileName: string) {
    try {
      setLogText(
        await invoke<string>("read_game_log", {
          instanceId: instance.id,
          fileName,
          level: "",
          query: "",
        }),
      );
    } catch {
      setLogText("读取日志失败。");
    }
  }

  useEffect(() => {
    if (tab === "logs") void loadLogs();
  }, [tab, loadLogs]);

  const instanceCrashes = crashes.filter(
    (crash) => crash.instanceId === instance.id,
  );
  const progress = downloadProgress[instance.id];

  const changeTab = (next: InstanceTab) => {
    setTab(next);
    onContentKindChange(
      next === "mods"
        ? "mod"
        : next === "resourcepacks"
          ? "resourcepack"
          : next === "shaders"
            ? "shaderpack"
            : next === "worlds"
              ? "world"
              : undefined,
    );
  };

  return (
    <div className="ui3-page-enter">
      <div className="instance-hero">
        <div className="instance-hero-art">
          <img src={grassBlock} alt="" />
        </div>
        <div className="instance-hero-copy">
          <h1>{instance.name}</h1>
          <p>
            Minecraft {instance.gameVersion} · {loaderLabel(instance.loaderType)} ·{" "}
            {javaLabel}
          </p>
        </div>
        <div className="instance-hero-actions">
          <Button variant="primary" onClick={() => onLaunch(instance)}>
            <Play size={16} fill="currentColor" />
            启动
          </Button>
          <Button onClick={() => onRepair(instance)}>
            <Wrench size={15} />
            修复
          </Button>
          <Button
            onClick={() => onOpenFolder(instance.id, "game")}
            title="打开游戏目录"
          >
            <FolderOpen size={15} />
            文件夹
          </Button>
          <Button variant="quiet" onClick={onBack}>
            返回游戏库
          </Button>
        </div>
      </div>

      <Tabs
        tabs={INSTANCE_TABS}
        value={tab}
        onChange={changeTab}
        label="实例功能"
      />

      {tab === "overview" ? (
        <section className="ui3-page">
          <div className="home-grid">
            <section className="ui3-card">
              <div className="ui3-section-head">
                <h2>健康状态</h2>
                {health ? (
                  <Badge tone={health.status === "ready" ? "success" : "warning"}>
                    {health.status === "ready" ? "已就绪" : "待处理"}
                  </Badge>
                ) : null}
              </div>
              {health ? (
                <ul className="instance-health-list">
                  <li>
                    {health.gameFilesOk ? "✓ 游戏文件完整" : "⚠ 游戏文件待安装"}
                  </li>
                  <li>
                    {health.loaderType} {health.loaderVersion ?? ""} · 模组{" "}
                    {health.modCount} 个
                  </li>
                  <li>
                    {health.missingDependencies.length
                      ? `⚠ 缺失前置：${health.missingDependencies.join("、")}`
                      : "✓ 前置完整"}
                  </li>
                  {health.incompatibleMods.length ? (
                    <li className="danger">
                      ⚠ 不兼容模组：
                      {health.incompatibleMods.slice(0, 6).join("、")}
                    </li>
                  ) : null}
                </ul>
              ) : healthError ? (
                <p className="ui3-secondary">{healthError}</p>
              ) : (
                <Progress indeterminate />
              )}
              {progress !== undefined ? (
                <div style={{ marginTop: 10 }}>
                  <Progress value={progress} />
                  <p className="ui3-muted" style={{ marginTop: 4 }}>
                    实例安装进度 {Math.round(progress)}%
                  </p>
                </div>
              ) : null}
            </section>

            <div className="home-side-col">
              <section className="ui3-card">
                <div className="ui3-section-head">
                  <h2>实例操作</h2>
                </div>
                <div className="ui3-grid" style={{ gridTemplateColumns: "1fr 1fr" }}>
                  <Button onClick={() => onClone(instance)}>复制实例</Button>
                  <Button onClick={() => onRename(instance)}>重命名</Button>
                  <Button onClick={() => onExport(instance.id, true)}>导出整合包</Button>
                  <Button variant="danger-quiet" onClick={() => onDelete(instance)}>
                    删除实例
                  </Button>
                </div>
              </section>
              <section className="ui3-card">
                <div className="ui3-section-head">
                  <h2>实例信息</h2>
                </div>
                <ul className="instance-health-list">
                  <li>运行内存：{instance.memoryMb} MB</li>
                  <li>来源：{instance.source === "modrinth" ? "Modrinth" : "本地"}</li>
                  <li>Java：{javaLabel}</li>
                </ul>
              </section>
            </div>
          </div>
          {message ? (
            <p className="form-message" role="status">
              {message}
            </p>
          ) : null}
        </section>
      ) : null}

      {tab === "mods" ? (
        <ModsPage
          instances={instances}
          selectedId={instance.id}
          onSelect={(id) => onSwitchInstance(id, "mods")}
          items={modItems}
          inspection={modInspection}
          busy={busy}
          message={message}
          onPick={onPickMod}
          onInstall={onInstallMod}
          onToggle={onToggleMod}
          onRemove={onRemoveMod}
          queuedCount={modQueueCount}
          dragging={dragging}
          onlineQuery={onlineModQuery}
          onlineProjects={onlineModProjects}
          onOnlineQuery={onOnlineModQuery}
          onOnlineSearch={onOnlineModSearch}
          onOnlineInstall={onOnlineModInstall}
          onTranslate={onTranslate}
          onInstallCurseforgeUrl={onInstallCurseforgeUrl}
          onlineLoader={modLoader}
          onlineVersion={modVersion}
          onOnlineLoader={onOnlineModLoader}
          onOnlineVersion={onOnlineModVersion}
          problemMods={problemMods}
          updates={modUpdates}
          onCheckUpdates={onCheckModUpdates}
          onUpdate={onUpdateMod}
          onUpdateAll={onUpdateAllMods}
          backups={removedBackups}
          onRestore={onRestoreBackup}
          onOpenFolder={() => onOpenFolder(instance.id, "mods")}
        />
      ) : null}

      {tab === "resourcepacks" || tab === "shaders" ? (
        <ArchiveContentPage
          title={tab === "resourcepacks" ? "资源包" : "光影"}
          kind={tab === "resourcepacks" ? "resourcepack" : "shaderpack"}
          instances={instances}
          targetId={instance.id}
          items={archiveItems}
          busy={busy}
          message={message}
          dragging={dragging}
          onTarget={(id) =>
            onSwitchInstance(id, tab === "resourcepacks" ? "resourcepacks" : "shaders")
          }
          onImport={() =>
            onImportArchive(tab === "resourcepacks" ? "resourcepack" : "shaderpack")
          }
          onToggle={onToggleArchive}
          onRemove={onRemoveArchive}
          backups={removedBackups}
          onRestore={onRestoreBackup}
          onOpenFolder={() =>
            onOpenFolder(
              instance.id,
              tab === "resourcepacks" ? "resourcepacks" : "shaderpacks",
            )
          }
        />
      ) : null}

      {tab === "worlds" ? (
        <WorldsPage
          instances={instances}
          targetId={instance.id}
          items={worldItems}
          busy={busy}
          message={message}
          dragging={dragging}
          onTarget={(id) => onSwitchInstance(id, "worlds")}
          onFolder={onImportWorldFolder}
          onZip={onImportWorldZip}
          onBackup={onBackupWorld}
          onDuplicate={onDuplicateWorld}
          onExport={onExportWorld}
          onRemove={onRemoveWorld}
          onDeletePermanent={onDeleteWorldPermanently}
          backups={removedBackups}
          onRestore={onRestoreBackup}
          onOpenFolder={() => onOpenFolder(instance.id, "saves")}
        />
      ) : null}

      {tab === "logs" ? (
        <section className="ui3-page">
          <div className="ui3-section-head">
            <h2>游戏日志</h2>
            <div className="ui3-row">
              <Button variant="quiet" size="sm" onClick={() => void loadLogs()}>
                刷新
              </Button>
              <Button variant="quiet" size="sm" onClick={onRefreshDiagnostics}>
                查看诊断
              </Button>
            </div>
          </div>
          {instanceCrashes.length ? (
            <section className="ui3-card" style={{ marginBottom: 14 }}>
              <div className="ui3-section-head">
                <h2>最近崩溃</h2>
                <Badge tone="danger">{instanceCrashes.length}</Badge>
              </div>
              {instanceCrashes.slice(0, 3).map((crash) => (
                <div className="recent-row" key={crash.id}>
                  <span className="recent-icon">
                    <Gamepad2 size={16} />
                  </span>
                  <div>
                    <strong>{crash.suspectedCause}</strong>
                    <small>{crash.suggestion}</small>
                  </div>
                  <time>{new Date(crash.occurredAt).toLocaleString("zh-CN")}</time>
                </div>
              ))}
            </section>
          ) : null}
          <div className="mod-rows" style={{ padding: 0, marginBottom: 14 }}>
            {logs.length ? (
              logs.map((log) => (
                <div key={log.fileName}>
                  <strong>{log.fileName}</strong>
                  <span>{Math.round(log.size / 1024)} KB</span>
                  <Button size="sm" onClick={() => void readLog(log.fileName)}>
                    读取
                  </Button>
                </div>
              ))
            ) : (
              <p className="ui3-muted">这个实例还没有游戏日志。</p>
            )}
          </div>
          {logText ? <pre className="log-preview">{logText.slice(-24000)}</pre> : null}
        </section>
      ) : null}

      {tab === "settings" ? (
        <section className="ui3-page">
          <div className="ui3-card" style={{ maxWidth: 620 }}>
            <div className="ui3-section-head">
              <h2>实例设置</h2>
            </div>
            <label className="ui3-input-label" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="ui3-secondary">运行内存</span>
              <div className="ui3-row">
                <select
                  value={MEMORY_PRESETS.includes(instance.memoryMb) ? String(instance.memoryMb) : "custom"}
                  onChange={(event) => {
                    if (event.target.value !== "custom") {
                      onMemoryChange(instance, Number(event.target.value));
                    }
                  }}
                >
                  {MEMORY_PRESETS.map((mb) => (
                    <option key={mb} value={mb}>
                      {mb / 1024} GB
                    </option>
                  ))}
                  <option value="custom">自定义</option>
                </select>
                <input
                  type="number"
                  min={2048}
                  max={65536}
                  step={512}
                  value={memory}
                  onChange={(event) => setMemory(event.target.value)}
                  onBlur={() => {
                    const value = Number(memory);
                    if (Number.isFinite(value)) {
                      onMemoryChange(instance, Math.max(2048, Math.min(65536, value)));
                    }
                  }}
                  aria-label="自定义内存 MB"
                />
                <span className="ui3-muted">MB</span>
              </div>
            </label>
            <p className="notice" style={{ marginTop: 12 }}>
              Java 与全局游戏设置可在“设置 → 游戏与 Java”中管理。
            </p>
            <Button variant="quiet" onClick={onOpenSettings}>
              打开全局设置
            </Button>
          </div>
        </section>
      ) : null}
    </div>
  );
}
