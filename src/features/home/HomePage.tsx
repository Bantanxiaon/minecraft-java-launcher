import {
  CircleAlert,
  Coffee,
  Download,
  FileArchive,
  Gamepad2,
  History,
  Play,
  Plus,
} from "lucide-react";
import { useState } from "react";
import type {
  DownloadJob,
  Instance,
  JavaRuntime,
  VersionSummary,
} from "../../types";
import { loaderLabel, loaderOptions } from "../../ui";
import { HomeUpdateCard } from "../../components/HomeUpdateCard";
import { Badge, Button, Progress } from "../../ui/components";
import type { Update } from "../../updater";
import shLogo from "../../assets/sh-logo.svg";

export type PlayHistoryEntry = {
  id: number;
  instanceId: number;
  instanceName: string;
  startedAt: string;
  endedAt?: string;
  exitCode?: number;
  usernameSnapshot?: string;
};

export type LoaderVersionRecord = {
  loaderKind: string;
  minecraftVersion: string;
  version: string;
  stable: boolean;
  recommended: boolean;
  latest: boolean;
  publishedAt?: string;
  source: string;
  fromCache: boolean;
  fetchedAt: string;
};

export type HomePageProps = {
  accountName?: string;
  selectedInstance?: Instance;
  instances: Instance[];
  onSelectInstance: (instanceId: number) => void;
  selectedJava?: JavaRuntime;
  gameRunning: boolean;
  busy: boolean;
  downloading: boolean;
  onLaunch: () => void;
  onTerminate: () => void;
  onOpenLibrary: () => void;
  onOpenInstance: (instanceId: number) => void;
  bootProblems: Array<{
    instanceName?: string;
    severity: "warn" | "error";
    text: string;
  }>;
  update?: Update | null;
  updateChecking: boolean;
  updateCheckError: boolean;
  onRetryUpdate: () => void;
  onOpenOnboarding: () => void;
  downloadJobs: DownloadJob[];
  downloadProgress: Record<number, number>;
  aggregateDownloadPercent?: number;
  onOpenDownloads: () => void;
  playHistory: PlayHistoryEntry[];
  versions: VersionSummary[];
  showInstanceForm: boolean;
  instanceName: string;
  gameVersion: string;
  instanceLoader: string;
  onInstanceName: (value: string) => void;
  onGameVersion: (value: string) => void;
  onInstanceLoader: (value: string) => void;
  onToggleInstanceForm: () => void;
  onCreateInstance: () => void;
  loaderBuilds: LoaderVersionRecord[];
  selectedLoaderBuild: string;
  buildsLoading: boolean;
  buildsCachedAt?: string;
  onLoaderBuildsChange: (value: string) => void;
  onOpenModpacks: () => void;
};

function formatRelativeTime(value?: string): string {
  if (!value) return "时间未知";
  const parsed = /^\d{10,}$/.test(value.trim())
    ? new Date(Number(value.trim()) * 1000)
    : new Date(value);
  if (Number.isNaN(parsed.getTime())) return "时间未知";
  const deltaMs = Date.now() - parsed.getTime();
  const minutes = Math.floor(deltaMs / 60000);
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days} 天前`;
  return parsed.toLocaleDateString("zh-CN");
}

function exitStatusText(entry: PlayHistoryEntry): string {
  if (entry.exitCode == null) return "上次游玩";
  return entry.exitCode === 0 ? "上次正常退出" : "上次异常退出";
}

export function HomePage({
  accountName,
  selectedInstance,
  instances,
  onSelectInstance,
  selectedJava,
  gameRunning,
  busy,
  onLaunch,
  onTerminate,
  onOpenLibrary,
  onOpenInstance,
  bootProblems,
  update,
  updateChecking,
  updateCheckError,
  onRetryUpdate,
  onOpenOnboarding,
  downloadJobs,
  downloadProgress,
  aggregateDownloadPercent,
  onOpenDownloads,
  playHistory,
  versions,
  showInstanceForm,
  instanceName,
  gameVersion,
  instanceLoader,
  onInstanceName,
  onGameVersion,
  onInstanceLoader,
  onToggleInstanceForm,
  onCreateInstance,
  loaderBuilds,
  selectedLoaderBuild,
  buildsLoading,
  buildsCachedAt,
  onLoaderBuildsChange,
  onOpenModpacks,
}: HomePageProps) {
  const [buildSearch, setBuildSearch] = useState("");
  const activeJobs = downloadJobs.filter((job) => job.status === "downloading");
  const filteredBuilds = buildSearch.trim()
    ? loaderBuilds.filter((build) => build.version.includes(buildSearch.trim()))
    : loaderBuilds;
  const launchDisabledReason = !selectedInstance
    ? "还没有游戏实例"
    : gameRunning
      ? "游戏正在运行"
      : selectedInstance.status !== "ready"
        ? "实例尚未完成安装"
        : busy
          ? "正在处理其他任务"
          : "";

  return (
    <div className="ui3-page ui3-page-enter">
      <header className="ui3-page-header">
        <div>
          <h1>{accountName ? `你好，${accountName}` : "欢迎使用 SH启动器"}</h1>
          <p>本地数据仅保存在此设备上。</p>
        </div>
        <div className="home-quick-actions">
          <Button variant="quiet" onClick={onOpenOnboarding}>
            开始游戏引导
          </Button>
          <Button variant="quiet" onClick={onOpenLibrary}>
            打开游戏库
          </Button>
        </div>
      </header>

      <section className="home-hero">
        <div className="home-hero-art">
          <img src={shLogo} alt="" />
        </div>
        <div className="home-hero-copy">
          <p className="home-eyebrow">当前实例</p>
          <h2>{selectedInstance?.name ?? "尚未安装游戏"}</h2>
          {instances.length > 1 ? (
            <select
              className="home-instance-select"
              aria-label="切换当前实例"
              value={selectedInstance?.id ?? ""}
              onChange={(event) => onSelectInstance(Number(event.target.value))}
            >
              {instances.map((instance) => (
                <option key={instance.id} value={instance.id}>
                  {instance.name} · {loaderLabel(instance.loaderType)}
                </option>
              ))}
            </select>
          ) : null}
          <div className="home-hero-facts">
            {selectedInstance ? (
              <span className="home-fact">
                <Gamepad2 size={14} />
                {loaderLabel(selectedInstance.loaderType)}{" "}
                {selectedInstance.gameVersion}
              </span>
            ) : (
              <span className="home-fact">选择版本后开始安装</span>
            )}
            <span className="home-fact">
              <Coffee size={14} />
              {selectedJava
                ? `Java ${selectedJava.majorVersion ?? selectedJava.version} · 64 位`
                : "未检测到兼容 Java"}
            </span>
            {selectedInstance ? (
              <span className="home-fact">
                {selectedInstance.status === "ready" ? "已就绪" : "待安装"}
              </span>
            ) : null}
          </div>
        </div>
        <div className="home-hero-actions">
          <Button
            variant="primary"
            size="lg"
            disabled={Boolean(launchDisabledReason)}
            onClick={onLaunch}
            title={launchDisabledReason}
          >
            <Play size={19} fill="currentColor" />
            {gameRunning ? "游戏运行中" : "开始游戏"}
          </Button>
          {gameRunning ? (
            <Button variant="danger" onClick={onTerminate}>
              强制结束游戏
            </Button>
          ) : null}
          {selectedInstance ? (
            <Button variant="quiet" onClick={() => onOpenInstance(selectedInstance.id)}>
              管理实例
            </Button>
          ) : null}
          {launchDisabledReason ? (
            <small className="ui3-muted">{launchDisabledReason}</small>
          ) : null}
        </div>
      </section>

      {bootProblems.length ? (
        <section className="boot-problems-card" role="alert">
          <div className="boot-problems-head">
            <CircleAlert size={17} />
            <strong>启动前发现问题</strong>
          </div>
          <ul>
            {bootProblems.slice(0, 6).map((problem, index) => (
              <li key={index} data-severity={problem.severity}>
                {problem.instanceName ? <b>{problem.instanceName}：</b> : null}
                {problem.text}
              </li>
            ))}
          </ul>
          <p>建议先处理以上问题再开始游戏，避免启动失败。</p>
        </section>
      ) : null}

      <HomeUpdateCard
        update={update}
        checking={updateChecking}
        checkError={updateCheckError}
        onRetry={onRetryUpdate}
      />

      <div className="home-grid">
        <section className="ui3-card">
          <div className="ui3-section-head">
            <h2>最近游戏</h2>
            <Button variant="quiet" size="sm" onClick={onOpenLibrary}>
              查看游戏库
            </Button>
          </div>
          {playHistory.length ? (
            playHistory.slice(0, 5).map((entry) => (
              <div className="recent-row" key={entry.id}>
                <span className="recent-icon">
                  <History size={16} />
                </span>
                <div>
                  <strong>{entry.instanceName}</strong>
                  <small>{exitStatusText(entry)}</small>
                </div>
                <time>{formatRelativeTime(entry.startedAt)}</time>
              </div>
            ))
          ) : (
            <p className="ui3-muted">
              还没有游戏记录。创建实例并启动后，最近活动会显示在这里。
            </p>
          )}
        </section>

        <div className="home-side-col">
          <section className="ui3-card">
            <div className="ui3-section-head">
              <h2>下载</h2>
              {downloadJobs.length ? (
                <Badge tone="info">{activeJobs.length} 下载中 · {downloadJobs.length - activeJobs.length} 其他</Badge>
              ) : null}
            </div>
            <div className="home-side-progress">
              <Progress
                value={aggregateDownloadPercent}
                indeterminate={
                  aggregateDownloadPercent === undefined && activeJobs.length > 0
                }
              />
              <div className="home-side-meta">
                <span>
                  {activeJobs.length
                    ? `${activeJobs.length} 个任务正在下载`
                    : downloadJobs.length
                      ? `最近完成 ${downloadJobs.length} 项`
                      : "暂无进行中的下载"}
                </span>
                <span>
                  {aggregateDownloadPercent !== undefined
                    ? `${aggregateDownloadPercent}%`
                    : selectedInstance
                      ? `${downloadProgress[selectedInstance.id] ?? 0}%`
                      : "—"}
                </span>
              </div>
            </div>
            {downloadJobs.length ? (
              <Button variant="quiet" size="sm" onClick={onOpenDownloads}>
                <Download size={14} />
                打开下载中心
              </Button>
            ) : null}
          </section>

          <section className="ui3-card">
            <div className="ui3-section-head">
              <h2>新建游戏配置</h2>
              {buildsCachedAt && !buildsLoading ? (
                <small className="ui3-muted">加载器元数据 · 缓存 {formatRelativeTime(buildsCachedAt)}</small>
              ) : null}
            </div>
            {showInstanceForm ? (
              <div className="home-instance-form">
                <label>
                  名称
                  <input
                    value={instanceName}
                    onChange={(event) => onInstanceName(event.target.value)}
                    placeholder="给这套游戏起个名字"
                  />
                </label>
                <label>
                  模组环境
                  <select
                    value={instanceLoader}
                    onChange={(event) => onInstanceLoader(event.target.value)}
                  >
                    {loaderOptions.map((loader) => (
                      <option key={loader} value={loader}>
                        {loaderLabel(loader)}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  Minecraft 版本
                  {versions.length ? (
                    <select
                      value={gameVersion}
                      onChange={(event) => onGameVersion(event.target.value)}
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
                      onChange={(event) => onGameVersion(event.target.value)}
                      placeholder={busy ? "正在读取官方版本…" : "Minecraft 版本"}
                    />
                  )}
                </label>
                {instanceLoader !== "vanilla" && gameVersion ? (
                  <label className="home-loader-build">
                    加载器版本
                    {buildsLoading ? (
                      <input disabled value="正在从官方元数据获取…" />
                    ) : loaderBuilds.length ? (
                      <>
                        <input
                          value={buildSearch}
                          onChange={(event) => setBuildSearch(event.target.value)}
                          placeholder="搜索全部版本，如 47.2"
                          aria-label="搜索加载器版本"
                        />
                        <select
                          value={selectedLoaderBuild || loaderBuilds[0]?.version || ""}
                          onChange={(event) => onLoaderBuildsChange(event.target.value)}
                          aria-label="加载器版本"
                        >
                          {filteredBuilds.filter((build) => build.recommended).map((build) => (
                            <option key={build.version} value={build.version}>
                              ★ 推荐 {build.version}
                            </option>
                          ))}
                          {filteredBuilds.filter((build) => build.latest && !build.recommended).map((build) => (
                            <option key={build.version} value={build.version}>
                              最新 {build.version}
                            </option>
                          ))}
                          {filteredBuilds.filter((build) => !build.recommended && !build.latest).map((build) => (
                            <option key={build.version} value={build.version}>
                              {build.version}
                            </option>
                          ))}
                          {filteredBuilds.length === 0 ? (
                            <option value="">未找到匹配版本</option>
                          ) : null}
                        </select>
                      </>
                    ) : (
                      <input disabled value="未找到可用版本" />
                    )}
                  </label>
                ) : null}
                <Button
                  variant="primary"
                  disabled={busy || !instanceName.trim() || !gameVersion}
                  onClick={onCreateInstance}
                >
                  创建
                </Button>
              </div>
            ) : (
              <div className="home-quick-actions">
                <button
                  type="button"
                  className="quick-action-tile"
                  onClick={onToggleInstanceForm}
                  disabled={busy}
                >
                  <Plus size={20} />
                  <span>
                    新建实例
                    <small>选择版本、加载器与加载器版本</small>
                  </span>
                </button>
                <button
                  type="button"
                  className="quick-action-tile"
                  onClick={onOpenModpacks}
                >
                  <FileArchive size={20} />
                  <span>
                    导入整合包
                    <small>Beta · 自动识别 Minecraft 与精确加载器版本</small>
                  </span>
                </button>
              </div>
            )}
            {!instances.length ? (
              <p className="ui3-muted">
                还没有实例。创建第一套游戏配置后，就可以开始游戏了。
              </p>
            ) : null}
          </section>
        </div>
      </div>
    </div>
  );
}
