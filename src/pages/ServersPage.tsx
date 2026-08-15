import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Account, Instance, ServerEntry, ServerPing } from "../types";

type ServersPageProps = {
  servers: ServerEntry[];
  instances: Instance[];
  accounts: Account[];
  selectedInstanceId?: number;
  selectedAccountId?: number;
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

  return (
    <>
      <header>
        <div>
          <h1>服务器</h1>
          <p>保存常用服务器地址，一键启动游戏并自动加入；支持外置登录服务器。</p>
        </div>
        <span className="ready-label">联机已开通</span>
      </header>

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
