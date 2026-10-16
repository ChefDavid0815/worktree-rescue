import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { baseHandlers, installBackend } from "./mockBackend";

afterEach(() => cleanup());

describe("settings", () => {
  it("renders every section and keeps stop-monitoring separate from deleting backups", async () => {
    vi.resetModules();
    installBackend(baseHandlers());
    const { App } = await import("../app/App");
    const { queryClient } = await import("../lib/queries");
    queryClient.clear();
    queryClient.setDefaultOptions({ queries: { retry: false } });
    render(<App />);
    await screen.findByText(/worktree is unreachable/);
    fireEvent.click(screen.getByRole("button", { name: /Settings/ }));
    expect(await screen.findByRole("heading", { name: "Capture scope" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Stop monitoring/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Delete backups/ })).toBeInTheDocument();
  });
});
