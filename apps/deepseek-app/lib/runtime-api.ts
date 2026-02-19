export type ThreadSummary = {
  id: string;
  title: string;
  preview: string;
  model: string;
  mode: string;
  archived: boolean;
  updated_at: string;
  latest_turn_id?: string | null;
  latest_turn_status?: string | null;
};

export type RuntimeTurnStatus =
  | "queued"
  | "in_progress"
  | "completed"
  | "failed"
  | "interrupted"
  | "canceled";

export type RuntimeItemKind =
  | "user_message"
  | "agent_message"
  | "tool_call"
  | "file_change"
  | "command_execution"
  | "context_compaction"
  | "status"
  | "error";

export type RuntimeItemStatus =
  | "queued"
  | "in_progress"
  | "completed"
  | "failed"
  | "interrupted"
  | "canceled";

export type ThreadDetail = {
  thread: {
    id: string;
    model: string;
    mode: string;
    updated_at: string;
    archived: boolean;
    latest_turn_id?: string | null;
  };
  turns: Array<{
    id: string;
    status: RuntimeTurnStatus;
    input_summary: string;
    created_at: string;
    started_at?: string | null;
    ended_at?: string | null;
  }>;
  items: Array<{
    id: string;
    turn_id: string;
    kind: RuntimeItemKind;
    status: RuntimeItemStatus;
    summary: string;
    detail?: string | null;
    started_at?: string | null;
    ended_at?: string | null;
  }>;
  latest_seq: number;
};

export type WorkspaceStatus = {
  workspace: string;
  git_repo: boolean;
  branch?: string | null;
  staged: number;
  unstaged: number;
  untracked: number;
  ahead?: number | null;
  behind?: number | null;
};

export type SkillEntry = {
  name: string;
  description: string;
  path: string;
};

export type SkillsResponse = {
  directory: string;
  warnings: string[];
  skills: SkillEntry[];
};

export type McpServerEntry = {
  name: string;
  enabled: boolean;
  required: boolean;
  command?: string | null;
  url?: string | null;
  connected: boolean;
  enabled_tools: string[];
  disabled_tools: string[];
};

export type McpServersResponse = {
  servers: McpServerEntry[];
};

export type McpToolEntry = {
  server: string;
  name: string;
  prefixed_name: string;
  description?: string | null;
  input_schema: unknown;
};

export type McpToolsResponse = {
  tools: McpToolEntry[];
};

export type AutomationStatus = "active" | "paused";

export type AutomationRecord = {
  id: string;
  name: string;
  prompt: string;
  rrule: string;
  cwds: string[];
  status: AutomationStatus;
  created_at: string;
  updated_at: string;
  next_run_at?: string | null;
  last_run_at?: string | null;
};

export type AutomationRunRecord = {
  id: string;
  automation_id: string;
  scheduled_for: string;
  status: "queued" | "running" | "completed" | "failed" | "canceled";
  created_at: string;
  started_at?: string | null;
  ended_at?: string | null;
  task_id?: string | null;
  thread_id?: string | null;
  turn_id?: string | null;
  error?: string | null;
};

export type SessionMetadata = {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  message_count: number;
  total_tokens: number;
  model: string;
  workspace: string;
  mode?: string | null;
};

export type SessionsResponse = {
  sessions: SessionMetadata[];
};

export type SessionDetail = {
  metadata: SessionMetadata;
  messages: Array<{
    role: string;
    content: Array<{ type: string; text?: string }>;
  }>;
  system_prompt?: string | null;
};

export type ResumeSessionResponse = {
  thread_id: string;
  session_id: string;
  message_count: number;
  summary: string;
};

export type TaskStatus = "queued" | "running" | "completed" | "failed" | "canceled";

export type TaskSummary = {
  id: string;
  status: TaskStatus;
  prompt_summary: string;
  model: string;
  mode: string;
  created_at: string;
  started_at?: string | null;
  ended_at?: string | null;
  duration_ms?: number | null;
  error?: string | null;
  thread_id?: string | null;
  turn_id?: string | null;
};

export type TaskRecord = TaskSummary & {
  prompt: string;
  workspace: string;
  allow_shell: boolean;
  trust_mode: boolean;
  auto_approve: boolean;
  result_summary?: string | null;
  result_detail_path?: string | null;
  runtime_event_count: number;
  tool_calls: Array<{
    id: string;
    name: string;
    status: "running" | "success" | "failed" | "canceled";
    started_at: string;
    ended_at?: string | null;
    duration_ms?: number | null;
    input_summary?: string | null;
    output_summary?: string | null;
    detail_path?: string | null;
    patch_ref?: string | null;
  }>;
  timeline: Array<{
    timestamp: string;
    kind: string;
    summary: string;
    detail_path?: string | null;
  }>;
};

export type TasksResponse = {
  tasks: TaskSummary[];
  counts: {
    queued: number;
    running: number;
    completed: number;
    failed: number;
    canceled: number;
  };
};

export type RuntimeEventPayload = {
  seq: number;
  timestamp?: string;
  thread_id?: string;
  turn_id?: string | null;
  item_id?: string | null;
  event: string;
  payload: unknown;
};

export type EventPayload = {
  event: string;
  payload: unknown;
  seq?: number;
  timestamp?: string;
  thread_id?: string;
  turn_id?: string | null;
  item_id?: string | null;
};

export type PendingApprovalStatus = "pending" | "denied";

export type PendingApproval = {
  id: string;
  event: "approval.required" | "sandbox.denied";
  status: PendingApprovalStatus;
  requestType: string;
  scope: string;
  consequence: string;
  createdAt: string;
  rawSnippet: string;
  seq?: number;
  threadId?: string;
  turnId?: string | null;
};

export type RuntimeApiError = {
  message: string;
  status: number;
};

export const DEFAULT_RUNTIME_BASE_URL = "http://127.0.0.1:7878";
const STORAGE_KEY = "deepseek.app.runtime.baseUrl";

export function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.trim().replace(/\/$/, "");
}

export function loadRuntimeBaseUrl(): string {
  if (typeof window === "undefined") {
    return DEFAULT_RUNTIME_BASE_URL;
  }
  const stored = window.localStorage.getItem(STORAGE_KEY);
  if (!stored?.trim()) {
    return DEFAULT_RUNTIME_BASE_URL;
  }
  return normalizeBaseUrl(stored);
}

export function persistRuntimeBaseUrl(baseUrl: string): void {
  if (typeof window === "undefined") {
    return;
  }
  window.localStorage.setItem(STORAGE_KEY, normalizeBaseUrl(baseUrl));
}

export function parseApiError(payload: unknown, fallbackStatus = 500): RuntimeApiError {
  const direct = payload as { message?: string; status?: number };
  if (typeof direct?.message === "string") {
    return {
      message: direct.message,
      status: direct.status ?? fallbackStatus,
    };
  }
  const obj = payload as { error?: { message?: string; status?: number } };
  return {
    message: obj?.error?.message ?? "Request failed",
    status: obj?.error?.status ?? fallbackStatus,
  };
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function pickString(payload: Record<string, unknown>, keys: string[]): string | null {
  for (const key of keys) {
    const value = payload[key];
    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }
  return null;
}

function asSnippet(payload: unknown): string {
  if (typeof payload === "string") {
    return payload.slice(0, 240);
  }
  try {
    return JSON.stringify(payload).slice(0, 240);
  } catch {
    return String(payload).slice(0, 240);
  }
}

function extractApprovalField(payload: unknown, keys: string[], fallback: string): string {
  const record = asRecord(payload);
  if (!record) {
    return fallback;
  }
  return pickString(record, keys) ?? fallback;
}

export function parsePendingApprovalEvent(event: EventPayload): PendingApproval | null {
  if (event.event !== "approval.required" && event.event !== "sandbox.denied") {
    return null;
  }

  const createdAt = event.timestamp ?? new Date().toISOString();
  const status: PendingApprovalStatus = event.event === "approval.required" ? "pending" : "denied";
  const requestType = extractApprovalField(
    event.payload,
    ["request_type", "type", "action", "kind"],
    event.event === "approval.required" ? "Approval request" : "Sandbox denial"
  );
  const scope = extractApprovalField(
    event.payload,
    ["scope", "target", "tool", "path", "command"],
    "Scope unavailable"
  );
  const consequence = extractApprovalField(
    event.payload,
    ["consequence", "reason", "summary", "message", "detail"],
    event.event === "approval.required"
      ? "Runtime paused and waiting for approval."
      : "Action blocked by sandbox policy."
  );
  const seqPart = event.seq != null ? `seq-${event.seq}` : `ts-${createdAt}`;

  return {
    id: `${event.event}-${seqPart}`,
    event: event.event,
    status,
    requestType,
    scope,
    consequence,
    createdAt,
    rawSnippet: asSnippet(event.payload),
    seq: event.seq,
    threadId: event.thread_id,
    turnId: event.turn_id,
  };
}

async function request<T>(baseUrl: string, path: string, init?: RequestInit): Promise<T> {
  const url = `${normalizeBaseUrl(baseUrl)}${path}`;
  const headers = new Headers(init?.headers ?? {});
  if (init?.body && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }

  const response = await fetch(url, {
    ...init,
    headers,
    cache: "no-store",
  });

  if (!response.ok) {
    let payload: unknown = null;
    try {
      payload = await response.json();
    } catch {
      payload = { error: { message: `HTTP ${response.status}`, status: response.status } };
    }
    throw parseApiError(payload, response.status);
  }

  return (await response.json()) as T;
}

export function getHealth(baseUrl: string): Promise<{ status: string; service?: string; mode?: string }> {
  return request(baseUrl, "/health");
}

export function getWorkspaceStatus(baseUrl: string): Promise<WorkspaceStatus> {
  return request(baseUrl, "/v1/workspace/status");
}

export function listSessions(
  baseUrl: string,
  query: {
    limit?: number;
    search?: string;
  } = {}
): Promise<SessionsResponse> {
  const params = new URLSearchParams();
  if (query.limit != null) {
    params.set("limit", String(query.limit));
  }
  if (query.search?.trim()) {
    params.set("search", query.search.trim());
  }
  const suffix = params.toString();
  return request(baseUrl, `/v1/sessions${suffix ? `?${suffix}` : ""}`);
}

export function getSession(baseUrl: string, sessionId: string): Promise<SessionDetail> {
  return request(baseUrl, `/v1/sessions/${encodeURIComponent(sessionId)}`);
}

export function resumeSessionThread(
  baseUrl: string,
  sessionId: string,
  payload: { model?: string; mode?: string } = {}
): Promise<ResumeSessionResponse> {
  return request(baseUrl, `/v1/sessions/${encodeURIComponent(sessionId)}/resume-thread`, {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export async function deleteSession(baseUrl: string, sessionId: string): Promise<void> {
  const url = `${normalizeBaseUrl(baseUrl)}/v1/sessions/${encodeURIComponent(sessionId)}`;
  const resp = await fetch(url, { method: "DELETE", cache: "no-store" });
  if (!resp.ok) {
    let payload: unknown = null;
    try {
      payload = await resp.json();
    } catch {
      payload = { error: { message: `HTTP ${resp.status}`, status: resp.status } };
    }
    throw parseApiError(payload, resp.status);
  }
}

export function listTasks(baseUrl: string, query: { limit?: number } = {}): Promise<TasksResponse> {
  const params = new URLSearchParams();
  if (query.limit != null) {
    params.set("limit", String(query.limit));
  }
  const suffix = params.toString();
  return request(baseUrl, `/v1/tasks${suffix ? `?${suffix}` : ""}`);
}

export function getTask(baseUrl: string, taskId: string): Promise<TaskRecord> {
  return request(baseUrl, `/v1/tasks/${taskId}`);
}

export function listThreadSummaries(
  baseUrl: string,
  query: { search?: string; limit?: number; includeArchived?: boolean }
): Promise<ThreadSummary[]> {
  const params = new URLSearchParams();
  if (query.search?.trim()) {
    params.set("search", query.search.trim());
  }
  params.set("limit", String(query.limit ?? 100));
  if (query.includeArchived) {
    params.set("include_archived", "true");
  }
  return request(baseUrl, `/v1/threads/summary?${params.toString()}`);
}

export function createThread(
  baseUrl: string,
  payload: { model?: string; mode?: string }
): Promise<{ id: string }> {
  return request(baseUrl, "/v1/threads", {
    method: "POST",
    body: JSON.stringify({
      model: payload.model,
      mode: payload.mode,
      archived: false,
    }),
  });
}

export function updateThread(
  baseUrl: string,
  threadId: string,
  payload: { archived?: boolean }
): Promise<{ id: string; archived: boolean }> {
  return request(baseUrl, `/v1/threads/${threadId}`, {
    method: "PATCH",
    body: JSON.stringify(payload),
  });
}

export function forkThread(baseUrl: string, threadId: string): Promise<{ id: string }> {
  return request(baseUrl, `/v1/threads/${threadId}/fork`, {
    method: "POST",
    body: JSON.stringify({}),
  });
}

export function resumeThread(baseUrl: string, threadId: string): Promise<{ id: string }> {
  return request(baseUrl, `/v1/threads/${threadId}/resume`, {
    method: "POST",
    body: JSON.stringify({}),
  });
}

export function compactThread(
  baseUrl: string,
  threadId: string,
  payload: { reason?: string } = {}
): Promise<{ thread: { id: string }; turn: { id: string } }> {
  return request(baseUrl, `/v1/threads/${threadId}/compact`, {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export function getThreadDetail(baseUrl: string, threadId: string): Promise<ThreadDetail> {
  return request(baseUrl, `/v1/threads/${threadId}`);
}

export function startTurn(
  baseUrl: string,
  threadId: string,
  payload: { prompt: string; model?: string; mode?: string }
): Promise<{ turn: { id: string } }> {
  return request(baseUrl, `/v1/threads/${threadId}/turns`, {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export function steerTurn(
  baseUrl: string,
  threadId: string,
  turnId: string,
  prompt: string
): Promise<{ id: string }> {
  return request(baseUrl, `/v1/threads/${threadId}/turns/${turnId}/steer`, {
    method: "POST",
    body: JSON.stringify({ prompt }),
  });
}

export function interruptTurn(
  baseUrl: string,
  threadId: string,
  turnId: string
): Promise<{ id: string }> {
  return request(baseUrl, `/v1/threads/${threadId}/turns/${turnId}/interrupt`, {
    method: "POST",
    body: JSON.stringify({}),
  });
}

export function listSkills(baseUrl: string): Promise<SkillsResponse> {
  return request(baseUrl, "/v1/skills");
}

export function listMcpServers(baseUrl: string): Promise<McpServersResponse> {
  return request(baseUrl, "/v1/apps/mcp/servers");
}

export function listMcpTools(baseUrl: string, server?: string): Promise<McpToolsResponse> {
  const params = new URLSearchParams();
  if (server) {
    params.set("server", server);
  }
  const suffix = params.toString();
  return request(baseUrl, `/v1/apps/mcp/tools${suffix ? `?${suffix}` : ""}`);
}

export function listAutomations(baseUrl: string): Promise<AutomationRecord[]> {
  return request(baseUrl, "/v1/automations");
}

export function createAutomation(
  baseUrl: string,
  payload: { name: string; prompt: string; rrule: string; status?: AutomationStatus; cwds?: string[] }
): Promise<AutomationRecord> {
  return request(baseUrl, "/v1/automations", {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export function updateAutomation(
  baseUrl: string,
  id: string,
  payload: Partial<{ name: string; prompt: string; rrule: string; status: AutomationStatus; cwds: string[] }>
): Promise<AutomationRecord> {
  return request(baseUrl, `/v1/automations/${id}`, {
    method: "PATCH",
    body: JSON.stringify(payload),
  });
}

export function deleteAutomation(baseUrl: string, id: string): Promise<AutomationRecord> {
  return request(baseUrl, `/v1/automations/${id}`, {
    method: "DELETE",
  });
}

export function pauseAutomation(baseUrl: string, id: string): Promise<AutomationRecord> {
  return request(baseUrl, `/v1/automations/${id}/pause`, {
    method: "POST",
    body: JSON.stringify({}),
  });
}

export function resumeAutomation(baseUrl: string, id: string): Promise<AutomationRecord> {
  return request(baseUrl, `/v1/automations/${id}/resume`, {
    method: "POST",
    body: JSON.stringify({}),
  });
}

export function runAutomation(baseUrl: string, id: string): Promise<AutomationRunRecord> {
  return request(baseUrl, `/v1/automations/${id}/run`, {
    method: "POST",
    body: JSON.stringify({}),
  });
}

export function listAutomationRuns(
  baseUrl: string,
  id: string,
  limit = 20
): Promise<AutomationRunRecord[]> {
  return request(baseUrl, `/v1/automations/${id}/runs?limit=${limit}`);
}

export function buildThreadEventsUrl(baseUrl: string, threadId: string, sinceSeq: number): string {
  return `${normalizeBaseUrl(baseUrl)}/v1/threads/${threadId}/events?since_seq=${Math.max(sinceSeq, 0)}`;
}

export function openThreadEvents(
  baseUrl: string,
  threadId: string,
  sinceSeq: number,
  onEvent: (event: EventPayload) => void,
  onError: (error: Event) => void
): EventSource {
  const source = new EventSource(buildThreadEventsUrl(baseUrl, threadId, sinceSeq));

  const eventNames = [
    "thread.started",
    "thread.forked",
    "thread.updated",
    "turn.started",
    "turn.lifecycle",
    "turn.steered",
    "turn.interrupt_requested",
    "turn.completed",
    "item.started",
    "item.delta",
    "item.completed",
    "item.failed",
    "item.interrupted",
    "approval.required",
    "sandbox.denied",
  ];

  for (const eventName of eventNames) {
    source.addEventListener(eventName, (event) => {
      const message = event as MessageEvent;
      try {
        const parsed = JSON.parse(message.data) as RuntimeEventPayload;
        onEvent({
          event: eventName,
          payload: parsed.payload,
          seq: parsed.seq,
          timestamp: parsed.timestamp,
          thread_id: parsed.thread_id,
          turn_id: parsed.turn_id,
          item_id: parsed.item_id,
        });
      } catch {
        onEvent({ event: eventName, payload: message.data });
      }
    });
  }

  source.onerror = onError;
  return source;
}
