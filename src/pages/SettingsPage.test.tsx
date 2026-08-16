import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LauncherSettings } from "../types";
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
      accounts={[]}
      onSelectAccount={vi.fn()}
      onRemoveAccount={vi.fn()}
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
});
