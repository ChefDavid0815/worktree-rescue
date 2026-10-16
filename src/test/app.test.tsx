import { render, screen, waitFor, within, fireEvent, cleanup } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { baseHandlers, installBackend, type Handlers } from "./mockBackend";

async function mount(overrides: Handlers = {}) {
  vi.resetModules();
  const calls = installBackend(baseHandlers(overrides));
  const { App } = await import("../app/App");
  const { queryClient } = await import("../lib/queries");
  queryClient.clear();
  queryClient.setDefaultOptions({ queries: { retry: false, staleTime: 0 } });
  render(<App />);
  return calls;
}

beforeEach(() => {
  localStorage.clear();
});
afterEach(() => {
  cleanup();
  vi.doUnmock("@tauri-apps/api/core");
});

describe("shell & overview", () => {
  it("states the most serious condition first and shows the recorder", async () => {
    await mount();
    expect(await screen.findByText(/1 worktree is unreachable/)).toBeInTheDocument();
    expect(await screen.findByRole("group", { name: "Recorder" })).toBeInTheDocument();
    expect(screen.getByText("Protected repositories")).toBeInTheDocument();
    // The critical risk appears in the attention list.
    expect(screen.getAllByText("Unreachable worktree with uncaptured work").length).toBeGreaterThan(0);
  });

  it("navigates with the rail to worktrees and risks", async () => {
    await mount();
    await screen.findByText(/worktree is unreachable/);
    fireEvent.click(screen.getByRole("button", { name: /Worktrees/ }));
    const table = await screen.findByRole("table", { name: "Worktrees" });
    expect(within(table).getByText("agent/cache-ttl")).toBeInTheDocument();
    expect(within(table).getByText("Prunable candidate")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Risks/ }));
    expect(await screen.findByText("Risk Observatory")).toBeInTheDocument();
  });

  it("shows an empty state with demo entry when no repository exists", async () => {
    await mount({ list_projects: () => [] });
    expect(await screen.findByText("Add a repository to start protecting it")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Explore the sandbox demo/ })).toBeInTheDocument();
  });

  it("surfaces backend errors with a retry action", async () => {
    await mount({
      list_worktrees: () => {
        throw { code: "git_not_found", message: "Git executable was not found." };
      },
    });
    expect(await screen.findByText("Git is not installed or could not be found")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("renders in Simplified Chinese when the language setting says so", async () => {
    await mount({
      get_settings: () => ({ ...baseHandlers().get_settings({}) as object, language: "zh-CN" }),
    });
    expect(await screen.findByText(/个工作树不可访问/)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /工作树/ }).length).toBeGreaterThan(0);
  });
});

describe("snapshot & recovery flows", () => {
  it("opens a snapshot from the timeline and starts recovery with a separate folder by default", async () => {
    await mount();
    await screen.findByText(/worktree is unreachable/);
    fireEvent.click(screen.getByRole("button", { name: /Timeline/ }));
    const row = await screen.findAllByText("Manual snapshot");
    fireEvent.click(row[0]);
    const recover = await screen.findByRole("button", { name: "Recover…" });
    fireEvent.click(recover);
    expect(await screen.findByText("Recover files")).toBeInTheDocument();
    const separate = screen.getByRole("radio", { name: /Recover to a separate folder/ });
    expect(separate).toHaveAttribute("aria-checked", "true");
    expect(await screen.findByRole("button", { name: "Recover 2 files" })).toBeEnabled();
  });

  it("requires an explicit acknowledgement before an in-place restore", async () => {
    const calls = await mount({
      preview_restore: () => ({
        id: "pv1",
        snapshotId: "s1",
        worktreePath: "C:\\dev\\orbit-api",
        variant: "workingTree",
        createdAt: Date.now(),
        items: [
          { path: "src/lib/cache.ts", action: "overwrite", blockedReason: null, currentSha256: "1".repeat(64), currentSize: 10, snapshotSha256: "c".repeat(64), snapshotSize: 300 },
        ],
        overwriteCount: 1,
        createCount: 0,
        blockedCount: 0,
      }),
    });
    await screen.findByText(/worktree is unreachable/);
    fireEvent.click(screen.getByRole("button", { name: /Timeline/ }));
    fireEvent.click((await screen.findAllByText("Manual snapshot"))[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Recover…" }));
    fireEvent.click(await screen.findByRole("radio", { name: /Restore selected files in place/ }));
    fireEvent.click(await screen.findByRole("button", { name: /Preview changes for/ }));
    const execute = await screen.findByRole("button", { name: "Restore 1 files" });
    expect(execute).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: /I reviewed the preview/ }));
    await waitFor(() => expect(execute).toBeEnabled());
    expect(calls.some((c) => c.cmd === "execute_restore")).toBe(false);
  });
});
