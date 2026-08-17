import { useState } from "react";
import { Info } from "lucide-react";
import type { ReactNode } from "react";
import type { Account, JavaRuntime, LauncherSettings } from "../../types";
import { UpdaterCard } from "../../components/UpdaterCard";
import { StoragePage } from "../../pages/StoragePage";
import { Badge, Button, Segmented, Tabs } from "../../ui/components";
import { SETTINGS_TABS } from "../../app/Router";
import type { SettingsTab } from "../../app/Router";
import type { ThemeMode } from "../../app/providers";

export type SettingsPageProps = {
  tab: SettingsTab;
  onTab: (tab: SettingsTab) => void;
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
  onLoginExternal: (
    apiRoot: string,
    username: string,
    password: string,
  ) => Promise<void>;
  microsoftLoginAvailable: boolean;
  accounts: Account[];
  selectedAccountId?: number;
  onSelectAccount: (id: number) => void;
  onRemoveAccount: (account: Account) => void;
  onCleanCache: () => void;
  onExportDiagnostics: () => void;
  themeMode: ThemeMode;
  onThemeMode: (mode: ThemeMode) => void;
  version: string;
  channelLabel: string;
};

function SettingRow({
  title,
  description,
  control,
}: {
  title: string;
  description: string;
  control: ReactNode;
}) {
  return (
    <div className="settings-row">
      <div className="settings-row-copy">
        <strong>{title}</strong>
        <small>{description}</small>
      </div>
      <div className="settings-row-control">{control}</div>
    </div>
  );
}

function SaveBar({ busy, onSave }: { busy: boolean; onSave: () => void }) {
  return (
    <div className="settings-save-bar">
      <Button variant="primary" disabled={busy} onClick={onSave}>
        {busy ? "保存中…" : "保存更改"}
      </Button>
    </div>
  );
}

export function SettingsPage({
  tab,
  onTab,
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
  onLoginExternal,
  microsoftLoginAvailable,
  accounts,
  selectedAccountId,
  onSelectAccount,
  onRemoveAccount,
  onCleanCache,
  onExportDiagnostics,
  themeMode,
  onThemeMode,
  version,
  channelLabel,
}: SettingsPageProps) {
  const [externalOpen, setExternalOpen] = useState(false);
  const [externalApiRoot, setExternalApiRoot] = useState("");
  const [externalUsername, setExternalUsername] = useState("");
  const [externalPassword, setExternalPassword] = useState("");
  const [externalBusy, setExternalBusy] = useState(false);
  const [downloadMode, setDownloadMode] = useState<
    "auto" | "robust" | "performance" | "custom"
  >(
    settings.downloadConcurrency <= 4
      ? "robust"
      : settings.downloadConcurrency >= 32
        ? "performance"
        : settings.downloadConcurrency === 16
          ? "auto"
          : "custom",
  );

  async function submitExternalLogin() {
    if (
      !externalApiRoot.trim() ||
      !externalUsername.trim() ||
      !externalPassword
    ) {
      return;
    }
    setExternalBusy(true);
    try {
      await onLoginExternal(
        externalApiRoot.trim(),
        externalUsername.trim(),
        externalPassword,
      );
      setExternalOpen(false);
      setExternalApiRoot("");
      setExternalUsername("");
      setExternalPassword("");
    } catch {
      // 错误信息由 App 统一显示
    } finally {
      setExternalBusy(false);
    }
  }

  function changeDownloadMode(
    mode: "auto" | "robust" | "performance" | "custom",
  ) {
    setDownloadMode(mode);
    const concurrency =
      mode === "auto"
        ? 16
        : mode === "robust"
          ? 4
          : mode === "performance"
            ? 32
            : settings.downloadConcurrency;
    onChange({ ...settings, downloadConcurrency: concurrency });
  }

  return (
    <div className="ui3-page-enter">
      <header className="ui3-page-header">
        <div>
          <h1>设置</h1>
          <p>下载、Java、存储与启动器行为。</p>
        </div>
        <Badge>本地设置</Badge>
      </header>
      <Tabs
        tabs={SETTINGS_TABS}
        value={tab}
        onChange={onTab}
        label="设置分类"
      />
      <div className="settings-tab-page">
        {tab === "general" ? (
          <section className="settings-panel">
            <div className="settings-account-list">
              <div>
                <strong>账户</strong>
                <small>
                  Microsoft 凭据保存在 Windows 凭据管理器；离线账户不保存密码。
                </small>
              </div>
              {accounts.map((account) => (
                <div className="settings-account-row" key={account.id}>
                  <button
                    type="button"
                    className={
                      account.id === selectedAccountId ? "selected" : ""
                    }
                    onClick={() => onSelectAccount(account.id)}
                  >
                    <span>{account.displayName}</span>
                    <small>
                      {account.accountType === "MICROSOFT"
                        ? "Microsoft 正版账户"
                        : account.accountType === "EXTERNAL"
                          ? "外置登录账户（authlib-injector）"
                          : "本地离线账户"}
                    </small>
                  </button>
                  <Button
                    variant="danger-quiet"
                    disabled={busy}
                    onClick={() => onRemoveAccount(account)}
                  >
                    移除
                  </Button>
                </div>
              ))}
            </div>

            <SettingRow
              title="外置登录"
              description="支持 LittleSkin、自建皮肤站等 authlib-injector 登录。首次需要联网下载一次组件。"
              control={
                externalOpen ? (
                  <div className="external-login-form">
                    <input
                      value={externalApiRoot}
                      onChange={(event) =>
                        setExternalApiRoot(event.target.value)
                      }
                      placeholder="外置登录地址"
                    />
                    <input
                      value={externalUsername}
                      onChange={(event) =>
                        setExternalUsername(event.target.value)
                      }
                      placeholder="用户名"
                    />
                    <input
                      type="password"
                      value={externalPassword}
                      onChange={(event) =>
                        setExternalPassword(event.target.value)
                      }
                      placeholder="密码"
                      onKeyDown={(event) => {
                        if (event.key === "Enter") void submitExternalLogin();
                      }}
                    />
                    <div className="external-login-actions">
                      <Button
                        variant="primary"
                        disabled={externalBusy || busy}
                        onClick={() => void submitExternalLogin()}
                      >
                        {externalBusy ? "登录中…" : "登录"}
                      </Button>
                      <Button
                        disabled={externalBusy}
                        onClick={() => setExternalOpen(false)}
                      >
                        取消
                      </Button>
                    </div>
                  </div>
                ) : (
                  <Button
                    variant="primary"
                    disabled={busy}
                    onClick={() => setExternalOpen(true)}
                  >
                    添加外置登录
                  </Button>
                )
              }
            />

            <SettingRow
              title="Microsoft 正版登录"
              description={
                microsoftLoginAvailable
                  ? "点击后打开微软官方网页；启动器不会看到或保存你的密码。"
                  : "尚未开放，不影响离线档案、游戏启动和内容管理。"
              }
              control={
                microsoftLoginAvailable ? (
                  <Button
                    variant="primary"
                    disabled={busy}
                    onClick={onLoginMicrosoft}
                  >
                    登录 Microsoft 账户
                  </Button>
                ) : (
                  <Badge tone="warning">尚未开放</Badge>
                )
              }
            />

            <SettingRow
              title="数据目录"
              description="按要求固定使用 D 盘"
              control={
                <input
                  disabled
                  value={String.raw`D:\MinecraftLauncherData`}
                />
              }
            />

            <SettingRow
              title="已有游戏目录"
              description="可选择 PCL、官方启动器或其他启动器正在使用的 .minecraft，SH 只读取并复用完整文件"
              control={
                <div className="directory-picker">
                  <input
                    value={settings.gameDirectory ?? ""}
                    placeholder="没有选择时会自动检查"
                    onChange={(event) =>
                      onChange({
                        ...settings,
                        gameDirectory: event.target.value || undefined,
                      })
                    }
                  />
                  <Button
                    disabled={busy}
                    onClick={onChooseExistingGameDirectory}
                  >
                    选择文件夹
                  </Button>
                </div>
              }
            />

            <SettingRow
              title="启动游戏前自动备份存档"
              description="每个实例只保留最近 5 份"
              control={
                <input
                  type="checkbox"
                  checked={settings.backupWorldsBeforeLaunch}
                  onChange={(event) =>
                    onChange({
                      ...settings,
                      backupWorldsBeforeLaunch: event.target.checked,
                    })
                  }
                />
              }
            />

            <SettingRow
              title="游戏启动后关闭启动器"
              description="启动器会保持后台直到游戏退出，确保记录与崩溃分析完整"
              control={
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
              }
            />

            <SettingRow
              title="界面语言"
              description="当前仅提供简体中文；英文界面将在完整翻译后开放"
              control={
                <select disabled value="zh-CN">
                  <option value="zh-CN">简体中文</option>
                </select>
              }
            />

            <SettingRow
              title="界面主题"
              description="深色为第一设计基准，支持浅色与跟随系统"
              control={
                <Segmented
                  label="界面主题"
                  value={themeMode}
                  onChange={onThemeMode}
                  options={[
                    { id: "dark", label: "深色" },
                    { id: "light", label: "浅色" },
                    { id: "system", label: "跟随系统" },
                  ]}
                />
              }
            />

            <SaveBar busy={busy} onSave={onSave} />
          </section>
        ) : null}

        {tab === "game" ? (
          <section className="settings-panel">
            <SettingRow
              title="运行环境检查"
              description="检查游戏运行需要的 64 位 Java 与系统组件"
              control={
                <div className="managed-java-actions">
                  <Button disabled={busy} onClick={onCheckEnvironment}>
                    检测全部环境
                  </Button>
                  <Button
                    variant="primary"
                    disabled={busy}
                    onClick={onSetupRecommended}
                  >
                    一键安装并验证 Java 21
                  </Button>
                </div>
              }
            />
            <SettingRow
              title="游戏使用的 Java"
              description="不同 Minecraft 版本需要不同 Java，启动器会提示"
              control={
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
                        Java {runtime.majorVersion ?? runtime.version} ·{" "}
                        {runtime.vendor}
                      </option>
                    ))}
                </select>
              }
            />
            <SettingRow
              title="默认内存（MB）"
              description="允许 2048–65536"
              control={
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
              }
            />
            <div className="managed-java-actions">
              <span className="ui3-muted">
                其他游戏版本需要时，可安装经过 SHA-256 校验的官方 OpenJDK：
              </span>
              {[8, 17, 21, 25].map((major) => (
                <Button
                  key={major}
                  disabled={busy}
                  onClick={() => onInstallJava(major)}
                >
                  Java {major}
                </Button>
              ))}
            </div>
            <SaveBar busy={busy} onSave={onSave} />
          </section>
        ) : null}

        {tab === "download" ? (
          <section className="settings-panel">
            <SettingRow
              title="下载模式"
              description="普通用户无需直接面对并发数；选择模式后自动应用"
              control={
                <Segmented
                  label="下载模式"
                  value={downloadMode}
                  onChange={changeDownloadMode}
                  options={[
                    { id: "auto", label: "自动" },
                    { id: "robust", label: "稳健" },
                    { id: "performance", label: "高性能" },
                    { id: "custom", label: "自定义" },
                  ]}
                />
              }
            />
            {downloadMode === "custom" ? (
              <SettingRow
                title="下载并发数"
                description="允许 1–64；仅自定义模式可调整"
                control={
                  <input
                    type="number"
                    min={1}
                    max={64}
                    value={settings.downloadConcurrency}
                    onChange={(event) => {
                      const value = Number(event.target.value);
                      onChange({
                        ...settings,
                        downloadConcurrency: value,
                      });
                    }}
                  />
                }
              />
            ) : (
              <SettingRow
                title="当前并发"
                description="由下载模式自动决定，切换模式后自动应用"
                control={
                  <input
                    disabled
                    value={`${settings.downloadConcurrency} 个连接`}
                  />
                }
              />
            )}
            <p className="ui3-muted">
              下载任务会自动校验 SHA-1/SHA-256，失败时自动重试并给出恢复动作；
              普通 Minecraft、Modrinth 内容下载属于正常联网行为。
            </p>
            <SaveBar busy={busy} onSave={onSave} />
          </section>
        ) : null}

        {tab === "storage" ? (
          <section className="settings-panel">
            <StoragePage />
            <SettingRow
              title="清理缓存"
              description="清理下载缓存和临时文件，释放磁盘空间；不会删除游戏、模组、整合包或存档"
              control={
                <Button disabled={busy} onClick={onCleanCache}>
                  清理缓存
                </Button>
              }
            />
          </section>
        ) : null}

        {tab === "update" ? (
          <section className="settings-panel">
            <UpdaterCard />
            <p className="ui3-muted">
              更新检查优先走国内 CDN，自动验证签名后安装。当前版本 v{version}（
              {channelLabel}）。
            </p>
          </section>
        ) : null}

        {tab === "advanced" ? (
          <section className="settings-panel">
            <SettingRow
              title="诊断报告"
              description="导出脱敏后的下载、崩溃与运行环境信息，便于排查问题"
              control={
                <Button disabled={busy} onClick={onExportDiagnostics}>
                  导出脱敏报告
                </Button>
              }
            />
            <SettingRow
              title="关于"
              description={`SH Launcher v${version} · ${channelLabel} · 数据目录 D:\\MinecraftLauncherData`}
              control={<Info size={18} className="ui3-muted" />}
            />
            <SaveBar busy={busy} onSave={onSave} />
          </section>
        ) : null}
        {message ? (
          <p className="mod-message" role="status">
            {message}
          </p>
        ) : null}
      </div>
    </div>
  );
}
