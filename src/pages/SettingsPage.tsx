import type { Account, JavaRuntime, LauncherSettings } from "../types";
import { UpdaterCard } from "../components/UpdaterCard";

type SettingsPageProps = {
  settings: LauncherSettings;
  busy: boolean;
  message: string;
  onChange: (settings: LauncherSettings) => void;
  onSave: () => void;
  onChooseExistingGameDirectory: () => void;
  javaRuntimes: JavaRuntime[];
  selectedJavaPath?: string;
  onSelectJava: (path: string) => void;
  onInstallJava: (major: number) => void;
  onCheckEnvironment: () => void;
  onSetupRecommended: () => void;
  onLoginMicrosoft: () => void;
  microsoftLoginAvailable: boolean;
  accounts: Account[];
  selectedAccountId?: number;
  onSelectAccount: (id: number) => void;
  onRemoveAccount: (account: Account) => void;
  onCleanCache: () => void;
};

export function SettingsPage({
  settings,
  busy,
  message,
  onChange,
  onSave,
  onChooseExistingGameDirectory,
  javaRuntimes,
  selectedJavaPath,
  onSelectJava,
  onInstallJava,
  onCheckEnvironment,
  onSetupRecommended,
  onLoginMicrosoft,
  microsoftLoginAvailable,
  accounts,
  selectedAccountId,
  onSelectAccount,
  onRemoveAccount,
  onCleanCache,
}: SettingsPageProps) {
  return (
    <>
      <header>
        <div>
          <h1>设置</h1>
          <p>下载、Java 内存与启动器行为。</p>
        </div>
        <span className="ready-label">本地设置</span>
      </header>
      <section className="settings-panel">
        <div className="settings-account-list">
          <div><strong>账户</strong><small>Microsoft 凭据保存在 Windows 凭据管理器；离线账户不保存密码。</small></div>
          {accounts.length ? accounts.map((account) => (
            <div className="settings-account-row" key={account.id}>
              <button className={account.id === selectedAccountId ? "selected" : ""} onClick={() => onSelectAccount(account.id)}>
                <span>{account.displayName}</span><small>{account.accountType === "MICROSOFT" ? "Microsoft 正版账户" : "本地离线账户"}</small>
              </button>
              <button className="danger" disabled={busy} onClick={() => onRemoveAccount(account)}>移除</button>
            </div>
          )) : <p>还没有账户。</p>}
        </div>
        <div className="environment-check" role="region" aria-label="运行环境检查">
          <div>
            <strong>运行环境检查</strong>
            <small>
              安装程序会自动补齐显示界面所需的系统组件；这里检查游戏运行需要的 64 位 Java。
            </small>
          </div>
          <div className="managed-java-actions">
            <button type="button" disabled={busy} onClick={onCheckEnvironment}>
              检测全部环境
            </button>
            <button
              className="primary"
              type="button"
              disabled={busy}
              onClick={onSetupRecommended}
            >
              一键安装并验证 Java 21
            </button>
          </div>
        </div>
        <div className="microsoft-login-card">
          <div>
            <strong>Microsoft 正版登录</strong>
            <small>{microsoftLoginAvailable
              ? "点击后会打开微软官方网页。启动器不会看到或保存你的密码。"
              : "暂未开通，不影响离线档案、游戏启动和内容管理。"}</small>
          </div>
          <button className="primary" type="button" disabled={busy || !microsoftLoginAvailable} onClick={onLoginMicrosoft}>
            {microsoftLoginAvailable ? "登录 Microsoft 账户" : "暂未开通"}
          </button>
        </div>
        <UpdaterCard />
        <div className="cache-clean-card">
          <div>
            <strong>存储与缓存</strong>
            <small>
              清理下载缓存和临时文件，释放磁盘空间；不会删除游戏、模组、整合包或存档。
            </small>
          </div>
          <button
            type="button"
            disabled={busy}
            onClick={onCleanCache}
          >
            清理缓存
          </button>
        </div>
        <label>
          <span>
            数据目录<small>按要求固定使用 D 盘</small>
          </span>
          <input disabled value={String.raw`D:\MinecraftLauncherData`} />
        </label>
        <label>
          <span>
            已有游戏目录<small>可选择 PCL、官方启动器或其他启动器正在使用的 .minecraft，SH 只读取并复用完整文件</small>
          </span>
          <div className="directory-picker">
            <input
              value={settings.gameDirectory ?? ""}
              placeholder="没有选择时会自动检查系统默认位置"
              onChange={(event) => onChange({ ...settings, gameDirectory: event.target.value || undefined })}
            />
            <button type="button" disabled={busy} onClick={onChooseExistingGameDirectory}>选择文件夹</button>
          </div>
        </label>
        <label>
          <span>
            下载并发数<small>允许 1–64</small>
          </span>
          <input
            type="number"
            min={1}
            max={64}
            value={settings.downloadConcurrency}
            onChange={(event) =>
              onChange({
                ...settings,
                downloadConcurrency: Number(event.target.value),
              })
            }
          />
        </label>
        <label>
          <span>
            默认内存（MB）<small>允许 2048–65536</small>
          </span>
          <input
            type="number"
            min={2048}
            max={65536}
            step={512}
            value={settings.defaultMemoryMb}
            onChange={(event) =>
              onChange({
                ...settings,
                defaultMemoryMb: Number(event.target.value),
              })
            }
          />
        </label>
        <label>
          <span>
            游戏使用的 Java<small>不同 Minecraft 版本需要不同 Java，启动器会提示</small>
          </span>
          <select
            value={selectedJavaPath ?? ""}
            onChange={(event) => onSelectJava(event.target.value)}
          >
            <option value="" disabled>
              选择 64 位 Java
            </option>
            {javaRuntimes
              .filter((runtime) => runtime.is64Bit)
              .map((runtime) => (
                <option key={runtime.path} value={runtime.path}>
                  Java {runtime.majorVersion ?? runtime.version} · {runtime.vendor}
                </option>
              ))}
          </select>
        </label>
        <div className="managed-java-actions">
          <span>其他游戏版本需要时，可安装经过 SHA-256 校验的官方 OpenJDK：</span>
          {[8, 17, 21, 25].map((major) => (
            <button
              type="button"
              disabled={busy}
              key={major}
              onClick={() => onInstallJava(major)}
            >
              Java {major}
            </button>
          ))}
        </div>
        <label>
          <span>界面语言</span>
          <select
            value={settings.language}
            onChange={(event) =>
              onChange({ ...settings, language: event.target.value })
            }
          >
            <option value="zh-CN">简体中文</option>
            <option value="en-US">English</option>
          </select>
        </label>
        <label className="checkbox-setting">
          <span>启动游戏前自动备份存档<small>每个实例只保留最近 5 份</small></span>
          <input
            type="checkbox"
            checked={settings.backupWorldsBeforeLaunch}
            onChange={(event) =>
              onChange({ ...settings, backupWorldsBeforeLaunch: event.target.checked })
            }
          />
        </label>
        <label className="checkbox-setting">
          <span>游戏启动后关闭启动器</span>
          <input
            type="checkbox"
            checked={settings.closeLauncherAfterGameStart}
            onChange={(event) =>
              onChange({
                ...settings,
                closeLauncherAfterGameStart: event.target.checked,
              })
            }
          />
        </label>
        <button className="primary" disabled={busy} onClick={onSave}>
          {busy ? "保存中…" : "保存设置"}
        </button>
        {message ? (
          <p className="mod-message" role="status">
            {message}
          </p>
        ) : null}
      </section>
    </>
  );
}
