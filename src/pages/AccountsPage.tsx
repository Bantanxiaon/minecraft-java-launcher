import { useState } from "react";
import { CircleUserRound, Plus, Trash2, UserPlus } from "lucide-react";
import type { Account } from "../types";
import { currentLocale, t } from "../i18n";

type AccountsPageProps = {
  accounts: Account[];
  selectedAccountId?: number;
  busy: boolean;
  message: string;
  onSelect: (accountId: number) => void;
  onRemove: (account: Account) => void;
  onCreateOffline: (name: string) => Promise<void>;
  onOpenSettings: () => void;
};

export function AccountsPage({
  accounts,
  selectedAccountId,
  busy,
  message,
  onSelect,
  onRemove,
  onCreateOffline,
  onOpenSettings,
}: AccountsPageProps) {
  const [name, setName] = useState("");
  const locale = currentLocale();

  return (
    <div className="accounts-page">
      <header>
        <div>
          <h1>{t("accounts.title", locale)}</h1>
          <p>{t("accounts.subtitle", locale)}</p>
        </div>
      </header>

      {message ? <p className="form-message">{message}</p> : null}

      <section className="accounts-list">
        <h2>已保存账户</h2>
        {accounts.length === 0 ? (
          <div className="empty-state">
            <CircleUserRound size={26} />
            <p>{t("accounts.empty", locale)}</p>
          </div>
        ) : (
          accounts.map((account) => {
            const active = account.id === selectedAccountId;
            return (
              <div
                key={account.id}
                className={`account-row ${active ? "active" : ""}`}
              >
                <div className="account-avatar">
                  {account.displayName[0]?.toUpperCase() ?? "?"}
                </div>
                <div className="account-copy">
                  <strong>{account.displayName}</strong>
                  <small>
                    {account.accountType === "MICROSOFT"
                      ? "Microsoft 正版账户"
                      : account.accountType === "EXTERNAL"
                        ? "外置登录账户"
                        : "本地离线账户"}
                  </small>
                </div>
                <div className="account-actions">
                  <button
                    className={active ? "primary" : "quiet"}
                    type="button"
                    disabled={active || busy}
                    onClick={() => onSelect(account.id)}
                  >
                    {active ? "当前" : "使用"}
                  </button>
                  <button
                    className="danger-quiet"
                    type="button"
                    disabled={busy}
                    aria-label={`移除 ${account.displayName}`}
                    onClick={() => onRemove(account)}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              </div>
            );
          })
        )}
      </section>

      <section className="account-create">
        <h2>{t("accounts.addOffline", locale)}</h2>
        <div className="account-create-row">
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="玩家名称（3–16 位字母/数字/下划线）"
            aria-label="离线账户名称"
            maxLength={16}
          />
          <button
            className="primary"
            type="button"
            disabled={busy || name.trim().length < 3}
            onClick={() => void onCreateOffline(name).then(() => setName(""))}
          >
            <UserPlus size={16} /> {t("accounts.create", locale)}
          </button>
        </div>
        <p className="account-hint">{t("accounts.offlineHint", locale)}</p>
      </section>

      <section className="account-providers">
        <h2>登录方式</h2>
        <div className="account-provider-row">
          <span>外置登录（Yggdrasil / LittleSkin / ely.by）</span>
          <button type="button" className="quiet" onClick={onOpenSettings}>
            <Plus size={16} /> {t("accounts.openSettings", locale)}
          </button>
        </div>
        <div className="account-provider-row deferred">
          <span>Microsoft 正版登录</span>
          <small>{t("accounts.microsoftDeferred", locale)}</small>
        </div>
      </section>
    </div>
  );
}
