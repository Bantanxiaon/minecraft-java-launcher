import { useState } from "react";
import type { BackupItem, ContentItem, Instance, ModInspection, ModpackInspection, ModUpdateInfo, OnlineProject } from "../types";
import { inspectionSupportsGame, loaderLabel } from "../ui";

function OnlineCatalog({
  title, query, onQuery, onSearch, projects, busy, disabled, onInstall,
}: {
  title: string;
  query: string;
  onQuery: (value: string) => void;
  onSearch: () => void;
  projects: OnlineProject[];
  busy: boolean;
  disabled?: boolean;
  onInstall: (project: OnlineProject) => void;
}) {
  return (
    <section className="online-catalog">
      <div className="section-heading">
        <div>
          <h2>{title}</h2>
          <p>来自模组下载平台 Modrinth。启动器会自动筛掉不适合当前游戏版本和模组环境的内容，并检查下载文件是否完整。</p>
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
      {disabled ? <p className="catalog-hint">请先选择一套已经启用 Fabric、Forge、NeoForge 或 Quilt 的游戏配置。</p> : null}
      {projects.length ? <div className="catalog-grid">
        {projects.map((project) => <article key={project.projectId}>
          <div className="catalog-mark">{project.title.slice(0, 1).toUpperCase()}</div>
          <div className="catalog-copy">
            <strong>{project.title}</strong>
            <small>作者 {project.author} · {project.downloads.toLocaleString("zh-CN")} 次下载</small>
            <p>{project.description}</p>
            <span>{project.categories.slice(0, 3).join(" · ") || "Minecraft"}</span>
          </div>
          <button disabled={busy} onClick={() => onInstall(project)}>下载并安装</button>
        </article>)}
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
        disabled={!selected || selected.loaderType === "vanilla"} onInstall={onOnlineInstall} />
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
        {items.length ? (
          <div className="mod-rows">
            {items.map((item) => {
              let metadata: Partial<ModInspection> = {};
              try {
                metadata = item.metadataJson
                  ? JSON.parse(item.metadataJson)
                  : {};
              } catch {
                /* keep filename fallback */
              }
              const update = updates.find((candidate) => candidate.contentId === item.id);
              return (
                <div key={item.id}>
                  <div className="mod-state">{item.enabled ? "ON" : "OFF"}</div>
                  <div>
                    <strong>{metadata.name ?? item.fileName}</strong>
                    <small>
                      {metadata.version ?? "未知版本"} · {item.fileName}
                    </small>
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
  targetId,
  onTarget,
  onlineQuery,
  onlineProjects,
  onOnlineQuery,
  onOnlineSearch,
  onOnlineInstall,
  onExport,
}: {
  inspection?: ModpackInspection;
  busy: boolean;
  message: string;
  dragging: boolean;
  onPick: () => void;
  onImport: () => void;
  instances: Instance[];
  targetId?: number;
  onTarget: (id: number) => void;
  onlineQuery: string;
  onlineProjects: OnlineProject[];
  onOnlineQuery: (value: string) => void;
  onOnlineSearch: () => void;
  onOnlineInstall: (project: OnlineProject) => void;
  onExport: (instanceId: number, includeSaves: boolean) => void;
}) {
  const [exportInstanceId, setExportInstanceId] = useState<number>();
  const [includeSaves, setIncludeSaves] = useState(false);
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
        onInstall={onOnlineInstall} />
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
          {inspection.format !== "modrinth" ? (
            <select
              className="pack-target"
              value={targetId ?? ""}
              onChange={(event) => onTarget(Number(event.target.value))}
            >
              <option value="" disabled>
                选择要导入到哪套现有游戏
              </option>
              {instances.map((instance) => (
                <option value={instance.id} key={instance.id}>
                  {instance.name} · {loaderLabel(instance.loaderType)}{" "}
                  {instance.gameVersion}
                </option>
              ))}
            </select>
          ) : null}
          <button
            className="primary pack-import"
            disabled={busy || (inspection.format !== "modrinth" && !targetId)}
            onClick={onImport}
          >
            {inspection.format === "modrinth"
              ? "创建新游戏配置并导入"
              : "导入本地可用内容"}
          </button>
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
