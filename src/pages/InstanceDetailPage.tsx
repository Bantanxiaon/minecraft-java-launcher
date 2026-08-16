import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { ContentItem, Instance, InstanceHealth, RemovedContent } from "../types";
import { loaderLabel } from "../ui";

type InstanceDetailPageProps = {
  instance: Instance;
  javaLabel: string;
  onBack: () => void;
  onLaunch: (instance: Instance) => void;
  onRepair: (instance: Instance) => void;
  onOpenFolder: (instance: Instance) => void;
  onMemoryChange: (instance: Instance, memoryMb: number) => void;
};

type ReconcileReportLoose = {
  instanceId: number;
  dbMissingOnDisk: string[];
  diskMissingInDb: string[];
  duplicateGroups: Array<{
    sha256: string;
    files: string[];
    keep: string;
    removableBytes: number;
  }>;
  fingerprint: string;
};

type ModpackUpdatePlanLoose = {
  instanceId: number;
  packVersion?: string;
  installs: string[];
  updates: string[];
  removals: string[];
  dependencyChanges: string[];
  conflicts: string[];
  protectedUserFiles: string[];
};

const MEMORY_PRESETS = [4096, 6144, 8192, 10240, 12288, 14336, 16384];

type Tab = "概览" | "内容" | "设置" | "日志" | "对账";

const KIND_LABELS: Record<string, string> = {
  mod: "模组",
  resourcepack: "资源包",
  shaderpack: "光影",
  world: "存档",
};

function contentDisplayName(item: ContentItem): string {
  try {
    const metadata = item.metadataJson
      ? (JSON.parse(item.metadataJson) as { name?: string; modId?: string })
      : {};
    if (metadata.name?.trim()) return metadata.name.trim();
    if (metadata.modId?.trim()) return metadata.modId.trim();
  } catch {
    // 忽略损坏的元数据，回退文件名。
  }
  return item.fileName.replace(/\.(jar|zip|mrpack)$/i, "");
}

function contentIcon(item: ContentItem): string | null {
  try {
    const metadata = item.metadataJson
      ? (JSON.parse(item.metadataJson) as { modrinthProjectId?: string })
      : {};
    if (metadata.modrinthProjectId) {
      return `https://cdn.modrinth.com/data/${metadata.modrinthProjectId}/icon.png`;
    }
  } catch {
    // 忽略
  }
  return null;
}

function contentVersion(item: ContentItem): string | null {
  try {
    const metadata = item.metadataJson
      ? (JSON.parse(item.metadataJson) as { version?: string })
      : {};
    return metadata.version?.trim() || null;
  } catch {
    return null;
  }
}

export function InstanceDetailPage({
  instance,
  javaLabel,
  onBack,
  onLaunch,
  onRepair,
  onOpenFolder,
  onMemoryChange,
}: InstanceDetailPageProps) {
  const [tab, setTab] = useState<Tab>("概览");
  const [health, setHealth] = useState<InstanceHealth | null>(null);
  const [content, setContent] = useState<ContentItem[]>([]);
  const [contentLoading, setContentLoading] = useState(false);
  const [logs, setLogs] = useState<{ fileName: string; size: number; modifiedAt: number }[]>([]);
  const [logText, setLogText] = useState("");
  const [reconcile, setReconcile] = useState<ReconcileReportLoose | null>(null);
  const [updatePlan, setUpdatePlan] = useState<ModpackUpdatePlanLoose | null>(null);
  const [message, setMessage] = useState("");

  const refreshHealth = useCallback(async () => {
    try {
      setHealth(await invoke<InstanceHealth>("instance_health", { instanceId: instance.id }));
    } catch (error) {
      setMessage(String(error));
    }
  }, [instance.id]);

  useEffect(() => {
    void refreshHealth();
  }, [refreshHealth]);

  async function loadLogs() {
    try {
      const all = await invoke<
        { instanceId: number; fileName: string; size: number; modifiedAt: number }[]
      >("list_game_logs");
      setLogs(all.filter((log) => log.instanceId === instance.id).slice(0, 60));
      setLogText("");
    } catch (error) {
      setMessage(String(error));
    }
  }

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
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function scanReconcile() {
    try {
      const report = await invoke<ReconcileReportLoose>("reconcile_scan", {
        instanceId: instance.id,
      });
      setReconcile(report);
      setMessage("");
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function applyReconcile() {
    if (!reconcile) return;
    if (!window.confirm("对账将补充缺失记录并清理完全重复的 JAR（保留到备份），确认继续？")) return;
    try {
      const result = await invoke<{
        addedRecords: number;
        removedStaleRecords: number;
        deduplicatedFiles: number;
        freedBytes: number;
      }>("reconcile_apply", { instanceId: instance.id, fingerprint: reconcile.fingerprint });
      setMessage(
        `对账完成：新增 ${result.addedRecords}、清理过期记录 ${result.removedStaleRecords}、去重 ${result.deduplicatedFiles}、释放 ${Math.round(result.freedBytes / 1024 / 1024)} MB。`,
      );
      setReconcile(null);
      void refreshHealth();
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function loadContent() {
    setContentLoading(true);
    try {
      setContent(
        await invoke<ContentItem[]>("list_content_items", {
          instanceId: instance.id,
        }),
      );
      setMessage("");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setContentLoading(false);
    }
  }

  async function toggleItem(item: ContentItem) {
    try {
      const command =
        item.kind === "mod" ? "set_mod_enabled" : "set_content_enabled";
      const updated = await invoke<ContentItem>(command, {
        contentId: item.id,
        enabled: !item.enabled,
      });
      setContent((existing) =>
        existing.map((candidate) =>
          candidate.id === updated.id ? updated : candidate,
        ),
      );
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function removeItem(item: ContentItem) {
    const label = KIND_LABELS[item.kind] ?? item.kind;
    if (!window.confirm(`将“${contentDisplayName(item)}”移到可恢复备份？（${label}）`)) return;
    try {
      const command =
        item.kind === "mod"
          ? "remove_mod_to_backup"
          : item.kind === "world"
            ? "remove_world_to_backup"
            : "remove_content_to_backup";
      const removed = await invoke<RemovedContent>(command, {
        contentId: item.id,
      });
      setContent((existing) =>
        existing.filter((candidate) => candidate.id !== item.id),
      );
      setMessage(`已移至可恢复备份：${removed.backupPath}`);
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function updateItem(item: ContentItem) {
    setMessage(`正在安全更新 ${contentDisplayName(item)}…`);
    try {
      const updated = await invoke<ContentItem>("update_modrinth_mod", {
        contentId: item.id,
      });
      setContent((existing) =>
        existing.map((candidate) =>
          candidate.id === updated.id ? updated : candidate,
        ),
      );
      setMessage("模组已更新，旧文件已放入可恢复备份。");
    } catch (error) {
      setMessage(`${String(error)}`);
    }
  }

  async function backupWorld(item: ContentItem) {
    try {
      const backup = await invoke<RemovedContent>("backup_world", {
        contentId: item.id,
      });
      setMessage(`存档已备份：${backup.backupPath}`);
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function duplicateWorld(item: ContentItem) {
    try {
      const duplicate = await invoke<ContentItem>("duplicate_world", {
        contentId: item.id,
      });
      setContent((existing) => [duplicate, ...existing]);
      setMessage(`存档已复制：${duplicate.fileName}`);
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function updateModpack() {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Modrinth Modpack", extensions: ["mrpack", "zip"] }],
      });
      const sourcePath = typeof selected === "string" ? selected : null;
      if (!sourcePath) return;
      setMessage("正在生成整合包更新计划（会先安全下载并校验新内容）…");
      const plan = await invoke<ModpackUpdatePlanLoose>(
        "update_modrinth_modpack",
        { instanceId: instance.id, sourcePath },
      );
      setUpdatePlan(plan);
      if (plan.conflicts.length) {
        setMessage(`更新完成，但有 ${plan.conflicts.length} 个文件被保护未覆盖。`);
      } else if (
        plan.installs.length + plan.updates.length + plan.removals.length === 0
      ) {
        setMessage("整合包已是最新，没有需要更新的内容。");
      } else {
        setMessage(
          `整合包已更新：新增 ${plan.installs.length}、更新 ${plan.updates.length}、移除 ${plan.removals.length}。`,
        );
      }
      void loadContent();
      void refreshHealth();
    } catch (error) {
      setMessage(String(error));
    }
  }

  const contentGroups = (["mod", "resourcepack", "shaderpack", "world"] as const)
    .map((kind) => ({
      kind,
      items: content.filter((item) => item.kind === kind),
    }))
    .filter((group) => group.items.length > 0);

  return (
    <>
      <header>
        <div>
          <h1>{instance.name}</h1>
          <p>
            Minecraft {instance.gameVersion} · {loaderLabel(instance.loaderType)} · {javaLabel}
          </p>
        </div>
        <button className="quiet" onClick={onBack}>返回游戏库</button>
      </header>
      <nav className="instance-tabs">
        {(["概览", "内容", "设置", "日志", "对账"] as const).map((item) => (
          <button
            key={item}
            className={tab === item ? "active" : ""}
            onClick={() => {
              setTab(item);
              setMessage("");
              if (item === "内容") void loadContent();
              if (item === "日志") void loadLogs();
              if (item === "对账") void scanReconcile();
            }}
          >
            {item}
          </button>
        ))}
      </nav>

      {tab === "概览" && health ? (
        <section className="pack-export-card">
          <h2>健康状态</h2>
          <ul className="instance-health-list">
            <li>{health.gameFilesOk ? "✓ 游戏文件完整" : "⚠ 游戏文件待安装"}</li>
            <li>{health.status === "ready" ? "✓ 实例就绪" : `状态：${health.status}`}</li>
            <li>{health.loaderType} {health.loaderVersion ?? ""} · 模组 {health.modCount} 个</li>
            {health.missingDependencies.length ? (
              <li className="danger">⚠ 缺失前置：{health.missingDependencies.join("、")}</li>
            ) : (
              <li>✓ 前置完整</li>
            )}
            {health.incompatibleMods.length ? (
              <li className="danger">⚠ 不兼容模组：{health.incompatibleMods.slice(0, 8).join("、")}</li>
            ) : null}
          </ul>
          <div className="server-form-actions">
            <button className="primary" onClick={() => onLaunch(instance)}>启动游戏</button>
            <button onClick={() => onRepair(instance)}>修复 / 校验</button>
            <button onClick={() => onOpenFolder(instance)}>打开文件夹</button>
            {instance.source === "modrinth" ? (
              <button onClick={() => void updateModpack()}>更新整合包</button>
            ) : null}
          </div>
        </section>
      ) : tab === "概览" ? (
        <p className="mod-message">正在读取健康状态…</p>
      ) : null}

      {tab === "概览" && updatePlan ? (
        <section className="pack-export-card">
          <div className="section-heading">
            <div>
              <h2>整合包更新计划</h2>
              <p>版本 {updatePlan.packVersion ?? "未知"}</p>
            </div>
            <button className="quiet" onClick={() => setUpdatePlan(null)}>关闭</button>
          </div>
          <ul className="instance-health-list">
            <li>新增 {updatePlan.installs.length} · 更新 {updatePlan.updates.length} · 移除 {updatePlan.removals.length}</li>
            {updatePlan.conflicts.length ? (
              <li className="danger">⚠ 已保护 {updatePlan.protectedUserFiles.length} 个用户文件（未覆盖）：{updatePlan.conflicts.join("；")}</li>
            ) : null}
            {updatePlan.updates.map((file) => <li key={file}>更新：{file}</li>)}
            {updatePlan.installs.map((file) => <li key={file}>新增：{file}</li>)}
            {updatePlan.removals.map((file) => <li key={file}>移除：{file}</li>)}
          </ul>
        </section>
      ) : null}

      {tab === "内容" ? (
        <section className="installed-mods">
          <div className="section-heading">
            <div>
              <h2>实例内容</h2>
              <p>模组、资源包、光影与存档的钻取式管理，技术文件名在详情中展示</p>
            </div>
          </div>
          {contentLoading ? (
            <p className="mod-message">正在读取内容…</p>
          ) : contentGroups.length ? (
            contentGroups.map((group) => (
              <div key={group.kind} className="instance-content-group">
                <h3>{KIND_LABELS[group.kind]} · {group.items.length}</h3>
                <div className="mod-rows">
                  {group.items.map((item) => {
                    const icon = contentIcon(item);
                    const version = contentVersion(item);
                    return (
                      <div key={item.id} className="content-detail-row">
                        <div className="content-detail-identity">
                          {icon ? (
                            <img
                              className="content-detail-icon"
                              src={icon}
                              alt=""
                              loading="lazy"
                              onError={(event) => {
                                event.currentTarget.style.display = "none";
                              }}
                            />
                          ) : (
                            <span className="content-detail-badge">
                              {KIND_LABELS[group.kind].slice(0, 1)}
                            </span>
                          )}
                          <div>
                            <strong>{contentDisplayName(item)}</strong>
                            <span className="content-detail-file">
                              {item.fileName}
                              {version ? ` · v${version}` : ""} · {item.source}
                            </span>
                          </div>
                        </div>
                        <div className="content-detail-actions">
                          {group.kind !== "world" ? (
                            <button onClick={() => void toggleItem(item)}>
                              {item.enabled ? "停用" : "启用"}
                            </button>
                          ) : (
                            <>
                              <button onClick={() => void backupWorld(item)}>
                                备份
                              </button>
                              <button onClick={() => void duplicateWorld(item)}>
                                复制
                              </button>
                            </>
                          )}
                          {item.kind === "mod" && item.source === "modrinth" ? (
                            <button onClick={() => void updateItem(item)}>
                              更新
                            </button>
                          ) : null}
                          <button onClick={() => void removeItem(item)}>
                            移除
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            ))
          ) : (
            <p className="mod-message">这个实例还没有已记录的内容。</p>
          )}
        </section>
      ) : null}

      {tab === "设置" ? (
        <section className="pack-export-card">
          <h2>实例设置</h2>
          <label className="library-memory">
            运行内存
            <select
              value={MEMORY_PRESETS.includes(instance.memoryMb) ? instance.memoryMb : "custom"}
              onChange={(event) => {
                if (event.target.value !== "custom") {
                  onMemoryChange(instance, Number(event.target.value));
                }
              }}
            >
              {MEMORY_PRESETS.map((mb) => (
                <option key={mb} value={mb}>{mb / 1024} GB</option>
              ))}
              <option value="custom">自定义</option>
            </select>
            <span>{instance.memoryMb} MB</span>
          </label>
          <p className="notice">Java：{javaLabel}（Java 选择与自动安装可在“设置 → Java”管理）。</p>
        </section>
      ) : null}

      {tab === "日志" ? (
        <section className="installed-mods">
          <div className="section-heading"><div><h2>游戏日志</h2><p>最近 {logs.length} 条</p></div></div>
          <div className="mod-rows">
            {logs.map((log) => (
              <div key={log.fileName}>
                <strong>{log.fileName}</strong>
                <span>{Math.round(log.size / 1024)} KB</span>
                <button onClick={() => void readLog(log.fileName)}>读取</button>
              </div>
            ))}
          </div>
          {logText ? <pre className="log-preview">{logText.slice(-20000)}</pre> : null}
        </section>
      ) : null}

      {tab === "对账" ? (
        <section className="pack-export-card">
          <h2>磁盘与数据库对账</h2>
          {reconcile ? (
            <>
              <p>
                数据库有但磁盘无：{reconcile.dbMissingOnDisk.length} · 磁盘有但数据库无：{reconcile.diskMissingInDb.length} · 完全重复组：{reconcile.duplicateGroups.length}
              </p>
              {reconcile.duplicateGroups.map((group) => (
                <p key={group.sha256} className="pack-warning">
                  重复（{group.sha256.slice(0, 8)}）：保留 {group.keep}，移除 {group.files.length - 1} 份，可释放 {Math.round(group.removableBytes / 1024 / 1024)} MB
                </p>
              ))}
              <div className="server-form-actions">
                <button className="primary" onClick={() => void applyReconcile()}>应用对账</button>
                <button onClick={() => void scanReconcile()}>重新扫描</button>
              </div>
            </>
          ) : (
            <p className="mod-message">点击“重新扫描”检查内容一致性。</p>
          )}
        </section>
      ) : null}
      {message ? <p className="form-message" role="status">{message}</p> : null}
    </>
  );
}
