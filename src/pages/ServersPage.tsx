import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Account, Instance, RoomInfo, ServerEntry, ServerPing } from "../types";

type ServersPageProps = {
  servers: ServerEntry[];
  instances: Instance[];
  accounts: Account[];
  selectedInstanceId?: number;
  selectedAccountId?: number;
  javaPath?: string;
  busy: boolean;
  message: string;
  onAddServer: (
    name: string,
    address: string,
    port: number,
    description: string,
  ) => Promise<void>;
  onUpdateServer: (
    server: ServerEntry,
    name: string,
    address: string,
    port: number,
    description: string,
  ) => Promise<void>;
  onRemoveServer: (server: ServerEntry) => Promise<void>;
  onJoin: (server: ServerEntry, instanceId: number, accountId: number) => void;
};

type PingState = {
  checking: boolean;
  result?: ServerPing;
};

const DEFAULT_PORT = 25565;

export function ServersPage({
  servers,
  instances,
  accounts,
  selectedInstanceId,
  selectedAccountId,
  javaPath,
  busy,
  message,
  onAddServer,
  onUpdateServer,
  onRemoveServer,
  onJoin,
}: ServersPageProps) {
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<ServerEntry | null>(null);
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [port, setPort] = useState(String(DEFAULT_PORT));
  const [description, setDescription] = useState("");
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState("");
  const [pings, setPings] = useState<Record<number, PingState>>({});
  const [joinInstanceId, setJoinInstanceId] = useState<number | undefined>(
    selectedInstanceId,
  );
  const [joinAccountId, setJoinAccountId] = useState<number | undefined>(
    selectedAccountId,
  );
  const [room, setRoom] = useState<RoomInfo>();
  const [roomBusy, setRoomBusy] = useState(false);
  const [roomMessage, setRoomMessage] = useState("");

  useEffect(() => {
    let dispose: (() => void) | undefined;
    let cancelled = false;
    void listen<RoomInfo>("multiplayer-state", (event) => {
      setRoom(event.payload);
    }).then((unlisten) => {
      if (cancelled) unlisten();
      else dispose = unlisten;
    });
    return () => {
      cancelled = true;
      dispose?.();
    };
  }, []);

  const readyInstances = instances.filter((instance) => instance.status === "ready");
  const effectiveInstanceId =
    joinInstanceId && readyInstances.some((instance) => instance.id === joinInstanceId)
      ? joinInstanceId
      : readyInstances[0]?.id;
  const effectiveAccountId =
    joinAccountId && accounts.some((account) => account.id === joinAccountId)
      ? joinAccountId
      : accounts[0]?.id;

  function openAddForm() {
    setEditing(null);
    setName("");
    setAddress("");
    setPort(String(DEFAULT_PORT));
    setDescription("");
    setFormError("");
    setFormOpen(true);
  }

  function openEditForm(server: ServerEntry) {
    setEditing(server);
    setName(server.name);
    setAddress(server.address);
    setPort(String(server.port));
    setDescription(server.description);
    setFormError("");
    setFormOpen(true);
  }

  async function saveForm() {
    const parsedPort = Number(port);
    if (!name.trim() || !address.trim()) return;
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
      setFormError("端口须为 1–65535。");
      return;
    }
    setFormError("");
    setSaving(true);
    try {
      if (editing) {
        await onUpdateServer(editing, name.trim(), address.trim(), parsedPort, description.trim());
      } else {
        await onAddServer(name.trim(), address.trim(), parsedPort, description.trim());
      }
      setFormOpen(false);
      setEditing(null);
    } finally {
      setSaving(false);
    }
  }

  async function checkPing(server: ServerEntry) {
    setPings((existing) => ({
      ...existing,
      [server.id]: { checking: true },
    }));
    try {
      const result = await invoke<ServerPing>("ping_server", {
        address: server.address,
        port: server.port,
      });
      setPings((existing) => ({
        ...existing,
        [server.id]: { checking: false, result },
      }));
    } catch (error) {
      setPings((existing) => ({
        ...existing,
        [server.id]: {
          checking: false,
          result: { reachable: false, error: String(error) },
        },
      }));
    }
  }

  function join(server: ServerEntry) {
    if (!effectiveInstanceId || !effectiveAccountId) return;
    onJoin(server, effectiveInstanceId, effectiveAccountId);
  }

  async function createRoom() {
    if (!effectiveInstanceId || !effectiveAccountId) {
      setRoomMessage("请先选择启动配置和账户。");
      return;
    }
    if (!javaPath) {
      setRoomMessage("未检测到可用的 64 位 Java，请先到设置安装。");
      return;
    }
    setRoomBusy(true);
    setRoomMessage("");
    try {
      await invoke("multiplayer_prepare", { instanceId: effectiveInstanceId });
      const started = await invoke<RoomInfo>("multiplayer_start", {
        instanceId: effectiveInstanceId,
        accountId: effectiveAccountId,
        javaPath,
      });
      setRoom(started);
      setRoomMessage("游戏正在启动；进入世界后选择“对局域网开放”，这里会自动显示邀请地址。");
    } catch (error) {
      setRoomMessage(String(error));
    } finally {
      setRoomBusy(false);
    }
  }

  async function stopRoom() {
    if (!effectiveInstanceId) return;
    try {
      setRoom(await invoke<RoomInfo>("multiplayer_stop", { instanceId: effectiveInstanceId }));
      setRoomMessage("联机已结束。");
    } catch (error) {
      setRoomMessage(String(error));
    }
  }

  return (
    <>
      <header>
        <div>
          <h1>联机</h1>
          <p>一键创建联机房间，或保存常用服务器地址快速加入。</p>
        </div>
        <span className="ready-label">免费联机</span>
      </header>

      <section className="pack-export-card">
        <div>
          <h2>一键创建房间</h2>
          <p>
            自动安装联机组件（e4mc），启动游戏后进入世界点“对局域网开放”，好友就能通过邀请地址直接加入。
          </p>
        </div>
        {room?.address ? (
          <div className="server-row-side">
            <span className="ping-badge ok">邀请地址</span>
            <code>{room.address}</code>
            <button type="button" onClick={() => void stopRoom()}>结束联机</button>
          </div>
        ) : room && room.state !== "IDLE" && room.state !== "CLOSED" && room.state !== "STOPPED" ? (
          <span className="ping-badge pending">{room.state === "PREPARING" ? "准备联机组件…" : "等待你在游戏中开放局域网…"}</span>
        ) : null}
        <div className="server-form-actions">
          <button
            className="primary"
            type="button"
            disabled={roomBusy || !effectiveInstanceId || !effectiveAccountId}
            onClick={() => void createRoom()}
          >
            {roomBusy ? "创建中…" : "创建房间并启动游戏"}
          </button>
        </div>
        {roomMessage ? <p className="pack-warning">{roomMessage}</p> : null}
      </section>

      <section className="server-toolbar">
        <label>
          <span>启动配置</span>
          <select
            value={effectiveInstanceId ?? ""}
            onChange={(event) => setJoinInstanceId(Number(event.target.value))}
          >
            {readyInstances.length ? readyInstances.map((instance) => (
              <option key={instance.id} value={instance.id}>
                {instance.name} · {instance.gameVersion}
              </option>
            )) : (
              <option value="" disabled>没有可启动的游戏配置</option>
            )}
          </select>
        </label>
        <label>
          <span>使用账户</span>
          <select
            value={effectiveAccountId ?? ""}
            onChange={(event) => setJoinAccountId(Number(event.target.value))}
          >
            {accounts.length ? accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.displayName} · {account.accountType === "MICROSOFT" ? "正版" : account.accountType === "EXTERNAL" ? "外置" : "离线"}
              </option>
            )) : (
              <option value="" disabled>还没有账户</option>
            )}
          </select>
        </label>
        <button
          className="primary"
          type="button"
          disabled={busy || saving}
          onClick={openAddForm}
        >
          + 添加服务器
        </button>
      </section>

      {formOpen ? (
        <section className="server-form">
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="服务器名称，如 暮色世界公益服"
          />
          <input
            value={address}
            onChange={(event) => setAddress(event.target.value)}
            placeholder="地址，如 play.example.com 或 192.168.1.10"
          />
          <input
            value={port}
            onChange={(event) => setPort(event.target.value)}
            placeholder={`端口（默认 ${DEFAULT_PORT}）`}
            inputMode="numeric"
          />
          <input
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            placeholder="备注（可选）"
          />
          <div className="server-form-actions">
            <button
              className="primary"
              type="button"
              disabled={saving || busy || !name.trim() || !address.trim()}
              onClick={() => void saveForm()}
            >
              {saving ? "保存中…" : editing ? "保存修改" : "添加"}
            </button>
            <button
              type="button"
              disabled={saving}
              onClick={() => {
                setFormOpen(false);
                setEditing(null);
              }}
            >
              取消
            </button>
          </div>
          {formError ? <p className="pack-warning">{formError}</p> : null}
        </section>
      ) : null}

      <section className="server-list">
        {servers.length ? servers.map((server) => {
          const ping = pings[server.id];
          return (
            <div className="server-row" key={server.id}>
              <div className="server-row-main">
                <strong>{server.name}</strong>
                <span>{server.address}:{server.port}</span>
                {server.description ? <small>{server.description}</small> : null}
              </div>
              <div className="server-row-side">
                {ping?.checking ? (
                  <em className="ping-badge pending">检测中…</em>
                ) : ping?.result ? (
                  ping.result.reachable ? (
                    <em className="ping-badge ok">在线 · {ping.result.latencyMs} ms</em>
                  ) : (
                    <em className="ping-badge fail" title={ping.result.error}>
                      离线{ping.result.error ? ` · ${ping.result.error}` : ""}
                    </em>
                  )
                ) : (
                  <em className="ping-badge idle">未检测</em>
                )}
                <div className="server-row-actions">
                  <button
                    type="button"
                    disabled={busy || ping?.checking}
                    onClick={() => void checkPing(server)}
                  >
                    检测
                  </button>
                  <button
                    type="button"
                    disabled={busy || !effectiveInstanceId || !effectiveAccountId}
                    onClick={() => join(server)}
                  >
                    启动并加入
                  </button>
                  <button type="button" onClick={() => openEditForm(server)}>编辑</button>
                  <button
                    className="danger"
                    type="button"
                    disabled={busy}
                    onClick={() => void onRemoveServer(server)}
                  >
                    删除
                  </button>
                </div>
              </div>
            </div>
          );
        }) : (
          <div className="server-empty">
            <div className="server-symbol">◎</div>
            <h2>还没有服务器</h2>
            <p>
              点击“添加服务器”保存地址；以后启动游戏时会自动带上服务器参数直接加入，不影响单机模式。
            </p>
          </div>
        )}
      </section>
      {message ? <p className="form-message" role="status">{message}</p> : null}
    </>
  );
}
