import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Account,
  Instance,
  RoomInfo,
  ServerEntry,
  ServerPing,
} from "../types";

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
  onQuickJoin: (address: string, instanceId: number, accountId: number) => void;
};

type PingState = {
  checking: boolean;
  result?: ServerPing;
};

type HistoryEntry = {
  sessionId: string;
  startedAt: string;
  endedAt?: string;
  provider: string;
  gameVersion?: string;
  loader?: string;
  helperVersion?: string;
  gotAddress: boolean;
  exitReason?: string;
};

const DEFAULT_PORT = 25565;

function errorText(error: unknown, fallback: string): string {
  if (
    error &&
    typeof error === "object" &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message;
  }
  return fallback || String(error);
}

function roomStatus(room: RoomInfo): { title: string; hint?: string } {
  switch (room.state) {
    case "PREPARING":
      return { title: "正在准备联机组件…" };
    case "INSTALLING_HELPER":
      return { title: "正在安装联机组件…" };
    case "GAME_STARTING":
      return { title: "正在启动游戏…" };
    case "WAITING_FOR_LAN":
      return {
        title: "游戏已启动",
        hint: "进入你的世界后点击“对局域网开放”，这里会自动显示邀请地址。",
      };
    case "LAN_OPENED":
      return {
        title: "世界已开放，正在建立公网联机…",
      };
    case "WAITING_FOR_TUNNEL":
      return { title: "正在连接公网中继…" };
    case "READY":
      return {
        title: "房间已创建",
        hint: room.lanPort
          ? "把下面的地址发给朋友即可加入。"
          : "公网地址已分配。进入世界后点“对局域网开放”，朋友即可加入。",
      };
    case "RECONNECTING":
      return {
        title: "连接中断，正在等待恢复…",
        hint: room.userMessage,
      };
    case "STOPPING":
      return { title: "正在结束联机…" };
    case "CLOSED":
      return { title: "联机已结束", hint: room.userMessage };
    case "ERROR":
      return { title: "暂时无法建立公网联机", hint: room.userMessage };
    case "IDLE":
    default:
      return { title: "" };
  }
}

function isValidQuickJoinAddress(value: string): boolean {
  const address = value.trim().toLowerCase().replace(/\.+$/, "");
  if (!address.endsWith(".e4mc.link") || address.length > 253) return false;
  if (/[/\\: ]/.test(address)) return false;
  const label = address.slice(0, -".e4mc.link".length);
  if (!label || label.length > 200 || label.includes("..")) return false;
  return /^[a-z0-9-]+(\.[a-z0-9-]+)*$/.test(label) && !label.startsWith("-");
}

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
  onQuickJoin,
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
  const [copied, setCopied] = useState(false);
  const [quickJoinAddress, setQuickJoinAddress] = useState("");
  const [joinMessage, setJoinMessage] = useState("");
  const [diagnostics, setDiagnostics] = useState<unknown>();
  const [history, setHistory] = useState<HistoryEntry[]>();
  const [historyBusy, setHistoryBusy] = useState(false);

  const readyInstances = instances.filter(
    (instance) => instance.status === "ready",
  );
  const effectiveInstanceId =
    joinInstanceId &&
    readyInstances.some((instance) => instance.id === joinInstanceId)
      ? joinInstanceId
      : readyInstances[0]?.id;
  const effectiveAccountId =
    joinAccountId && accounts.some((account) => account.id === joinAccountId)
      ? joinAccountId
      : accounts[0]?.id;

  const instanceRef = useRef<number | undefined>(undefined);
  useEffect(() => {
    instanceRef.current = effectiveInstanceId;
  }, [effectiveInstanceId]);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    let cancelled = false;
    void listen<RoomInfo>("multiplayer-state", (event) => {
      if (instanceRef.current && event.payload.instanceId !== instanceRef.current)
        return;
      setRoom(event.payload);
      setCopied(false);
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
    if (!effectiveInstanceId) return;
    let cancelled = false;
    void invoke<RoomInfo>("multiplayer_state", {
      instanceId: effectiveInstanceId,
    })
      .then((state) => {
        if (!cancelled) setRoom(state);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [effectiveInstanceId]);

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
        await onUpdateServer(
          editing,
          name.trim(),
          address.trim(),
          parsedPort,
          description.trim(),
        );
      } else {
        await onAddServer(
          name.trim(),
          address.trim(),
          parsedPort,
          description.trim(),
        );
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
    setDiagnostics(undefined);
    try {
      const started = await invoke<RoomInfo>("multiplayer_start", {
        instanceId: effectiveInstanceId,
        accountId: effectiveAccountId,
        javaPath,
      });
      setRoom(started);
    } catch (error) {
      setRoomMessage(errorText(error, "创建房间失败。"));
    } finally {
      setRoomBusy(false);
    }
  }

  async function stopRoom() {
    const sessionId = room?.sessionId;
    if (!sessionId) return;
    setRoomBusy(true);
    try {
      setRoom(
        await invoke<RoomInfo>("multiplayer_stop", { sessionId }),
      );
      setRoomMessage("");
    } catch (error) {
      setRoomMessage(errorText(error, "结束联机失败。"));
    } finally {
      setRoomBusy(false);
    }
  }

  async function cancelRoom() {
    const sessionId = room?.sessionId;
    if (!sessionId) return;
    try {
      setRoom(await invoke<RoomInfo>("multiplayer_cancel", { sessionId }));
      setRoomMessage("");
    } catch (error) {
      setRoomMessage(errorText(error, "取消联机失败。"));
    }
  }

  async function copyAddress() {
    const address = room?.publicAddress;
    if (!address) return;
    try {
      await navigator.clipboard.writeText(address);
      setCopied(true);
    } catch {
      setRoomMessage("复制失败，请手动选择并复制地址。");
    }
  }

  async function showDiagnostics() {
    const sessionId = room?.sessionId;
    if (!sessionId) return;
    try {
      setDiagnostics(
        await invoke<unknown>("multiplayer_diagnostics", { sessionId }),
      );
    } catch (error) {
      setRoomMessage(errorText(error, "读取联机诊断失败。"));
    }
  }

  async function showHistory() {
    if (!effectiveInstanceId) return;
    setHistoryBusy(true);
    try {
      setHistory(
        await invoke<HistoryEntry[]>("multiplayer_history", {
          instanceId: effectiveInstanceId,
        }),
      );
    } catch (error) {
      setRoomMessage(errorText(error, "读取联机记录失败。"));
    } finally {
      setHistoryBusy(false);
    }
  }

  function submitQuickJoin() {
    setJoinMessage("");
    if (!effectiveInstanceId || !effectiveAccountId) {
      setJoinMessage("请先选择启动配置和账户。");
      return;
    }
    if (!isValidQuickJoinAddress(quickJoinAddress)) {
      setJoinMessage("邀请地址格式不正确，请输入形如 xxxx.e4mc.link 的地址。");
      return;
    }
    setJoinMessage("正在启动并加入，进入游戏后请稍候连接。");
    onQuickJoin(
      quickJoinAddress.trim().toLowerCase(),
      effectiveInstanceId,
      effectiveAccountId,
    );
  }

  const status = room && room.state !== "IDLE" ? roomStatus(room) : null;
  const roomActive =
    room && !["IDLE", "CLOSED"].includes(room.state) ? room : undefined;

  return (
    <>
      <header>
        <div>
          <h1>联机</h1>
          <p>一键创建临时联机房间，或输入邀请地址快速加入好友的世界。</p>
        </div>
        <span className="ready-label">免费联机</span>
      </header>

      <section className="pack-export-card multiplayer-card">
        <div>
          <h2>一键创建房间</h2>
          <p>
            自动安装并校验联机组件（e4mc），启动游戏后进入世界点“对局域网开放”，
            这里会自动识别公网地址，好友输入地址即可加入。
          </p>
        </div>

        {status && status.title ? (
          <div className="room-state">
            <span
              className={`ping-badge ${
                room?.state === "READY"
                  ? "ok"
                  : room?.state === "ERROR"
                    ? "fail"
                    : "pending"
              }`}
            >
              {status.title}
            </span>
            {status.hint ? <p className="room-hint">{status.hint}</p> : null}
          </div>
        ) : null}

        {room?.state === "READY" && room.publicAddress ? (
          <div className="server-row-side">
            <span className="ping-badge ok">邀请地址</span>
            <code>{room.publicAddress}</code>
            <button type="button" onClick={() => void copyAddress()}>
              {copied ? "已复制" : "复制邀请地址"}
            </button>
          </div>
        ) : null}

        {room && room.helperVersion && roomActive ? (
          <span className="ping-badge idle">e4mc {room.helperVersion}</span>
        ) : null}

        <div className="server-form-actions">
          <button
            className="primary"
            type="button"
            disabled={roomBusy || !!roomActive || !effectiveInstanceId || !effectiveAccountId}
            onClick={() => void createRoom()}
          >
            {roomBusy ? "创建中…" : "创建房间并启动游戏"}
          </button>
          {roomActive ? (
            <>
              {room?.state === "PREPARING" || room?.state === "INSTALLING_HELPER" ? (
                <button type="button" onClick={() => void cancelRoom()}>
                  取消
                </button>
              ) : (
                <button type="button" onClick={() => void stopRoom()}>
                  结束联机（将关闭当前游戏）
                </button>
              )}
              {room?.state === "ERROR" ? (
                <>
                  <button type="button" onClick={() => void createRoom()}>
                    重试
                  </button>
                  <button type="button" onClick={() => void showDiagnostics()}>
                    查看诊断
                  </button>
                </>
              ) : null}
            </>
          ) : null}
        </div>
        {roomMessage ? <p className="pack-warning">{roomMessage}</p> : null}

        <p className="room-note">
          这是临时朋友联机，不是永久服务器；关闭游戏后房间即结束。仅把邀请地址分享给你信任的人。
        </p>
        <p className="room-note">
          e4mc 只提供网络隧道，不绕过 Minecraft 正版验证。使用离线账户时能否加入取决于当前世界与整合包的身份验证方式。
        </p>
      </section>

      <section className="pack-export-card multiplayer-card">
        <div>
          <h2>快速加入</h2>
          <p>粘贴好友发来的 e4mc 邀请地址，直接启动并加入。双方应使用相同的 Minecraft 版本、加载器和一致的 Mod/整合包。</p>
        </div>
        <div className="server-form">
          <input
            value={quickJoinAddress}
            onChange={(event) => setQuickJoinAddress(event.target.value)}
            placeholder="邀请地址，如 xxxx.e4mc.link"
          />
          <div className="server-form-actions">
            <button
              className="primary"
              type="button"
              disabled={busy || !quickJoinAddress.trim()}
              onClick={submitQuickJoin}
            >
              启动并加入
            </button>
          </div>
        </div>
        {joinMessage ? <p className="pack-warning">{joinMessage}</p> : null}
      </section>

      <section className="server-toolbar">
        <label>
          <span>启动配置</span>
          <select
            value={effectiveInstanceId ?? ""}
            onChange={(event) => setJoinInstanceId(Number(event.target.value))}
          >
            {readyInstances.length ? (
              readyInstances.map((instance) => (
                <option key={instance.id} value={instance.id}>
                  {instance.name} · {instance.gameVersion}
                </option>
              ))
            ) : (
              <option value="" disabled>
                没有可启动的游戏配置
              </option>
            )}
          </select>
        </label>
        <label>
          <span>使用账户</span>
          <select
            value={effectiveAccountId ?? ""}
            onChange={(event) => setJoinAccountId(Number(event.target.value))}
          >
            {accounts.length ? (
              accounts.map((account) => (
                <option key={account.id} value={account.id}>
                  {account.displayName} ·{" "}
                  {account.accountType === "MICROSOFT"
                    ? "正版"
                    : account.accountType === "EXTERNAL"
                      ? "外置"
                      : "离线"}
                </option>
              ))
            ) : (
              <option value="" disabled>
                还没有账户
              </option>
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
        <button type="button" disabled={historyBusy} onClick={() => void showHistory()}>
          {historyBusy ? "读取中…" : "联机记录"}
        </button>
      </section>

      {diagnostics ? (
        <section className="diagnostics-panel">
          <div className="diagnostics-head">
            <h2>联机诊断</h2>
            <button type="button" onClick={() => setDiagnostics(undefined)}>
              关闭
            </button>
          </div>
          <pre>{JSON.stringify(diagnostics, null, 2)}</pre>
        </section>
      ) : null}

      {history ? (
        <section className="server-list">
          {history.length ? (
            history.map((entry) => (
              <div className="server-row" key={entry.sessionId}>
                <div className="server-row-main">
                  <strong>
                    {new Date(Number(entry.startedAt) * 1000).toLocaleString()}
                  </strong>
                  <span>
                    {entry.gameVersion} · {entry.loader}
                    {entry.helperVersion ? ` · e4mc ${entry.helperVersion}` : ""}
                  </span>
                  <small>
                    {entry.gotAddress ? "已获得邀请地址" : "未获得邀请地址"}
                    {entry.exitReason
                      ? ` · ${entry.exitReason === "game_exited" ? "游戏退出" : entry.exitReason === "user_stopped" ? "主动结束" : entry.exitReason === "cancelled" ? "已取消" : entry.exitReason}`
                      : ""}
                  </small>
                </div>
                <button type="button" onClick={() => setHistory(undefined)}>
                  收起
                </button>
              </div>
            ))
          ) : (
            <div className="server-empty">
              <div className="server-symbol">◎</div>
              <h2>还没有联机记录</h2>
              <p>创建过联机房间后，这里会显示轻量的时间与结果记录。</p>
            </div>
          )}
        </section>
      ) : null}

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
        {servers.length ? (
          servers.map((server) => {
            const ping = pings[server.id];
            return (
              <div className="server-row" key={server.id}>
                <div className="server-row-main">
                  <strong>{server.name}</strong>
                  <span>
                    {server.address}:{server.port}
                  </span>
                  {server.description ? <small>{server.description}</small> : null}
                </div>
                <div className="server-row-side">
                  {ping?.checking ? (
                    <em className="ping-badge pending">检测中…</em>
                  ) : ping?.result ? (
                    ping.result.reachable ? (
                      <em className="ping-badge ok">
                        在线 · {ping.result.latencyMs} ms
                      </em>
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
                    <button type="button" onClick={() => openEditForm(server)}>
                      编辑
                    </button>
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
          })
        ) : (
          <div className="server-empty">
            <div className="server-symbol">◎</div>
            <h2>还没有服务器</h2>
            <p>
              点击“添加服务器”保存地址；以后启动游戏时会自动带上服务器参数直接加入，不影响单机模式。
            </p>
          </div>
        )}
      </section>
      {message ? (
        <p className="form-message" role="status">
          {message}
        </p>
      ) : null}
    </>
  );
}
