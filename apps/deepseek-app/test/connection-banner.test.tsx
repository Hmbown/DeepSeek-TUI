import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ConnectionBanner } from "@/components/chat/ConnectionBanner";

describe("ConnectionBanner", () => {
  it("renders reconnecting state actions", () => {
    const retry = vi.fn();
    const openSettings = vi.fn();

    render(
      <ConnectionBanner
        state="reconnecting"
        message="Live stream disconnected"
        runState={{
          state: "reconnecting",
          tone: "warning",
          label: "Reconnecting stream",
          reason: "Live stream disconnected",
          reasonSource: "connection",
        }}
        baseUrl="http://127.0.0.1:7878"
        onRetryNow={retry}
        onOpenSettings={openSettings}
      />
    );

    expect(screen.getByText("Reconnecting stream")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /retry now/i }));
    fireEvent.click(screen.getByRole("button", { name: /runtime:/i }));
    expect(retry).toHaveBeenCalledTimes(1);
    expect(openSettings).toHaveBeenCalledTimes(1);
  });

  it("does not render while fully online and idle", () => {
    const { container } = render(
      <ConnectionBanner
        state="online"
        message="Runtime online"
        runState={{
          state: "online",
          tone: "success",
          label: "Runtime online",
          reason: "Runtime online",
          reasonSource: "connection",
        }}
        baseUrl="http://127.0.0.1:7878"
        onRetryNow={vi.fn()}
        onOpenSettings={vi.fn()}
      />
    );
    expect(container.textContent).toBe("");
  });

  it("renders waiting approval state", () => {
    render(
      <ConnectionBanner
        state="online"
        message="Runtime online"
        runState={{
          state: "waiting-approval",
          tone: "warning",
          label: "Waiting for approval",
          reason: "1 approval request pending",
          reasonSource: "approvals",
        }}
        baseUrl="http://127.0.0.1:7878"
        onRetryNow={vi.fn()}
        onOpenSettings={vi.fn()}
      />
    );

    expect(screen.getByText("Waiting for approval")).toBeInTheDocument();
    expect(screen.getByText("1 approval request pending")).toBeInTheDocument();
  });

  it("renders offline/failed state", () => {
    render(
      <ConnectionBanner
        state="offline"
        message="Runtime unavailable"
        runState={{
          state: "failed",
          tone: "danger",
          label: "Runtime offline",
          reason: "Runtime unavailable",
          reasonSource: "connection",
        }}
        baseUrl="http://127.0.0.1:7878"
        onRetryNow={vi.fn()}
        onOpenSettings={vi.fn()}
      />
    );

    expect(screen.getByText("Runtime offline")).toBeInTheDocument();
  });
});
