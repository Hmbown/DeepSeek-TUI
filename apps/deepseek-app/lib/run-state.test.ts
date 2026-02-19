import { describe, expect, it } from "vitest";

import { deriveDesktopRunState } from "@/lib/run-state";

describe("deriveDesktopRunState", () => {
  const base = {
    connectionState: "online" as const,
    connectionMessage: "Runtime online",
    pendingApprovalCount: 0,
    activeTurnStatus: null,
    latestTurnStatus: null,
    runningTaskCount: 0,
    latestTaskStatus: null,
  };

  it("prioritizes checking over other states", () => {
    const state = deriveDesktopRunState({
      ...base,
      connectionState: "checking",
      pendingApprovalCount: 3,
      activeTurnStatus: "in_progress",
    });
    expect(state.state).toBe("checking");
  });

  it("returns waiting-approval when approvals are pending", () => {
    const state = deriveDesktopRunState({
      ...base,
      pendingApprovalCount: 2,
    });
    expect(state.state).toBe("waiting-approval");
    expect(state.reason).toContain("2 approval requests");
  });

  it("returns running when turn or tasks are active", () => {
    const state = deriveDesktopRunState({
      ...base,
      activeTurnStatus: "queued",
      runningTaskCount: 1,
    });
    expect(state.state).toBe("running");
  });

  it("maps offline to failed", () => {
    const state = deriveDesktopRunState({
      ...base,
      connectionState: "offline",
      connectionMessage: "Runtime unavailable",
    });
    expect(state.state).toBe("failed");
  });

  it("returns completed when latest turn completed", () => {
    const state = deriveDesktopRunState({
      ...base,
      latestTurnStatus: "completed",
    });
    expect(state.state).toBe("completed");
  });
});
