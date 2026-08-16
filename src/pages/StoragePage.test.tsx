import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StoragePage } from "./StoragePage";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

describe("StoragePage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_storage_overview") {
        return {
          totalBytes: 20 * 1024 ** 3,
          reclaimableBytes: 3 * 1024 ** 3,
          categories: [
            { category: "DOWNLOAD_CACHE", itemCount: 12, bytes: 2 * 1024 ** 3 },
            { category: "LOG", itemCount: 40, bytes: 1024 ** 3 },
          ],
        };
      }
      if (command === "list_deleted_instances") {
        return [
          {
            id: "del-1",
            originalInstanceId: 5,
            displayName: "Old Pack",
            backupPath: "D:/backup/5",
            sizeBytes: 1024 ** 3,
            deletedAt: "1",
          },
        ];
      }
      if (command === "build_safe_cleanup_plan") {
        return { fingerprint: "fp-clean", reclaimableBytes: 3 * 1024 ** 3 };
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  it("shows overview and executes cleanup with fingerprint after confirm", async () => {
    const user = userEvent.setup();
    render(<StoragePage />);
    expect(await screen.findByText("20.00 GB")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "预览安全清理" }));
    expect(await screen.findByText(/预计释放 3.00 GB/)).toBeInTheDocument();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_storage_overview") {
        return {
          totalBytes: 20 * 1024 ** 3,
          reclaimableBytes: 0,
          categories: [],
        };
      }
      if (command === "list_deleted_instances") {
        return [
          {
            id: "del-1",
            originalInstanceId: 5,
            displayName: "Old Pack",
            backupPath: "D:/backup/5",
            sizeBytes: 1024 ** 3,
            deletedAt: "1",
          },
        ];
      }
      if (command === "execute_cleanup_plan") {
        return { freedBytes: 3 * 1024 ** 3, removedItems: 52 };
      }
      return undefined;
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await user.click(screen.getByRole("button", { name: /确认清理 3.00 GB/ }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("execute_cleanup_plan", {
        fingerprint: "fp-clean",
      }),
    );
  });

  it("restores a deleted instance", async () => {
    const user = userEvent.setup();
    render(<StoragePage />);
    await screen.findByText("Old Pack");
    await user.click(screen.getByRole("button", { name: "恢复" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("restore_deleted_instance", {
        id: "del-1",
      }),
    );
  });

  it("requires double confirmation for permanent delete", async () => {
    const user = userEvent.setup();
    const confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<StoragePage />);
    await screen.findByText("Old Pack");
    await user.click(screen.getByRole("button", { name: "永久删除" }));
    expect(confirmMock).toHaveBeenCalledTimes(2);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "permanently_delete_instance_backup",
        { id: "del-1" },
      ),
    );
  });

  it("surfaces backend errors without losing the page", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_storage_overview") throw new Error("磁盘读取失败");
      if (command === "list_deleted_instances") return [];
      return undefined;
    });
    render(<StoragePage />);
    expect(await screen.findByText(/磁盘读取失败/)).toBeInTheDocument();
  });
});
