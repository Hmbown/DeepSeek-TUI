import type { ConnectionState } from "@/hooks/use-runtime-connection";
import type { TaskStatus, RuntimeTurnStatus } from "@/lib/runtime-api";

export type DesktopRunState =
  | "idle"
  | "checking"
  | "online"
  | "reconnecting"
  | "running"
  | "waiting-approval"
  | "failed"
  | "completed";

export type DesktopRunTone = "neutral" | "warning" | "danger" | "success";

export type DesktopRunStateDetail = {
  state: DesktopRunState;
  tone: DesktopRunTone;
  label: string;
  reason: string;
};

export type RunStateInput = {
  connectionState: ConnectionState;
  connectionMessage: string;
  pendingApprovalCount: number;
  activeTurnStatus: RuntimeTurnStatus | null;
  latestTurnStatus: RuntimeTurnStatus | null;
  runningTaskCount: number;
  latestTaskStatus: TaskStatus | null;
};

const RUNNING_TURN_STATES: Set<RuntimeTurnStatus> = new Set(["queued", "in_progress"]);
const FAILED_TURN_STATES: Set<RuntimeTurnStatus> = new Set(["failed", "interrupted", "canceled"]);
const FAILED_TASK_STATES: Set<TaskStatus> = new Set(["failed", "canceled"]);

export function deriveDesktopRunState(input: RunStateInput): DesktopRunStateDetail {
  const reason = input.connectionMessage.trim() || "Runtime state unavailable";

  if (input.connectionState === "checking") {
    return {
      state: "checking",
      tone: "warning",
      label: "Checking runtime",
      reason,
    };
  }

  if (input.connectionState === "reconnecting") {
    return {
      state: "reconnecting",
      tone: "warning",
      label: "Reconnecting stream",
      reason,
    };
  }

  if (input.pendingApprovalCount > 0) {
    return {
      state: "waiting-approval",
      tone: "warning",
      label: "Waiting for approval",
      reason: `${input.pendingApprovalCount} approval request${input.pendingApprovalCount === 1 ? "" : "s"} pending`,
    };
  }

  if (
    (input.activeTurnStatus != null && RUNNING_TURN_STATES.has(input.activeTurnStatus)) ||
    input.runningTaskCount > 0
  ) {
    return {
      state: "running",
      tone: "warning",
      label: "Running",
      reason: "Turn or task is currently in progress",
    };
  }

  if (input.connectionState === "offline") {
    return {
      state: "failed",
      tone: "danger",
      label: "Runtime offline",
      reason,
    };
  }

  if (
    (input.latestTurnStatus != null && FAILED_TURN_STATES.has(input.latestTurnStatus)) ||
    (input.latestTaskStatus != null && FAILED_TASK_STATES.has(input.latestTaskStatus))
  ) {
    return {
      state: "failed",
      tone: "danger",
      label: "Action failed",
      reason: "Latest turn or task ended with an error",
    };
  }

  if (input.latestTurnStatus === "completed" || input.latestTaskStatus === "completed") {
    return {
      state: "completed",
      tone: "success",
      label: "Completed",
      reason: "Latest turn or task completed successfully",
    };
  }

  if (input.connectionState === "online") {
    return {
      state: "online",
      tone: "success",
      label: "Runtime online",
      reason,
    };
  }

  return {
    state: "idle",
    tone: "neutral",
    label: "Idle",
    reason: "No active thread, task, or approval",
  };
}
