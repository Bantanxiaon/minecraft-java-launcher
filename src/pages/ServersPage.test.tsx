import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Account, Instance } from "../types";
import { ServersPage } from "./ServersPage";

const invokeMock = vi.fn();
const listenMock = vi.fn(async (_event: string) => () => undefined);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string) => listenMock(event),
}));

const instance: Instance = {
  id: 1,
  name: "QA",
  rootPath: "D:/qa",
  gameVersion: "1.20.1",
  loaderType: "forge",
  memoryMb: 4096,
  status: "ready",
  source: "new",
};

const account: Account = {
  id: 2,
  accountType: "OFFLINE",
  displayName: "Player",
  createdAt: "1",
};

function renderPage() {
  return render(
    <ServersPage
      servers={[]}
      instances={[instance]}
      accounts={[account]}
      selectedInstanceId={1}
      selectedAccountId={2}
      javaPath="C:/java/bin/java.exe"
      busy={false}
      message=""
      onAddServer={vi.fn(async () => undefined)}
      onUpdateServer={vi.fn(async () => undefined)}
      onRemoveServer={vi.fn(async () => undefined)}
      onJoin={vi.fn()}
    />,
  );
}

describe("ServersPage multiplayer lifecycle", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "multiplayer_prepare") return "就绪";
      if (command === "multiplayer_start") {
        return {
          instanceId: 1,
          state: "READY",
          address: "play.example.e4mc.link",
        };
      }
      if (command === "multiplayer_stop") {
        return { instanceId: 1, state: "STOPPED", address: null };
      }
      throw new Error(`unexpected ${command}`);
    });
  });

  it("creates a room, waits for LAN, then stops", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "创建房间并启动游戏" }));
    expect(
      await screen.findByText("play.example.e4mc.link"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("multiplayer_prepare", {
        instanceId: 1,
      }),
    );
    expect(invokeMock).toHaveBeenCalledWith("multiplayer_start", {
      instanceId: 1,
      accountId: 2,
      javaPath: "C:/java/bin/java.exe",
    });
    await user.click(screen.getByRole("button", { name: "结束联机" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("multiplayer_stop", {
        instanceId: 1,
      }),
    );
  });

  it("requires java before starting a room", async () => {
    const user = userEvent.setup();
    render(
      <ServersPage
        servers={[]}
        instances={[instance]}
        accounts={[account]}
        selectedInstanceId={1}
        selectedAccountId={2}
        javaPath=""
        busy={false}
        message=""
        onAddServer={vi.fn(async () => undefined)}
        onUpdateServer={vi.fn(async () => undefined)}
        onRemoveServer={vi.fn(async () => undefined)}
        onJoin={vi.fn()}
      />,
    );
    await user.click(screen.getByRole("button", { name: "创建房间并启动游戏" }));
    expect(
      await screen.findByText(/未检测到可用的 64 位 Java/),
    ).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("shows errors when room creation fails", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "multiplayer_prepare") throw new Error("网络不可用");
      return undefined;
    });
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "创建房间并启动游戏" }));
    expect(await screen.findByText(/网络不可用/)).toBeInTheDocument();
  });
});
