import type { RuntimeItemKind, ThreadDetail, ThreadSummary, WorkspaceSummary } from "@/lib/runtime-api";

export function filterThreadSummaries(items: ThreadSummary[], query: string): ThreadSummary[] {
  const normalized = query.trim().toLowerCase();
  if (!normalized) {
    return items;
  }
  return items.filter((item) => {
    const haystack = `${item.id} ${item.title} ${item.preview} ${item.model} ${item.mode}`.toLowerCase();
    return haystack.includes(normalized);
  });
}

export type TranscriptCellRole =
  | "user"
  | "assistant"
  | "system"
  | "tool"
  | "file"
  | "command"
  | "compaction"
  | "status"
  | "error";

export type TranscriptCell = {
  id: string;
  role: TranscriptCellRole;
  title: string;
  content: string;
  turnId?: string;
  status?: string;
  kind?: RuntimeItemKind;
  startedAt?: string | null;
  endedAt?: string | null;
  reasoning_content?: string | null;
};

function mapRole(kind: RuntimeItemKind): TranscriptCellRole {
  switch (kind) {
    case "user_message":
      return "user";
    case "agent_message":
      return "assistant";
    case "tool_call":
      return "tool";
    case "file_change":
      return "file";
    case "command_execution":
      return "command";
    case "context_compaction":
      return "compaction";
    case "status":
      return "status";
    case "error":
      return "error";
    default:
      return "system";
  }
}

function mapTitle(role: TranscriptCellRole): string {
  switch (role) {
    case "user":
      return "You";
    case "assistant":
      return "Assistant";
    case "tool":
      return "Tool";
    case "file":
      return "File";
    case "command":
      return "Command";
    case "compaction":
      return "Compaction";
    case "status":
      return "Status";
    case "error":
      return "Error";
    default:
      return "System";
  }
}

function itemContent(summary: string, detail?: string | null): string {
  const candidate = detail?.trim();
  if (candidate) {
    return candidate;
  }
  return summary;
}

export function buildTranscript(detail: ThreadDetail | null): TranscriptCell[] {
  if (!detail) {
    return [];
  }

  const cells: TranscriptCell[] = detail.items.map((item) => {
    const role = mapRole(item.kind);
    return {
      id: item.id,
      role,
      title: mapTitle(role),
      content: itemContent(item.summary, item.detail),
      turnId: item.turn_id,
      status: item.status,
      kind: item.kind,
      startedAt: item.started_at,
      endedAt: item.ended_at,
      reasoning_content: item.reasoning_content,
    };
  });

  if (cells.length > 0) {
    return cells;
  }

  return detail.turns.map((turn) => ({
    id: turn.id,
    role: "system",
    title: "System",
    content: turn.input_summary,
    status: turn.status,
    turnId: turn.id,
    startedAt: turn.started_at,
    endedAt: turn.ended_at,
  }));
}

export function findActiveTurnId(detail: ThreadDetail | null): string | null {
  if (!detail) {
    return null;
  }

  for (let index = detail.turns.length - 1; index >= 0; index -= 1) {
    const turn = detail.turns[index];
    if (turn.status === "queued" || turn.status === "in_progress") {
      return turn.id;
    }
  }
  return null;
}

// -- Feature 1: Hierarchical thread grouping --

export type ThreadGroup = {
  workspace: WorkspaceSummary;
  threads: ThreadSummary[];
};

const UNGROUPED_WORKSPACE: WorkspaceSummary = {
  id: "__ungrouped__",
  path: "",
  name: "Ungrouped",
  thread_count: 0,
};

export function groupThreadsByWorkspace(
  threads: ThreadSummary[],
  workspaces: WorkspaceSummary[]
): ThreadGroup[] {
  const workspaceMap = new Map<string, WorkspaceSummary>();
  for (const ws of workspaces) {
    workspaceMap.set(ws.path, ws);
  }

  const grouped = new Map<string, ThreadSummary[]>();
  const ungrouped: ThreadSummary[] = [];

  for (const thread of threads) {
    if (thread.workspace && workspaceMap.has(thread.workspace)) {
      const existing = grouped.get(thread.workspace) ?? [];
      existing.push(thread);
      grouped.set(thread.workspace, existing);
    } else {
      ungrouped.push(thread);
    }
  }

  const result: ThreadGroup[] = [];

  for (const ws of workspaces) {
    const wsThreads = grouped.get(ws.path);
    if (wsThreads && wsThreads.length > 0) {
      result.push({
        workspace: { ...ws, thread_count: wsThreads.length },
        threads: wsThreads,
      });
    }
  }

  if (ungrouped.length > 0) {
    result.push({
      workspace: { ...UNGROUPED_WORKSPACE, thread_count: ungrouped.length },
      threads: ungrouped,
    });
  }

  return result;
}

// -- Feature 2: Thread status icons and relative time --

export type ThreadStatusIconName = "loader" | "check" | "x" | "minus";

export function threadStatusIcon(status?: string | null): ThreadStatusIconName {
  if (!status) {
    return "minus";
  }
  switch (status) {
    case "in_progress":
    case "queued":
      return "loader";
    case "completed":
      return "check";
    case "failed":
      return "x";
    case "interrupted":
    case "canceled":
      return "minus";
    default:
      return "minus";
  }
}

export function formatRelativeTime(value?: string | null): string {
  if (!value) {
    return "-";
  }
  const then = new Date(value).getTime();
  if (Number.isNaN(then)) {
    return value;
  }
  const diffMs = Date.now() - then;
  const abs = Math.abs(diffMs);
  const secs = Math.floor(abs / 1000);
  if (secs < 60) {
    return "now";
  }
  const mins = Math.floor(secs / 60);
  if (mins < 60) {
    return `${mins}m`;
  }
  const hours = Math.floor(mins / 60);
  if (hours < 24) {
    return `${hours}h`;
  }
  const days = Math.floor(hours / 24);
  return `${days}d`;
}
