import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Account, LauncherSettings } from "../types";
import { SettingsPage } from "./SettingsPage";

const settings: LauncherSettings = {
  language: "zh-CN",
  downloadConcurrency: 8,
  closeLauncherAfterGameStart: false,
  defaultMemoryMb: 4096,
  backupWorldsBeforeLaunch: true,
  uiTheme: "modern",
};

function renderPage(overrides?: {
  message?: string;
  onChange?: (settings: LauncherSettings) => void;
  accounts?: Account[];
  selectedAccountId?: number;
  onSelectAccount?: (accountId: number) => void;
  onRemoveAccount?: (account: Account) => void;
}) {
  const onChange = overrides?.onChange ?? vi.fn();
  return render(
    <SettingsPage
      settings={settings}
      busy={false}
      message={overrides?.message ?? ""}
      onChange={onChange}
      onSave={vi.fn()}
      onChooseExistingGameDirectory={vi.fn()}
      javaRuntimes={[]}
      onSelectJava={vi.fn()}
      onInstallJava={vi.fn()}
      onCheckEnvironment={vi.fn()}
      onSetupRecommended={vi.fn()}
      onLoginMicrosoft={vi.fn()}
      onLoginExternal={vi.fn(async () => undefined)}
      microsoftLoginAvailable={false}
      accounts={overrides?.accounts ?? []}
      selectedAccountId={overrides?.selectedAccountId}
      onSelectAccount={overrides?.onSelectAccount ?? vi.fn()}
      onRemoveAccount={overrides?.onRemoveAccount ?? vi.fn()}
      onCleanCache={vi.fn()}
    />,
  );
}

describe("SettingsPage", () => {
  it("propagates settings changes through onChange", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderPage({ onChange });
    const checkbox = screen.getByRole("checkbox", {
      name: /游戏启动后关闭启动器/,
    });
    await user.click(checkbox);
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ closeLauncherAfterGameStart: true }),
    );
  });

  it("renders error message for recovery feedback", () => {
    renderPage({ message: "Java 安装失败：网络不可用" });
    expect(screen.getByText(/Java 安装失败/)).toBeInTheDocument();
  });

  it("selects accounts by immutable account id and keys rows by id", async () => {
    const user = userEvent.setup();
    const accounts: Account[] = [
      {
        id: 7,
        accountType: "OFFLINE",
        displayName: "Steve",
        minecraftUuid: "5627dd98-e6be-3c21-b8a8-e92344183641",
        createdAt: "1",
      },
      {
        id: 9,
        accountType: "OFFLINE",
        displayName: "steve",
        minecraftUuid: "53909932-f794-33c0-9329-948045a4c1ce",
        createdAt: "1",
      },
    ];
    const onSelectAccount = vi.fn();
    renderPage({ accounts, selectedAccountId: 7, onSelectAccount });
    const buttons = screen.getAllByRole("button", { name: /Steve|steve/ });
    expect(buttons).toHaveLength(2);
    await user.click(buttons[1]);
    expect(onSelectAccount).toHaveBeenCalledWith(9);
    // 仅大小写不同的名字仍必须按 id 渲染为两个独立身份，不得以 index 或名字作 key 合并。
    expect(buttons[0]).not.toBe(buttons[1]);
  });

  it("removes the exact account object selected by id", async () => {
    const user = userEvent.setup();
    const account: Account = {
      id: 42,
      accountType: "OFFLINE",
      displayName: "RemoveMe",
      createdAt: "1",
    };
    const onRemoveAccount = vi.fn();
    renderPage({ accounts: [account], onRemoveAccount });
    await user.click(screen.getByRole("button", { name: "移除" }));
    expect(onRemoveAccount).toHaveBeenCalledWith(
      expect.objectContaining({ id: 42 }),
    );
  });
});
