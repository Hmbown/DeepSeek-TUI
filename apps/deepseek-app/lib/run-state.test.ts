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
    queuedTaskCount: 0,
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
    expect(state.reasonSource).toBe("connection");
  });

  it("returns waiting-approval when approvals are pending", () => {
    const state = deriveDesktopRunState({
      ...base,
      pendingApprovalCount: 2,
    });
    expect(state.state).toBe("waiting-approval");
    expect(state.reason).toContain("2 approval requests");
    expect(state.reasonSource).toBe("approvals");
  });

  it("returns running with active-turn source when turn is active", () => {
    const state = deriveDesktopRunState({
      ...base,
      activeTurnStatus: "queued",
      runningTaskCount: 0,
      queuedTaskCount: 0,
    });
    expect(state.state).toBe("running");
    expect(state.reasonSource).toBe("active-turn");
  });

  it("returns running with task-state source when tasks are active", () => {
    const state = deriveDesktopRunState({
      ...base,
      activeTurnStatus: null,
      runningTaskCount: 1,
      queuedTaskCount: 2,
    });
    expect(state.state).toBe("running");
    expect(state.reasonSource).toBe("task-state");
    expect(state.reason).toContain("1 running, 2 queued");
  });

  it("maps offline to failed", () => {
    const state = deriveDesktopRunState({
      ...base,
      connectionState: "offline",
      connectionMessage: "Runtime unavailable",
    });
    expect(state.state).toBe("failed");
    expect(state.reasonSource).toBe("connection");
  });

  it("returns failed from latest task status when no latest turn failure exists", () => {
    const state = deriveDesktopRunState({
      ...base,
      latestTaskStatus: "failed",
    });
    expect(state.state).toBe("failed");
    expect(state.reasonSource).toBe("task-state");
  });

  it("returns completed when latest turn completed", () => {
    const state = deriveDesktopRunState({
      ...base,
      latestTurnStatus: "completed",
    });
    expect(state.state).toBe("completed");
    expect(state.reasonSource).toBe("active-turn");
  });

  it("adds reconnect attempt context to reconnecting reason", () => {
    const state = deriveDesktopRunState({
      ...base,
      connectionState: "reconnecting",
      connectionMessage: "Live stream disconnected",
      reconnectAttempt: 3,
      reconnectDelayMs: 2000,
    });
    expect(state.state).toBe("reconnecting");
    expect(state.reason).toContain("attempt 3");
    expect(state.reason).toContain("2.0s");
    expect(state.reasonSource).toBe("connection");
  });

  it("returns online state for base input", () => {
    const state = deriveDesktopRunState(base);
    expect(state.state).toBe("online");
    expect(state.reasonSource).toBe("connection");
    expect(state.tone).toBe("success");
  });

  it("returns failed when latest turn status is failed", () => {
    const state = deriveDesktopRunState({
      ...base,
      latestTurnStatus: "failed",
    });
    expect(state.state).toBe("failed");
    expect(state.reasonSource).toBe("active-turn");
    expect(state.tone).toBe("danger");
  });

  it("returns completed when latest task status is completed", () => {
    const state = deriveDesktopRunState({
      ...base,
      latestTaskStatus: "completed",
    });
    expect(state.state).toBe("completed");
    expect(state.reasonSource).toBe("task-state");
    expect(state.tone).toBe("success");
  });
});
