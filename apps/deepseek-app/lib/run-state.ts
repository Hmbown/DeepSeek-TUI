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
  reasonSource: "connection" | "approvals" | "active-turn" | "task-state" | "idle";
};

export type RunStateInput = {
  connectionState: ConnectionState;
  connectionMessage: string;
  pendingApprovalCount: number;
  activeTurnStatus: RuntimeTurnStatus | null;
  latestTurnStatus: RuntimeTurnStatus | null;
  runningTaskCount: number;
  queuedTaskCount: number;
  latestTaskStatus: TaskStatus | null;
  reconnectAttempt?: number;
  reconnectDelayMs?: number | null;
};

const RUNNING_TURN_STATES: Set<RuntimeTurnStatus> = new Set(["queued", "in_progress"]);
const FAILED_TURN_STATES: Set<RuntimeTurnStatus> = new Set(["failed", "interrupted", "canceled"]);
const FAILED_TASK_STATES: Set<TaskStatus> = new Set(["failed", "canceled"]);

export function deriveDesktopRunState(input: RunStateInput): DesktopRunStateDetail {
  const connectionReason = input.connectionMessage.trim() || "Runtime state unavailable";
  const reconnectSuffix =
    input.reconnectAttempt && input.reconnectDelayMs
      ? ` (attempt ${input.reconnectAttempt}, retry in ${(input.reconnectDelayMs / 1000).toFixed(1)}s)`
      : "";

  if (input.connectionState === "checking") {
    return {
      state: "checking",
      tone: "warning",
      label: "Checking runtime",
      reason: `connection: ${connectionReason}`,
      reasonSource: "connection",
    };
  }

  if (input.connectionState === "reconnecting") {
    return {
      state: "reconnecting",
      tone: "warning",
      label: "Reconnecting stream",
      reason: `connection: ${connectionReason}${reconnectSuffix}`,
      reasonSource: "connection",
    };
  }

  if (input.pendingApprovalCount > 0) {
    return {
      state: "waiting-approval",
      tone: "warning",
      label: "Waiting for approval",
      reason: `approvals: ${input.pendingApprovalCount} approval request${input.pendingApprovalCount === 1 ? "" : "s"} pending`,
      reasonSource: "approvals",
    };
  }

  if (input.activeTurnStatus != null && RUNNING_TURN_STATES.has(input.activeTurnStatus)) {
    return {
      state: "running",
      tone: "warning",
      label: "Running",
      reason: `active turn: status ${input.activeTurnStatus.replaceAll("_", " ")}`,
      reasonSource: "active-turn",
    };
  }

  if (input.runningTaskCount > 0 || input.queuedTaskCount > 0) {
    return {
      state: "running",
      tone: "warning",
      label: "Running",
      reason: `task state: ${input.runningTaskCount} running, ${input.queuedTaskCount} queued`,
      reasonSource: "task-state",
    };
  }

  if (input.connectionState === "offline") {
    return {
      state: "failed",
      tone: "danger",
      label: "Runtime offline",
      reason: `connection: ${connectionReason}`,
      reasonSource: "connection",
    };
  }

  if (input.latestTurnStatus != null && FAILED_TURN_STATES.has(input.latestTurnStatus)) {
    return {
      state: "failed",
      tone: "danger",
      label: "Action failed",
      reason: `active turn: latest turn ${input.latestTurnStatus.replaceAll("_", " ")}`,
      reasonSource: "active-turn",
    };
  }

  if (input.latestTaskStatus != null && FAILED_TASK_STATES.has(input.latestTaskStatus)) {
    return {
      state: "failed",
      tone: "danger",
      label: "Action failed",
      reason: `task state: latest task ${input.latestTaskStatus.replaceAll("_", " ")}`,
      reasonSource: "task-state",
    };
  }

  if (input.latestTurnStatus === "completed") {
    return {
      state: "completed",
      tone: "success",
      label: "Completed",
      reason: "active turn: latest turn completed",
      reasonSource: "active-turn",
    };
  }

  if (input.latestTaskStatus === "completed") {
    return {
      state: "completed",
      tone: "success",
      label: "Completed",
      reason: "task state: latest task completed",
      reasonSource: "task-state",
    };
  }

  if (input.connectionState === "online") {
    return {
      state: "online",
      tone: "success",
      label: "Runtime online",
      reason: `connection: ${connectionReason}`,
      reasonSource: "connection",
    };
  }

  return {
    state: "idle",
    tone: "neutral",
    label: "Idle",
    reason: "idle: no active thread, task, or approval",
    reasonSource: "idle",
  };
}
