import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CleanupPlan, DeletedInstance, StorageOverview } from "../types";

const CATEGORY_LABELS: Record<string, string> = {
  INSTANCE: "游戏实例",
  DOWNLOAD_CACHE: "下载缓存",
  PARTIAL_DOWNLOAD: "未完成下载",
  JAVA_RUNTIME: "Java 运行环境",
  JAVA_ARCHIVE: "Java 安装包",
  LOADER_INSTALLER: "加载器安装包",
  LOG: "日志",
  CRASH_REPORT: "崩溃报告",
  WORLD_BACKUP: "世界备份",
  REMOVED_CONTENT_BACKUP: "内容备份",
  DELETED_INSTANCE: "已删除实例",
  TEMPORARY: "临时文件",
  CORRUPT_BACKUP: "损坏备份",
};

function formatBytes(value?: number): string {
  if (!value) return "0 B";
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(2)} GB`;
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.round(value / 1024)} KB`;
}

export function StoragePage() {
  const [overview, setOverview] = useState<StorageOverview | null>(null);
  const [deleted, setDeleted] = useState<DeletedInstance[]>([]);
  const [plan, setPlan] = useState<CleanupPlan | null>(null);
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [overviewValue, deletedValue] = await Promise.all([
        invoke<StorageOverview>("get_storage_overview"),
        invoke<DeletedInstance[]>("list_deleted_instances"),
      ]);
      setOverview(overviewValue);
      setDeleted(deletedValue);
      setMessage("");
    } catch (error) {
      setMessage(`无法读取存储信息：${String(error)}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function buildPlan() {
    setMessage("正在预览可清理内容…");
    try {
      setPlan(await invoke<CleanupPlan>("build_safe_cleanup_plan"));
      setMessage("清理预览已生成，确认后才会真正删除。");
    } catch (error) {
      setMessage(`生成清理计划失败：${String(error)}`);
    }
  }

  async function executePlan() {
    if (!plan) return;
    if (!window.confirm(`确定清理 ${formatBytes(plan.reclaimableBytes)} 安全内容吗？不会删除实例、存档和已安装模组。`)) {
      return;
    }
    setMessage("正在清理…");
    try {
      const result = await invoke<{ freedBytes: number; removedItems: number }>(
        "execute_cleanup_plan",
        { fingerprint: plan.fingerprint },
      );
      setMessage(`已释放 ${formatBytes(result.freedBytes)}，清理 ${result.removedItems} 项。`);
      setPlan(null);
      void refresh();
    } catch (error) {
      setMessage(`清理失败：${String(error)}`);
    }
  }

  async function restore(item: DeletedInstance) {
    setMessage("正在恢复实例…");
    try {
      await invoke<number>("restore_deleted_instance", { id: item.id });
      setMessage(`实例“${item.displayName}”已恢复，请到游戏库重新校验游戏文件。`);
      void refresh();
    } catch (error) {
      setMessage(`恢复失败：${String(error)}`);
    }
  }

  async function permanentDelete(item: DeletedInstance) {
    if (!window.confirm(`永久删除“${item.displayName}”？此操作不可恢复。`)) return;
    if (!window.confirm("再次确认：将永久删除该实例备份，无法找回。")) return;
    setMessage("正在永久删除…");
    try {
      await invoke("permanently_delete_instance_backup", { id: item.id });
      setMessage("实例备份已永久删除。");
      void refresh();
    } catch (error) {
      setMessage(`删除失败：${String(error)}`);
    }
  }

  return (
    <>
      <header>
        <div>
          <h1>存储管理</h1>
          <p>查看实例、缓存、备份和临时文件的磁盘占用，安全清理与实例备份管理。</p>
        </div>
        <span className="ready-label">磁盘存储</span>
      </header>

      {loading && !overview ? (
        <p className="mod-message">正在计算磁盘占用…</p>
      ) : overview ? (
        <>
          <section className="storage-hero">
            <div>
              <strong>已使用</strong>
              <span>{formatBytes(overview.totalBytes)}</span>
            </div>
            <div>
              <strong>可安全释放</strong>
              <span>{formatBytes(overview.reclaimableBytes)}</span>
            </div>
            <button className="primary" type="button" onClick={() => void buildPlan()}>
              预览安全清理
            </button>
          </section>
          <section className="installed-mods">
            <div className="section-heading">
              <div>
                <h2>占用分类</h2>
                <p>实例与存档不会被安全清理删除。</p>
              </div>
            </div>
            <div className="storage-categories">
              {overview.categories.map((category) => (
                <div key={category.category} className="storage-category-row">
                  <span>{CATEGORY_LABELS[category.category] ?? category.category}</span>
                  <span>{category.itemCount} 项</span>
                  <strong>{formatBytes(category.bytes)}</strong>
                </div>
              ))}
            </div>
          </section>
          {plan ? (
            <section className="pack-export-card">
              <div>
                <h2>清理预览</h2>
                <p>
                  将清理下载缓存、未完成下载、日志和临时文件，预计释放{" "}
                  {formatBytes(plan.reclaimableBytes)}；不会删除游戏实例、世界存档和已安装模组。
                </p>
              </div>
              <div className="server-form-actions">
                <button className="primary" type="button" onClick={() => void executePlan()}>
                  确认清理 {formatBytes(plan.reclaimableBytes)}
                </button>
                <button type="button" onClick={() => setPlan(null)}>
                  取消
                </button>
              </div>
            </section>
          ) : null}
        </>
      ) : null}

      <section className="installed-mods">
        <div className="section-heading">
          <div>
            <h2>已删除实例</h2>
            <p>删除的实例会先保留在这里，可恢复或永久删除。</p>
          </div>
          <span>{deleted.length} 个</span>
        </div>
        {deleted.length ? (
          <div className="mod-rows">
            {deleted.map((item) => (
              <div key={item.id}>
                <div>
                  <strong>{item.displayName}</strong>
                  <small>
                    {item.gameVersion ? `Minecraft ${item.gameVersion}` : "版本未知"}
                    {item.loaderType ? ` · ${item.loaderType}` : ""} · 删除于{" "}
                    {new Date(Number(item.deletedAt) * 1000).toLocaleString("zh-CN")}
                  </small>
                </div>
                <span>{formatBytes(item.sizeBytes)}</span>
                <button onClick={() => void restore(item)}>恢复</button>
                <button className="danger" onClick={() => void permanentDelete(item)}>
                  永久删除
                </button>
              </div>
            ))}
          </div>
        ) : (
          <p className="mod-message">没有已删除的实例。</p>
        )}
      </section>
      {message ? <p className="form-message" role="status">{message}</p> : null}
    </>
  );
}
