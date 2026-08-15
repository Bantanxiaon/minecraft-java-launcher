import { useEffect, useMemo, useRef, useState } from "react";
import type {
  BackupItem,
  ContentItem,
  Instance,
  JavaRuntime,
  ModInspection,
  ModpackArchive,
  ModpackInspection,
  ModUpdateInfo,
  OnlineProject,
} from "../types";
import { inspectionSupportsGame, loaderLabel } from "../ui";

function archiveSizeText(size?: number): string {
  if (!size) return "";
  if (size >= 1024 ** 3) return `${(size / 1024 ** 3).toFixed(2)} GB`;
  if (size >= 1024 ** 2) return `${(size / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.round(size / 1024)} KB`;
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

const CATEGORY_ZH: Record<string, string> = {
  adventure: "冒险",
  magic: "魔法",
  technology: "科技",
  storage: "存储",
  worldgen: "地形生成",
  world: "世界",
  food: "食物",
  combat: "战斗",
  decoration: "装饰",
  building: "建筑",
  redstone: "红石",
  farming: "农业",
  library: "前置库",
  utility: "实用",
  performance: "性能优化",
  optimization: "性能优化",
  social: "社交",
  server: "服务端",
  equipment: "装备",
  armor: "盔甲",
  weapons: "武器",
  tools: "工具",
  biomes: "生物群系",
  mobs: "生物",
  bosses: "Boss",
  dimensions: "维度",
  exploration: "探索",
  minigame: "小游戏",
  map: "地图",
  modpack: "整合包",
  "mc-mods": "模组",
  "modpacks": "整合包",
};

function categoryText(value: string): string {
  return CATEGORY_ZH[value.toLowerCase()] ?? value;
}

function OnlineCatalog({
  title, query, onQuery, onSearch, projects, busy, disabled, onInstall,
  loaderOptions, selectedLoader, onLoader, versionValue, onVersion,
  onTranslate,
}: {
  title: string;
  query: string;
  onQuery: (value: string) => void;
  onSearch: () => void;
  projects: OnlineProject[];
  busy: boolean;
  disabled?: boolean;
  onInstall: (project: OnlineProject) => void;
  loaderOptions?: string[];
  selectedLoader?: string;
  onLoader?: (value: string) => void;
  versionValue?: string;
  onVersion?: (value: string) => void;
  onTranslate?: (text: string) => Promise<string | undefined>;
}) {
  const [translations, setTranslations] = useState<Record<string, string>>({});
  const onTranslateRef = useRef(onTranslate);
  onTranslateRef.current = onTranslate;

  useEffect(() => {
    if (!onTranslateRef.current) return;
    let cancelled = false;
    const queue: Array<{ text: string; keys: string[] }> = [];
    const byText = new Map<string, string[]>();
    const hasCjk = (value: string) => /[\u4e00-\u9fff]/.test(value);
    for (const project of projects) {
      if (queue.length >= 18) break;
      const titleKey = `${project.source}:${project.projectId}:title`;
      const descKey = `${project.source}:${project.projectId}:desc`;
      if (!project.titleZh && !hasCjk(project.title) && project.title) {
        byText.set(project.title, [...(byText.get(project.title) ?? []), titleKey]);
      }
      if (
        !project.descriptionZh &&
        !hasCjk(project.description) &&
        project.description
      ) {
        const text = project.description.slice(0, 300);
        byText.set(text, [...(byText.get(text) ?? []), descKey]);
      }
    }
    for (const [text, keys] of byText) {
      if (queue.length >= 18) break;
      queue.push({ text, keys });
    }
    let index = 0;
    const workers = Array.from({ length: 3 }, async () => {
      while (index < queue.length && !cancelled) {
        const item = queue[index];
        index += 1;
        const translated = await onTranslateRef.current?.(item.text);
        if (translated && !cancelled) {
          setTranslations((existing) => {
            const next = { ...existing };
            for (const key of item.keys) next[key] = translated;
            return next;
          });
        }
      }
    });
    void Promise.all(workers);
    return () => {
      cancelled = true;
    };
  }, [projects]);

  return (
    <section className="online-catalog">
      <div className="section-heading">
        <div>
          <h2>{title}</h2>
          <p>同时搜索 Modrinth 与 CurseForge，结果自动翻译为中文；自动筛掉不适合当前游戏版本和模组环境的内容。</p>
        </div>
        <span>联网</span>
      </div>
      <div className="catalog-search">
        <input value={query} maxLength={80} placeholder="搜索名称、作者或关键词"
          onChange={(event) => onQuery(event.target.value)}
          onKeyDown={(event) => { if (event.key === "Enter" && !busy && !disabled) onSearch(); }} />
        <button className="primary" disabled={busy || disabled} onClick={onSearch}>
          {busy ? "处理中…" : "搜索"}
        </button>
      </div>
      {loaderOptions && onLoader && onVersion ? (
        <div className="catalog-filters">
          <label>
            <span>模组环境</span>
            <select
              value={selectedLoader ?? ""}
              onChange={(event) => onLoader(event.target.value)}
            >
              <option value="">跟随当前游戏配置</option>
              {loaderOptions.map((loader) => (
                <option key={loader} value={loader}>
                  {loaderLabel(loader)}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>游戏版本</span>
            <input
              value={versionValue ?? ""}
              maxLength={24}
              placeholder="跟随当前游戏配置"
              onChange={(event) => onVersion(event.target.value)}
            />
          </label>
        </div>
      ) : null}
      {disabled ? <p className="catalog-hint">请先选择一套已经启用 Fabric、Forge、NeoForge 或 Quilt 的游戏配置。</p> : null}
      {projects.length ? <div className="catalog-grid">
        {projects.map((project) => {
          const titleKey = `${project.source}:${project.projectId}:title`;
          const descKey = `${project.source}:${project.projectId}:desc`;
          const title = translations[titleKey] ?? project.titleZh ?? project.title;
          const description = translations[descKey] ?? project.descriptionZh ?? project.description;
          return (
            <article key={`${project.source}-${project.projectId}`}>
              <div className="catalog-mark">{project.title.slice(0, 1).toUpperCase()}</div>
              <div className="catalog-copy">
                <strong>{title}</strong>
                <small>
                  <em className="catalog-source">{project.source === "curseforge" ? "CurseForge" : "Modrinth"}</em>
                  {" "}作者 {project.author} · {project.downloads.toLocaleString("zh-CN")} 次下载
                </small>
                {title !== project.title ? (
                  <small className="catalog-original">原文：{project.title}</small>
                ) : null}
                <p>{description}</p>
                <span>{project.categories.slice(0, 3).map(categoryText).join(" · ") || "Minecraft"}</span>
              </div>
              <button disabled={busy} onClick={() => onInstall(project)}>下载并安装</button>
            </article>
          );
        })}
      </div> : null}
    </section>
  );
}

function BackupSection({ items, busy, onRestore }: {
  items: BackupItem[];
  busy: boolean;
  onRestore: (item: BackupItem) => void;
}) {
  if (!items.length) return null;
  return (
    <section className="backup-section">
      <div className="section-heading">
        <div><h2>可恢复备份</h2><p>移除时保留下来的文件，可一键放回原来的游戏配置。</p></div>
        <span>{items.length} 个</span>
      </div>
      <div className="backup-list">
        {items.map((item) => (
          <div key={item.backupName}>
            <div><strong>{item.originalName}</strong><small>{(item.size / 1024 / 1024).toFixed(1)} MB</small></div>
            <button disabled={busy} onClick={() => onRestore(item)}>恢复</button>
          </div>
        ))}
      </div>
    </section>
  );
}

type ModsPageProps = {
  instances: Instance[];
  selectedId?: number;
  onSelect: (id: number) => void;
  items: ContentItem[];
  inspection?: ModInspection;
  busy: boolean;
  message: string;
  onPick: () => void;
  onInstall: () => void;
  onToggle: (item: ContentItem) => void;
  onRemove: (item: ContentItem) => void;
  queuedCount: number;
  dragging: boolean;
  onlineQuery: string;
  onlineProjects: OnlineProject[];
  onOnlineQuery: (value: string) => void;
  onOnlineSearch: () => void;
  onOnlineInstall: (project: OnlineProject) => void;
  onTranslate: (text: string) => Promise<string | undefined>;
  onInstallCurseforgeUrl: (url: string) => void;
  onlineLoader?: string;
  onlineVersion?: string;
  onOnlineLoader?: (value: string) => void;
  onOnlineVersion?: (value: string) => void;
  problemMods?: Record<string, string>;
  updates: ModUpdateInfo[];
  onCheckUpdates: () => void;
  onUpdate: (item: ContentItem) => void;
  onUpdateAll: () => void;
  backups: BackupItem[];
  onRestore: (item: BackupItem) => void;
  onOpenFolder: () => void;
};

export function ModsPage({
  instances,
  selectedId,
  onSelect,
  items,
  inspection,
  busy,
  message,
  onPick,
  onInstall,
  onToggle,
  onRemove,
  queuedCount,
  dragging,
  onlineQuery,
  onlineProjects,
  onOnlineQuery,
  onOnlineSearch,
  onOnlineInstall,
  onTranslate,
  onInstallCurseforgeUrl,
  onlineLoader,
  onlineVersion,
  onOnlineLoader,
  onOnlineVersion,
  problemMods,
  updates,
  onCheckUpdates,
  onUpdate,
  onUpdateAll,
  backups,
  onRestore,
  onOpenFolder,
}: ModsPageProps) {
  const selected = instances.find((instance) => instance.id === selectedId);
  const compatible = Boolean(
    inspection &&
    selected &&
    inspection.loaderType === selected.loaderType &&
    inspectionSupportsGame(
      inspection.gameVersionRequirements,
      selected.gameVersion,
    ) &&
    selected.loaderType !== "vanilla",
  );
  const [modSearch, setModSearch] = useState("");
  const [curseforgeUrl, setCurseforgeUrl] = useState("");
  const [modFilter, setModFilter] = useState<"all" | "enabled" | "disabled">(
    "all",
  );
  const visibleItems = useMemo(() => {
    const keyword = modSearch.trim().toLowerCase();
    return items.filter((item) => {
      if (modFilter === "enabled" && !item.enabled) return false;
      if (modFilter === "disabled" && item.enabled) return false;
      if (!keyword) return true;
      let metadata: Partial<ModInspection> = {};
      try {
        metadata = item.metadataJson ? JSON.parse(item.metadataJson) : {};
      } catch {
        // 文件名兜底
      }
      return (
        item.fileName.toLowerCase().includes(keyword) ||
        (metadata.name ?? "").toLowerCase().includes(keyword) ||
        (metadata.modId ?? "").toLowerCase().includes(keyword)
      );
    });
  }, [items, modSearch, modFilter]);
  return (
    <>
      <header>
        <div>
          <h1>模组</h1>
          <p>每套游戏配置单独管理模组，可以安装、暂时关闭或移到备份区。</p>
        </div>
        <span className="ready-label">安全管理</span>
        <button className="quiet" disabled={!selectedId} onClick={onOpenFolder}>打开模组文件夹</button>
      </header>
      <section className={`mod-window ${dragging ? "dragging" : ""}`}>
        <div className="mod-toolbar">
          <div>
            <label htmlFor="mod-instance">安装到哪套游戏</label>
            <select
              id="mod-instance"
              value={selectedId ?? ""}
              onChange={(event) => onSelect(Number(event.target.value))}
            >
              <option value="" disabled>
                选择游戏配置
              </option>
              {instances.map((instance) => (
                <option value={instance.id} key={instance.id}>
                  {instance.name} · {loaderLabel(instance.loaderType)}{" "}
                  {instance.gameVersion}
                </option>
              ))}
            </select>
          </div>
          <button disabled={busy} onClick={onPick}>
            {busy ? "处理中…" : "选择本地模组文件"}
          </button>
        </div>
        <div className="loader-list">
          {["Fabric", "Forge", "NeoForge", "Quilt", "Vanilla"].map((loader) => (
            <div key={loader}>
              <strong>{loader}</strong>
              <small>
                {loader === "Vanilla" ? "纯原版，不装模组" : "检查模组说明文件"}
              </small>
            </div>
          ))}
        </div>
        {inspection ? (
          <div className="mod-result">
            <div>
              <strong>{inspection.name ?? inspection.fileName}</strong>
              <span>
                {loaderLabel(inspection.loaderType)} ·{" "}
                {inspection.version ?? "未知版本"} ·{" "}
                {(inspection.fileSize / 1024 / 1024).toFixed(1)} MB
              </span>
              <small>文件完整性编号：{inspection.sha256.slice(0, 20)}…</small>
            </div>
            <button
              className="primary"
              disabled={busy || !compatible}
              onClick={onInstall}
            >
              {queuedCount > 1 ? `安装 ${queuedCount} 个模组` : "安装到这套游戏"}
            </button>
            {!compatible ? (
              <p>
                {selected
                  ? selected.loaderType === "vanilla"
                    ? "纯原版游戏不能安装这类模组。"
                    : inspection.loaderType !== selected.loaderType
                      ? `这个模组需要 ${loaderLabel(inspection.loaderType)}，当前选择的是 ${loaderLabel(selected.loaderType)}。`
                      : `这个模组只支持 ${inspection.gameVersionRequirements.join(" 或 ")}，当前游戏是 ${selected.gameVersion}。`
                  : "请先选择要安装到哪套游戏。"}
              </p>
            ) : null}
            {inspection.warnings.map((warning) => (
              <p key={warning}>{warning}</p>
            ))}
            {inspection.dependencies.length ? (
              <p>声明依赖：{inspection.dependencies.join(", ")}</p>
            ) : null}
            {inspection.gameVersionRequirements.length ? (
              <p>
                支持的游戏版本：{inspection.gameVersionRequirements.join(" 或 ")}
              </p>
            ) : null}
            {inspection.conflicts.length ? (
              <p>声明冲突：{inspection.conflicts.join(", ")}</p>
            ) : null}
          </div>
        ) : null}
        {message ? (
          <p className="mod-message" role="status">
            {message}
          </p>
        ) : null}
      </section>
      <OnlineCatalog title="在线搜索模组" query={onlineQuery} onQuery={onOnlineQuery}
        onSearch={onOnlineSearch} projects={onlineProjects} busy={busy}
        disabled={!selected || selected.loaderType === "vanilla"} onInstall={onOnlineInstall}
        onTranslate={onTranslate}
        loaderOptions={["fabric", "quilt", "forge", "neoforge"]}
        selectedLoader={onlineLoader}
        onLoader={onOnlineLoader}
        versionValue={onlineVersion}
        onVersion={onOnlineVersion} />
      <div className="curse-url-row">
        <input
          value={curseforgeUrl}
          maxLength={300}
          placeholder="或粘贴 CurseForge 项目/文件链接（需先导入过包含该模组的整合包）"
          onChange={(event) => setCurseforgeUrl(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && curseforgeUrl.trim() && selected && selected.loaderType !== "vanilla") {
              onInstallCurseforgeUrl(curseforgeUrl.trim());
            }
          }}
        />
        <button
          disabled={busy || !curseforgeUrl.trim() || !selected || selected.loaderType === "vanilla"}
          onClick={() => onInstallCurseforgeUrl(curseforgeUrl.trim())}
        >
          从网址安装
        </button>
      </div>
      <section className="installed-mods">
        <div className="section-heading">
          <div>
            <h2>已管理的模组</h2>
            <p>
              {selected
                ? `${selected.name} · ${loaderLabel(selected.loaderType)} ${selected.gameVersion}`
                : "选择游戏配置后查看"}
            </p>
          </div>
          <div className="section-actions">
            <span>{items.length} 个</span>
            <button disabled={busy || !selected} onClick={onCheckUpdates}>检查更新</button>
            <button className="primary" disabled={busy || !updates.some((item) => item.updateAvailable)} onClick={onUpdateAll}>全部更新</button>
          </div>
        </div>
        <div className="installed-mod-filters">
          <input
            value={modSearch}
            maxLength={80}
            placeholder="搜索已安装的模组（名称 / 文件名 / ID）"
            onChange={(event) => setModSearch(event.target.value)}
          />
          <select
            aria-label="模组状态筛选"
            value={modFilter}
            onChange={(event) =>
              setModFilter(event.target.value as "all" | "enabled" | "disabled")
            }
          >
            <option value="all">全部状态</option>
            <option value="enabled">已启用</option>
            <option value="disabled">已停用</option>
          </select>
          <span className="filter-count">
            {visibleItems.length} / {items.length} 个
          </span>
        </div>
        {visibleItems.length ? (
          <div className="mod-rows">
            {visibleItems.map((item) => {
              let metadata: Partial<ModInspection> = {};
              try {
                metadata = item.metadataJson
                  ? JSON.parse(item.metadataJson)
                  : {};
              } catch {
                /* keep filename fallback */
              }
              const update = updates.find((candidate) => candidate.contentId === item.id);
              const problem = problemMods?.[item.fileName];
              return (
                <div key={item.id} className={problem ? "mod-problem" : undefined}>
                  <div className="mod-state">{item.enabled ? "ON" : "OFF"}</div>
                  <div>
                    <strong>{metadata.name ?? item.fileName}</strong>
                    <small>
                      {metadata.version ?? "未知版本"} · {item.fileName}
                    </small>
                    {problem ? (
                      <em className="mod-problem-badge" title={problem}>
                        有问题
                      </em>
                    ) : null}
                  </div>
                  <span className={item.enabled ? "enabled" : "disabled"}>
                    {update?.updateAvailable
                      ? `可更新到 ${update.latestVersion}`
                      : item.enabled ? "已启用" : "已停用"}
                  </span>
                  {update?.updateAvailable ? (
                    <button className="primary" disabled={busy} onClick={() => onUpdate(item)}>更新</button>
                  ) : null}
                  <button disabled={busy} onClick={() => onToggle(item)}>
                    {item.enabled ? "停用" : "启用"}
                  </button>
                  <button
                    className="danger"
                    disabled={busy}
                    onClick={() => onRemove(item)}
                  >
                    移除
                  </button>
                  {problem ? (
                    <small className="mod-problem-reason">{problem}</small>
                  ) : null}
                </div>
              );
            })}
          </div>
        ) : (
          <div className="empty-mods">
            {selected ? "这套游戏还没有安装模组。" : "请选择一套游戏配置。"}
          </div>
        )}
      </section>
      <BackupSection items={backups} busy={busy} onRestore={onRestore} />
    </>
  );
}

export function ComingSoonPage({ title }: { title: string }) {
  return (
    <>
      <header>
        <div>
          <h1>{title}</h1>
          <p>此模块已进入工程计划。</p>
        </div>
        <span className="paused-label">开发中</span>
      </header>
      <section className="server-window">
        <h2>功能暂未开放</h2>
        <p>完成真实后端能力与测试后才会启用入口。</p>
      </section>
    </>
  );
}

export function ModpacksPage({
  inspection,
  busy,
  message,
  dragging,
  onPick,
  onImport,
  instances,
  onlineQuery,
  onlineProjects,
  onOnlineQuery,
  onOnlineSearch,
  onOnlineInstall,
  onTranslate,
  onExport,
  archives,
  javaRuntimes,
  onImportArchive,
  onRemoveArchive,
  onInstallJava,
}: {
  inspection?: ModpackInspection;
  busy: boolean;
  message: string;
  dragging: boolean;
  onPick: () => void;
  onImport: (gameVersion?: string, loaderType?: string) => void;
  instances: Instance[];
  targetId?: number;
  onTarget: (id: number) => void;
  onlineQuery: string;
  onlineProjects: OnlineProject[];
  onOnlineQuery: (value: string) => void;
  onOnlineSearch: () => void;
  onOnlineInstall: (project: OnlineProject) => void;
  onTranslate: (text: string) => Promise<string | undefined>;
  onExport: (instanceId: number, includeSaves: boolean) => void;
  archives: ModpackArchive[];
  javaRuntimes: JavaRuntime[];
  onImportArchive: (archive: ModpackArchive) => void;
  onRemoveArchive: (archive: ModpackArchive) => void;
  onInstallJava: (major: number) => void;
}) {
  const [exportInstanceId, setExportInstanceId] = useState<number>();
  const [includeSaves, setIncludeSaves] = useState(false);
  const [genericVersion, setGenericVersion] = useState("");
  const [genericLoader, setGenericLoader] = useState("forge");
  return (
    <>
      <header>
        <div>
          <h1>整合包</h1>
          <p>可以在线下载，也可以选择或拖入电脑里的整合包。</p>
        </div>
        <span className="ready-label">本地导入</span>
      </header>
      <section className={`pack-dropzone ${dragging ? "dragging" : ""}`}>
        <div className="server-symbol">⇩</div>
        <h2>把整合包文件拖到这里</h2>
        <p>
          支持 Modrinth（.mrpack）、CurseForge 和普通压缩包（.zip）。
          导入前会检查文件结构和大小，防止异常压缩包损坏电脑中的文件。
        </p>
        <button disabled={busy} onClick={onPick}>
          {busy ? "正在检查…" : "选择整合包文件"}
        </button>
      </section>
      <OnlineCatalog title="在线搜索整合包" query={onlineQuery} onQuery={onOnlineQuery}
        onSearch={onOnlineSearch} projects={onlineProjects} busy={busy}
        onInstall={onOnlineInstall} onTranslate={onTranslate} />
      <section className="pack-export-card">
        <div>
          <h2>导出自己的整合包</h2>
          <p>包含模组、设置、资源包、光影、游戏选项和服务器列表；不会包含账户、登录令牌或启动器数据库。</p>
        </div>
        <select value={exportInstanceId ?? ""} onChange={(event) => setExportInstanceId(Number(event.target.value))}>
          <option value="" disabled>选择要导出的游戏配置</option>
          {instances.map((instance) => <option value={instance.id} key={instance.id}>{instance.name} · {instance.gameVersion}</option>)}
        </select>
        <label><input type="checkbox" checked={includeSaves} onChange={(event) => setIncludeSaves(event.target.checked)} /> 同时包含存档（默认不包含）</label>
        <button className="primary" disabled={busy || !exportInstanceId} onClick={() => exportInstanceId && onExport(exportInstanceId, includeSaves)}>导出 ZIP</button>
      </section>
      <section className="installed-mods">
        <div className="section-heading">
          <div>
            <h2>已下载整合包</h2>
            <p>
              导入过的整合包都会记录在这里；每个整合包对应一套独立实例，游戏版本与 Java 自动匹配。
            </p>
          </div>
          <div className="section-actions">
            <span>{archives.length} 个</span>
          </div>
        </div>
        {archives.length ? (
          <div className="mod-rows">
            {archives.map((archive) => {
              const requiredJava = javaMajorForGameVersion(archive.gameVersion);
              const javaInstalled = requiredJava
                ? javaRuntimes.some(
                    (runtime) =>
                      runtime.is64Bit && runtime.majorVersion === requiredJava,
                  )
                : true;
              return (
                <div className="pack-archive-row" key={archive.id}>
                  <div>
                    <strong>{archive.name ?? archive.fileName}</strong>
                    <small>
                      {archive.format.toUpperCase()} ·{" "}
                      {archive.gameVersion
                        ? `Minecraft ${archive.gameVersion}`
                        : "版本未知"}{" "}
                      ·{" "}
                      {archive.loaderType
                        ? loaderLabel(archive.loaderType)
                        : "加载器未知"}
                      {archiveSizeText(archive.sizeBytes)
                        ? ` · ${archiveSizeText(archive.sizeBytes)}`
                        : ""}
                    </small>
                    {archive.version ? (
                      <small>包版本：{archive.version}</small>
                    ) : null}
                    {archive.instanceName ? (
                      <small>
                        对应实例：{archive.instanceName}
                        {archive.instanceStatus === "ready"
                          ? "（已就绪）"
                          : "（待安装）"}
                      </small>
                    ) : (
                      <small>对应实例：尚未创建</small>
                    )}
                  </div>
                  <span className={requiredJava && javaInstalled ? "enabled" : "disabled"}>
                    {requiredJava
                      ? javaInstalled
                        ? `已装 Java ${requiredJava}`
                        : `需要 Java ${requiredJava}`
                      : "Java 自动匹配"}
                  </span>
                  {requiredJava && !javaInstalled ? (
                    <button onClick={() => onInstallJava(requiredJava)}>
                      安装 Java {requiredJava}
                    </button>
                  ) : null}
                  <button
                    className="primary"
                    disabled={busy}
                    onClick={() => onImportArchive(archive)}
                  >
                    导入为独立实例
                  </button>
                  <button
                    className="danger"
                    disabled={busy}
                    onClick={() => onRemoveArchive(archive)}
                  >
                    移除记录
                  </button>
                </div>
              );
            })}
          </div>
        ) : (
          <p className="mod-message">
            还没有导入过整合包；导入后这里会显示游戏版本、加载器、Java 要求和对应实例。
          </p>
        )}
      </section>
      {inspection ? (
        <section className="pack-preview">
          <div>
            <span>{inspection.format.toUpperCase()}</span>
            <h2>{inspection.name ?? inspection.fileName}</h2>
            <p>
              {inspection.gameVersion
                ? `Minecraft ${inspection.gameVersion}`
                : "未声明 Minecraft 版本"}{" "}
              ·{" "}
              {inspection.loaderType
                ? loaderLabel(inspection.loaderType)
                : "模组运行环境待确认"}
            </p>
          </div>
          <dl>
            <div>
              <dt>模组文件</dt>
              <dd>{inspection.modCount}</dd>
            </div>
            <div>
              <dt>覆盖文件</dt>
              <dd>{inspection.overrideCount}</dd>
            </div>
            <div>
              <dt>包版本</dt>
              <dd>{inspection.version ?? "未知"}</dd>
            </div>
          </dl>
          {inspection.warnings.map((warning) => (
            <p className="pack-warning" key={warning}>
              {warning}
            </p>
          ))}
          {inspection.format === "generic" ? (
            <div className="pack-generic-form">
              <input
                value={genericVersion}
                onChange={(event) => setGenericVersion(event.target.value)}
                placeholder="Minecraft 版本，如 1.20.1"
              />
              <select
                value={genericLoader}
                onChange={(event) => setGenericLoader(event.target.value)}
                aria-label="模组运行环境"
              >
                {["vanilla", "forge", "fabric", "neoforge", "quilt"].map((loader) => (
                  <option key={loader} value={loader}>
                    {loaderLabel(loader)}
                  </option>
                ))}
              </select>
              <button
                className="primary pack-import"
                disabled={busy || !genericVersion.trim()}
                onClick={() => onImport(genericVersion.trim(), genericLoader)}
              >
                创建独立实例并导入
              </button>
              <small className="pack-warning">
                这个压缩包没有标准清单，需要你确认版本和加载器；导入后会自动安装游戏、Java 与加载器。
              </small>
            </div>
          ) : (
            <button
              className="primary pack-import"
              disabled={busy}
              onClick={() => onImport()}
            >
              创建独立实例并导入
            </button>
          )}
        </section>
      ) : null}
      {message ? (
        <p className="mod-message" role="status">
          {message}
        </p>
      ) : null}
    </>
  );
}

export function ArchiveContentPage({
  title,
  kind,
  instances,
  targetId,
  items,
  busy,
  message,
  dragging,
  onTarget,
  onImport,
  onToggle,
  onRemove,
  backups,
  onRestore,
  onOpenFolder,
}: {
  title: string;
  kind: string;
  instances: Instance[];
  targetId?: number;
  items: ContentItem[];
  busy: boolean;
  message: string;
  dragging: boolean;
  onTarget: (id: number) => void;
  onImport: () => void;
  onToggle: (item: ContentItem) => void;
  onRemove: (item: ContentItem) => void;
  backups: BackupItem[];
  onRestore: (item: BackupItem) => void;
  onOpenFolder: () => void;
}) {
  return (
    <>
      <header>
        <div>
          <h1>{title}</h1>
          <p>支持一次选择多个压缩包或直接拖入；每套游戏分开保存，移除后仍可从备份恢复。</p>
        </div>
        <div className="header-actions"><button className="quiet" disabled={!targetId} onClick={onOpenFolder}>打开文件夹</button><span className="ready-label">外部导入</span></div>
      </header>
      <section className={`archive-toolbar ${dragging ? "dragging" : ""}`}>
        <select
          value={targetId ?? ""}
          onChange={(event) => onTarget(Number(event.target.value))}
        >
          <option value="" disabled>
            选择游戏配置
          </option>
          {instances.map((instance) => (
            <option key={instance.id} value={instance.id}>
              {instance.name} · {instance.gameVersion}
            </option>
          ))}
        </select>
        <div>
          <strong>拖入{title}压缩包</strong>
          <small>会检查文件结构、大小和完整性</small>
        </div>
        <button disabled={busy || !targetId} onClick={onImport}>
          {busy ? "导入中…" : `选择${title}`}
        </button>
      </section>
      <section className="installed-mods">
        <div className="section-heading">
          <div>
            <h2>已管理的{title}</h2>
            <p>
              {kind === "resourcepack"
                ? "这里管理可用文件；游戏内是否选中仍由 Minecraft 配置决定。"
                : "启用后由兼容的光影模组在游戏内选择。"}
            </p>
          </div>
          <span>{items.length} 个</span>
        </div>
        {items.length ? (
          <div className="mod-rows">
            {items.map((item) => (
              <div key={item.id}>
                <div className="mod-state">ZIP</div>
                <div>
                  <strong>{item.fileName}</strong>
                  <small>文件完整性编号：{item.hash.slice(0, 16)}…</small>
                </div>
                <span className={item.enabled ? "enabled" : "disabled"}>
                  {item.enabled ? "可用" : "已停用"}
                </span>
                <button disabled={busy} onClick={() => onToggle(item)}>
                  {item.enabled ? "停用" : "启用"}
                </button>
                <button
                  className="danger"
                  disabled={busy}
                  onClick={() => onRemove(item)}
                >
                  移除
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-mods">尚未导入。</div>
        )}
      </section>
      <BackupSection items={backups} busy={busy} onRestore={onRestore} />
      {message ? (
        <p className="mod-message" role="status">
          {message}
        </p>
      ) : null}
    </>
  );
}

export function WorldsPage({
  instances,
  targetId,
  items,
  busy,
  message,
  dragging,
  onTarget,
  onFolder,
  onZip,
  onBackup,
  onDuplicate,
  onExport,
  onRemove,
  onDeletePermanent,
  backups,
  onRestore,
  onOpenFolder,
}: {
  instances: Instance[];
  targetId?: number;
  items: ContentItem[];
  busy: boolean;
  message: string;
  dragging: boolean;
  onTarget: (id: number) => void;
  onFolder: () => void;
  onZip: () => void;
  onBackup: (item: ContentItem) => void;
  onDuplicate: (item: ContentItem) => void;
  onExport: (item: ContentItem) => void;
  onRemove: (item: ContentItem) => void;
  onDeletePermanent: (item: ContentItem) => void;
  backups: BackupItem[];
  onRestore: (item: BackupItem) => void;
  onOpenFolder: () => void;
}) {
  return (
    <>
      <header>
        <div>
          <h1>存档</h1>
          <p>支持存档文件夹或压缩包，也可以直接拖入；启动器会自动寻找真正的存档目录。</p>
        </div>
        <div className="header-actions"><button className="quiet" disabled={!targetId} onClick={onOpenFolder}>打开存档文件夹</button><span className="ready-label">安全导入</span></div>
      </header>
      <section className={`archive-toolbar ${dragging ? "dragging" : ""}`}>
        <select
          value={targetId ?? ""}
          onChange={(event) => onTarget(Number(event.target.value))}
        >
          <option value="" disabled>
            选择游戏配置
          </option>
          {instances.map((instance) => (
            <option key={instance.id} value={instance.id}>
              {instance.name} · {instance.gameVersion}
            </option>
          ))}
        </select>
        <div>
          <strong>拖入存档文件夹或压缩包</strong>
          <small>遇到同名存档会自动改名，异常路径会被拦截</small>
        </div>
        <div className="world-actions">
          <button disabled={busy || !targetId} onClick={onFolder}>
            选择文件夹
          </button>
          <button disabled={busy || !targetId} onClick={onZip}>
            选择压缩包
          </button>
        </div>
      </section>
      <section className="installed-mods">
        <div className="section-heading">
          <div>
            <h2>已管理的存档</h2>
            <p>“移除”会转移到备份区；“彻底删除”不可恢复。</p>
          </div>
          <span>{items.length} 个</span>
        </div>
        {items.length ? (
          <div className="mod-rows">
            {items.map((item) => (
              <div key={item.id}>
                <div className="mod-state">WORLD</div>
                <div>
                  <strong>{item.fileName}</strong>
                  <small>level.dat {item.hash.slice(0, 16)}…</small>
                </div>
                <span className="enabled">可游玩</span>
                <button disabled={busy} onClick={() => onBackup(item)}>备份</button>
                <button disabled={busy} onClick={() => onDuplicate(item)}>复制</button>
                <button disabled={busy} onClick={() => onExport(item)}>导出</button>
                <button
                  className="danger"
                  disabled={busy}
                  onClick={() => onRemove(item)}
                >
                  移除
                </button>
                <button
                  className="danger"
                  disabled={busy}
                  title="永久删除，不可恢复"
                  onClick={() => onDeletePermanent(item)}
                >
                  彻底删除
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-mods">尚未导入存档。</div>
        )}
      </section>
      <BackupSection items={backups} busy={busy} onRestore={onRestore} />
      {message ? (
        <p className="mod-message" role="status">
          {message}
        </p>
      ) : null}
    </>
  );
}
