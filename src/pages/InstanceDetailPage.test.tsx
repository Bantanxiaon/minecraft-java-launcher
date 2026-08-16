import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Instance } from "../types";
import { InstanceDetailPage } from "./InstanceDetailPage";

const invokeMock = vi.fn();
const { openMock } = vi.hoisted(() => ({ openMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: openMock,
}));

const instance: Instance = {
  id: 1,
  name: "QA Pack",
  rootPath: "D:/tmp/qa",
  gameVersion: "1.20.1",
  loaderType: "forge",
  memoryMb: 4096,
  status: "ready",
  source: "modrinth",
};

function renderPage() {
  return render(
    <InstanceDetailPage
      instance={instance}
      javaLabel="Java 17 · 64 位"
      onBack={vi.fn()}
      onLaunch={vi.fn()}
      onRepair={vi.fn()}
      onOpenFolder={vi.fn()}
      onMemoryChange={vi.fn()}
    />,
  );
}

describe("InstanceDetailPage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    invokeMock.mockReset();
    openMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "instance_health") {
        return {
          instanceId: 1,
          name: "QA Pack",
          gameVersion: "1.20.1",
          loaderType: "forge",
          loaderVersion: null,
          memoryMb: 4096,
          status: "ready",
          gameFilesOk: true,
          modCount: 3,
          missingDependencies: [],
          incompatibleMods: [],
        };
      }
      if (command === "list_content_items") {
        return [
          {
            id: 10,
            instanceId: 1,
            kind: "mod",
            fileName: "jei-1.20.1.jar",
            hash: "abc",
            metadataJson: JSON.stringify({
              name: "JEI",
              modId: "jei",
              version: "15.2.0",
              modrinthProjectId: "u6dRKJwZ",
            }),
            enabled: true,
            source: "modrinth",
            installedAt: "1",
          },
          {
            id: 11,
            instanceId: 1,
            kind: "world",
            fileName: "My World",
            hash: "def",
            enabled: true,
            source: "local",
            installedAt: "1",
          },
        ];
      }
      if (command === "reconcile_scan") {
        return {
          instanceId: 1,
          dbMissingOnDisk: [],
          diskMissingInDb: [],
          duplicateGroups: [
            {
              sha256: "aaabbbcc",
              files: ["a.jar", "b.jar"],
              keep: "a.jar",
              removableBytes: 1048576,
            },
          ],
          fingerprint: "fp-1",
        };
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  it("renders health overview and reconcile drill-down", async () => {
    const user = userEvent.setup();
    renderPage();
    expect(await screen.findByText("✓ 游戏文件完整")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "对账" }));
    expect(await screen.findByText(/完全重复组：1/)).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("reconcile_scan", {
      instanceId: 1,
    });
  });

  it("applies reconcile with the scanned fingerprint", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "对账" }));
    await screen.findByText(/完全重复组：1/);
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "reconcile_apply") {
        return {
          addedRecords: 0,
          removedStaleRecords: 0,
          deduplicatedFiles: 1,
          freedBytes: 1048576,
        };
      }
      return undefined;
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await user.click(screen.getByRole("button", { name: "应用对账" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("reconcile_apply", {
        instanceId: 1,
        fingerprint: "fp-1",
      }),
    );
  });

  it("drills into content with friendly names and technical file names", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "内容" }));
    expect(await screen.findByText("JEI")).toBeInTheDocument();
    expect(screen.getByText(/jei-1.20.1.jar/)).toBeInTheDocument();
    expect(screen.getByText("My World")).toBeInTheDocument();
  });

  it("removes content to recoverable backup after confirmation", async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "instance_health") {
        return {
          instanceId: 1,
          name: "QA Pack",
          gameVersion: "1.20.1",
          loaderType: "forge",
          loaderVersion: null,
          memoryMb: 4096,
          status: "ready",
          gameFilesOk: true,
          modCount: 1,
          missingDependencies: [],
          incompatibleMods: [],
        };
      }
      if (command === "list_content_items")
        return [
          {
            id: 10,
            instanceId: 1,
            kind: "mod",
            fileName: "jei-1.20.1.jar",
            hash: "abc",
            metadataJson: JSON.stringify({ name: "JEI" }),
            enabled: true,
            source: "modrinth",
            installedAt: "1",
          },
        ];
      if (command === "remove_mod_to_backup")
        return { id: 10, backupPath: "D:/backup/jei.jar" };
      throw new Error(`unexpected ${command}`);
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    renderPage();
    await user.click(screen.getByRole("button", { name: "内容" }));
    await screen.findByText("JEI");
    await user.click(screen.getAllByRole("button", { name: "移除" })[0]);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("remove_mod_to_backup", {
        contentId: 10,
      }),
    );
  });

  it("shows health errors instead of crashing", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "instance_health") throw new Error("磁盘不可用");
      return undefined;
    });
    renderPage();
    expect(await screen.findByText(/磁盘不可用/)).toBeInTheDocument();
  });

  it("renders modpack update plan and surfaces protected user files", async () => {
    const user = userEvent.setup();
    openMock.mockResolvedValue("D:/pack-new.mrpack");
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "instance_health") {
        return {
          instanceId: 1,
          name: "QA Pack",
          gameVersion: "1.20.1",
          loaderType: "forge",
          loaderVersion: null,
          memoryMb: 4096,
          status: "ready",
          gameFilesOk: true,
          modCount: 1,
          missingDependencies: [],
          incompatibleMods: [],
        };
      }
      if (command === "update_modrinth_modpack") {
        return {
          instanceId: 1,
          packVersion: "2.0.0",
          installs: [],
          updates: ["mods/jei-1.20.1.jar"],
          removals: [],
          dependencyChanges: [],
          conflicts: ["配置文件已被用户修改，保留现有版本。"],
          protectedUserFiles: ["config/x.toml"],
        };
      }
      if (command === "list_content_items") return [];
      throw new Error(`unexpected ${command}`);
    });
    renderPage();
    await user.click(
      await screen.findByRole("button", { name: "更新整合包" }),
    );
    expect(await screen.findByText("整合包更新计划")).toBeInTheDocument();
    expect(screen.getByText("更新：mods/jei-1.20.1.jar")).toBeInTheDocument();
    expect(
      screen.getByText(/已保护 1 个用户文件/),
    ).toBeInTheDocument();
  });
});
