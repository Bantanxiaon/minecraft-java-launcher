import type { Account } from "../../types";
import { AccountsPage as LegacyAccountsPage } from "../../pages/AccountsPage";

export type AccountsPageProps = {
  accounts: Account[];
  selectedAccountId?: number;
  busy: boolean;
  message: string;
  onSelect: (accountId: number) => void;
  onRemove: (account: Account) => void;
  onCreateOffline: (name: string) => Promise<void>;
  onOpenSettings: () => void;
};

export function AccountsPage(props: AccountsPageProps) {
  return (
    <div className="ui3-page-enter">
      <LegacyAccountsPage {...props} />
    </div>
  );
}
