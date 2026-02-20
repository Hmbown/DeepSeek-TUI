import { afterEach, describe, expect, it, vi } from "vitest";

import {
  buildThreadEventsUrl,
  getSession,
  listSessions,
  listTasks,
  listThreadSummaries,
  normalizeBaseUrl,
  parsePendingApprovalEvent,
  parseApiError,
  resumeSessionThread,
  type RuntimeApiError,
} from "@/lib/runtime-api";

function okJson(payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status: 200,
    headers: {
      "content-type": "application/json",
    },
  });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("runtime-api helpers", () => {
  it("normalizes trailing slash", () => {
    expect(normalizeBaseUrl("http://127.0.0.1:7878/")).toBe("http://127.0.0.1:7878");
  });

  it("builds thread summary query with include_archived", async () => {
    const fetchMock = vi.fn().mockResolvedValue(okJson([]));
    vi.stubGlobal("fetch", fetchMock);

    await listThreadSummaries("http://127.0.0.1:7878", {
      search: "abc",
      limit: 50,
      includeArchived: true,
    });

    const [url] = fetchMock.mock.calls[0] ?? [];
    expect(String(url)).toContain("/v1/threads/summary?");
    expect(String(url)).toContain("search=abc");
    expect(String(url)).toContain("limit=50");
    expect(String(url)).toContain("include_archived=true");
  });

  it("builds sessions and tasks query params", async () => {
    const fetchMock = vi.fn().mockResolvedValue(okJson({ sessions: [] })).mockResolvedValueOnce(okJson({ sessions: [] }));
    vi.stubGlobal("fetch", fetchMock);

    await listSessions("http://127.0.0.1:7878", { limit: 20, search: "hello" });
    await listTasks("http://127.0.0.1:7878", { limit: 5 });

    const [sessionsUrl] = fetchMock.mock.calls[0] ?? [];
    const [tasksUrl] = fetchMock.mock.calls[1] ?? [];
    expect(String(sessionsUrl)).toContain("/v1/sessions?");
    expect(String(sessionsUrl)).toContain("limit=20");
    expect(String(sessionsUrl)).toContain("search=hello");
    expect(String(tasksUrl)).toContain("/v1/tasks?limit=5");
  });

  it("builds thread events url", () => {
    const url = buildThreadEventsUrl("http://127.0.0.1:7878/", "thr_123", 42);
    expect(url).toBe("http://127.0.0.1:7878/v1/threads/thr_123/events?since_seq=42");
  });

  it("parses structured API errors", () => {
    const payload = {
      error: {
        message: "bad request",
        status: 400,
      },
    };
    const parsed: RuntimeApiError = parseApiError(payload, 500);
    expect(parsed.message).toBe("bad request");
    expect(parsed.status).toBe(400);
  });

  it("falls back for unknown payload", () => {
    const parsed = parseApiError({ something: true }, 502);
    expect(parsed.message).toBe("Request failed");
    expect(parsed.status).toBe(502);
  });

  it("parses approval.required into pending approval model", () => {
    const parsed = parsePendingApprovalEvent({
      event: "approval.required",
      payload: {
        request_type: "shell command",
        scope: "workspace write",
        reason: "Command wants to modify files",
        actions: [{ type: "approve" }, { type: "deny" }],
      },
      seq: 88,
      timestamp: "2026-01-01T00:00:00Z",
      thread_id: "thr_1",
      turn_id: "turn_1",
    });

    expect(parsed).not.toBeNull();
    expect(parsed?.status).toBe("pending");
    expect(parsed?.requestType).toBe("shell command");
    expect(parsed?.scope).toBe("workspace write");
    expect(parsed?.actionHints?.approve?.available).toBe(true);
    expect(parsed?.actionHints?.deny?.available).toBe(true);
  });

  it("getSession calls correct endpoint", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      okJson({
        metadata: { id: "sess_1", title: "Test" },
        messages: [],
        system_prompt: null,
      })
    );
    vi.stubGlobal("fetch", fetchMock);

    const result = await getSession("http://127.0.0.1:7878", "sess_1");
    const [url] = fetchMock.mock.calls[0] ?? [];
    expect(String(url)).toContain("/v1/sessions/sess_1");
    expect(result.metadata.id).toBe("sess_1");
  });

  it("resumeSessionThread calls POST with model/mode", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      okJson({
        thread_id: "thr_new",
        session_id: "sess_1",
        message_count: 4,
        summary: "Resumed session",
      })
    );
    vi.stubGlobal("fetch", fetchMock);

    const result = await resumeSessionThread("http://127.0.0.1:7878", "sess_1", {
      model: "wagmii-chat",
      mode: "agent",
    });

    const [url, init] = fetchMock.mock.calls[0] ?? [];
    expect(String(url)).toContain("/v1/sessions/sess_1/resume-thread");
    expect(init?.method).toBe("POST");
    expect(result.thread_id).toBe("thr_new");
    expect(result.message_count).toBe(4);
  });
});
