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
      onQuickJoin={vi.fn()}
    />,
  );
}

describe("ServersPage multiplayer lifecycle", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "multiplayer_state") {
        return { instanceId: 1, state: "IDLE", reconnectCount: 0 };
      }
      if (command === "multiplayer_start") {
        return {
          sessionId: "session-a",
          instanceId: 1,
          state: "READY",
          publicAddress: "play.example.e4mc.link",
          lanPort: 52913,
          reconnectCount: 0,
        };
      }
      if (command === "multiplayer_stop") {
        return { sessionId: "session-a", instanceId: 1, state: "CLOSED", reconnectCount: 0 };
      }
      if (command === "multiplayer_diagnostics") {
        return { sessionId: "session-a", provider: "e4mc", state: "READY" };
      }
      throw new Error(`unexpected ${command}`);
    });
  });

  it("creates a room, shows the invite address, then stops", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "创建房间并启动游戏" }));
    expect(
      await screen.findByText("play.example.e4mc.link"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("multiplayer_start", {
        instanceId: 1,
        accountId: 2,
        javaPath: "C:/java/bin/java.exe",
      }),
    );
    await user.click(
      screen.getByRole("button", { name: "结束联机（将关闭当前游戏）" }),
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("multiplayer_stop", {
        sessionId: "session-a",
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
        onQuickJoin={vi.fn()}
      />,
    );
    await user.click(screen.getByRole("button", { name: "创建房间并启动游戏" }));
    expect(
      await screen.findByText(/未检测到可用的 64 位 Java/),
    ).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith(
      "multiplayer_start",
      expect.anything(),
    );
  });

  it("shows errors when room creation fails", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "multiplayer_state") {
        return { instanceId: 1, state: "IDLE", reconnectCount: 0 };
      }
      if (command === "multiplayer_start") {
        throw new Error("网络不可用");
      }
      return undefined;
    });
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "创建房间并启动游戏" }));
    expect(await screen.findByText(/网络不可用/)).toBeInTheDocument();
  });

  it("validates quick join address and delegates launch", async () => {
    const onQuickJoin = vi.fn();
    const user = userEvent.setup();
    render(
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
        onQuickJoin={onQuickJoin}
      />,
    );
    const input = screen.getByPlaceholderText(/邀请地址/);
    await user.type(input, "evil.example.com");
    await user.click(screen.getByRole("button", { name: "启动并加入" }));
    expect(
      await screen.findByText(/邀请地址格式不正确/),
    ).toBeInTheDocument();
    expect(onQuickJoin).not.toHaveBeenCalled();
    await user.clear(input);
    await user.type(input, "abc.e4mc.link");
    await user.click(screen.getByRole("button", { name: "启动并加入" }));
    await waitFor(() =>
      expect(onQuickJoin).toHaveBeenCalledWith("abc.e4mc.link", 1, 2),
    );
  });

  it("shows diagnostics from a failed session", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "multiplayer_state") {
        return { instanceId: 1, state: "IDLE", reconnectCount: 0 };
      }
      if (command === "multiplayer_start") {
        return {
          sessionId: "session-a",
          instanceId: 1,
          state: "ERROR",
          errorCode: "PROVIDER_UNAVAILABLE",
          userMessage: "e4mc 联机服务出现异常。",
          reconnectCount: 0,
        };
      }
      if (command === "multiplayer_diagnostics") {
        return { sessionId: "session-a", provider: "e4mc", state: "ERROR" };
      }
      return undefined;
    });
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "创建房间并启动游戏" }));
    expect(
      await screen.findByText("e4mc 联机服务出现异常。"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "查看诊断" }));
    expect(await screen.findByText(/联机诊断/)).toBeInTheDocument();
  });
});
