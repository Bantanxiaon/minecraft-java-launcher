import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { AccountsPage } from "./AccountsPage";
import type { Account } from "../types";

const account: Account = {
  id: 1,
  accountType: "OFFLINE",
  displayName: "Steve",
  minecraftUuid: "00000000-0000-0000-0000-000000000001",
  createdAt: "1",
  lastUsedAt: "1",
};

describe("AccountsPage", () => {
  it("creates an offline account with the typed name", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(
      <AccountsPage
        accounts={[]}
        busy={false}
        message=""
        onSelect={() => {}}
        onRemove={() => {}}
        onCreateOffline={onCreate}
        onOpenSettings={() => {}}
      />,
    );
    const input = screen.getByLabelText("离线账户名称");
    fireEvent.change(input, { target: { value: "Alex" } });
    fireEvent.click(screen.getByRole("button", { name: /创建/ }));
    expect(onCreate).toHaveBeenCalledWith("Alex");
  });

  it("selects and removes accounts", () => {
    const onSelect = vi.fn();
    const onRemove = vi.fn();
    render(
      <AccountsPage
        accounts={[account]}
        selectedAccountId={1}
        busy={false}
        message=""
        onSelect={onSelect}
        onRemove={onRemove}
        onCreateOffline={vi.fn().mockResolvedValue(undefined)}
        onOpenSettings={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /移除 Steve/ }));
    expect(onRemove).toHaveBeenCalledWith(account);
  });

  it("shows Microsoft as deferred instead of a fake button", () => {
    render(
      <AccountsPage
        accounts={[]}
        busy={false}
        message=""
        onSelect={() => {}}
        onRemove={() => {}}
        onCreateOffline={vi.fn().mockResolvedValue(undefined)}
        onOpenSettings={() => {}}
      />,
    );
    expect(screen.getByText("后续支持（等待官方授权）")).toBeTruthy();
  });
});
