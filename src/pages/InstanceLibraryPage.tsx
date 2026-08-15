import type { Instance } from "../types";
import { useMemo, useState } from "react";
import { Box, Play, Plus, Search } from "lucide-react";
import grassBlock from "../assets/grass-block.png";

export function InstanceLibraryPage({
  instances,
  onPlay,
  onCreate,
  onClone,
  onRename,
  onRepair,
  onDelete,
  onOpen,
}: {
  instances: Instance[];
  onPlay: (instance: Instance) => void;
  onCreate: () => void;
  onClone: (instance: Instance) => void;
  onRename: (instance: Instance) => void;
  onRepair: (instance: Instance) => void;
  onDelete: (instance: Instance) => void;
  onOpen: (instance: Instance) => void;
}) {
  const [query, setQuery] = useState("");
  const [loader, setLoader] = useState("all");
  const [sort, setSort] = useState("name");
  const visibleInstances = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase("zh-CN");
    return instances
      .filter((instance) => loader === "all" || instance.loaderType === loader)
      .filter((instance) =>
        !normalized || `${instance.name} ${instance.gameVersion} ${instance.loaderType}`
          .toLocaleLowerCase("zh-CN")
          .includes(normalized),
      )
      .toSorted((left, right) => {
        if (sort === "version") return right.gameVersion.localeCompare(left.gameVersion, undefined, { numeric: true });
        if (sort === "status") return left.status.localeCompare(right.status);
        return left.name.localeCompare(right.name, "zh-CN");
      });
  }, [instances, loader, query, sort]);

  return (
    <>
      <header>
        <div>
          <h1>游戏库</h1>
          <p>所有 Minecraft 实例都在这里，彼此独立保存。</p>
        </div>
        <button className="quiet" onClick={onCreate}><Plus size={16} /> 新建实例</button>
      </header>
      <section className="library-toolbar">
        <label><Search size={16} /><input aria-label="搜索实例" placeholder="搜索实例" value={query} onChange={(event) => setQuery(event.target.value)} /></label>
        <select aria-label="按模组环境筛选" value={loader} onChange={(event) => setLoader(event.target.value)}>
          <option value="all">全部环境</option>
          <option value="vanilla">Vanilla</option>
          <option value="fabric">Fabric</option>
          <option value="forge">Forge</option>
          <option value="neoforge">NeoForge</option>
          <option value="quilt">Quilt</option>
        </select>
        <select aria-label="实例排序" value={sort} onChange={(event) => setSort(event.target.value)}>
          <option value="name">按名称</option>
          <option value="version">按游戏版本</option>
          <option value="status">按安装状态</option>
        </select>
        <span>{visibleInstances.length} / {instances.length} 个实例</span>
      </section>
      {instances.length ? (
        visibleInstances.length ? <section className="instance-library-grid">
          {visibleInstances.map((instance) => (
            <article className="library-card" key={instance.id}>
              <div className="library-art"><img src={grassBlock} alt="" /></div>
              <div className="library-card-body">
                <div className="library-card-title"><h2>{instance.name}</h2></div>
                <p>{instance.gameVersion} · {instance.loaderType === "vanilla" ? "Vanilla" : instance.loaderType}</p>
                <div className="library-card-meta"><span><Box size={14} /> {instance.status === "ready" ? "已就绪" : "待安装"}</span><span>{instance.memoryMb} MB</span></div>
                <button className="library-play" onClick={() => onPlay(instance)}><Play size={15} fill="currentColor" /> 启动实例</button>
                <div className="library-secondary-actions"><button onClick={() => onOpen(instance)}>文件夹</button><button onClick={() => onRepair(instance)}>修复</button><button onClick={() => onRename(instance)}>重命名</button><button onClick={() => onClone(instance)}>复制</button><button className="danger" onClick={() => onDelete(instance)}>移除</button></div>
              </div>
            </article>
          ))}
        </section> : <section className="library-empty"><Search size={30} /><h2>没有符合条件的实例</h2><p>换一个名称或筛选条件再试。</p><button className="quiet" onClick={() => { setQuery(""); setLoader("all"); }}>清除筛选</button></section>
      ) : (
        <section className="library-empty"><img src={grassBlock} alt="" /><h2>还没有游戏实例</h2><p>创建第一个实例后，就可以在这里一键启动。</p><button className="play" onClick={onCreate}><Plus size={17} /> 创建实例</button></section>
      )}
    </>
  );
}
