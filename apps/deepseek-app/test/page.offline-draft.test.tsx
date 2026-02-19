import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import Home from "@/app/page";

class MockEventSource {
  onerror: ((event: Event) => void) | null = null;

  addEventListener() {
    // no-op
  }

  close() {
    // no-op
  }
}

function okJson(payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status: 200,
    headers: {
      "content-type": "application/json",
    },
  });
}

describe("Offline-safe composer drafts", () => {
  beforeEach(() => {
    vi.stubGlobal("EventSource", MockEventSource as unknown as typeof EventSource);
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: {
        getItem: vi.fn(() => null),
        setItem: vi.fn(),
        removeItem: vi.fn(),
      },
    });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("preserves per-thread draft, blocks offline send, and retries manually when online", async () => {
    let online = false;
    const startTurnCalls: string[] = [];

    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      const method = (init?.method ?? "GET").toUpperCase();

      if (url.endsWith("/health")) {
        if (online) {
          return okJson({ status: "ok", service: "deepseek-runtime-api", mode: "local" });
        }
        return new Response(JSON.stringify({ error: { message: "Runtime down", status: 503 } }), {
          status: 503,
          headers: { "content-type": "application/json" },
        });
      }

      if (url.includes("/v1/workspace/status")) {
        return okJson({
          workspace: "/repo",
          git_repo: true,
          branch: "main",
          staged: 0,
          unstaged: 0,
          untracked: 0,
          ahead: 0,
          behind: 0,
        });
      }

      if (url.includes("/v1/threads/summary")) {
        return okJson([
          {
            id: "thr_1",
            title: "First",
            preview: "Preview first",
            model: "deepseek-reasoner",
            mode: "agent",
            archived: false,
            updated_at: "2026-01-01T00:00:00Z",
            latest_turn_status: "completed",
          },
          {
            id: "thr_2",
            title: "Second",
            preview: "Preview second",
            model: "deepseek-chat",
            mode: "agent",
            archived: false,
            updated_at: "2026-01-01T00:00:00Z",
            latest_turn_status: "completed",
          },
        ]);
      }

      if (url.includes("/v1/threads/thr_1") && !url.includes("/turns")) {
        if (!online) {
          return new Response(JSON.stringify({ error: { message: "Runtime down", status: 503 } }), {
            status: 503,
            headers: { "content-type": "application/json" },
          });
        }
        return okJson({
          thread: {
            id: "thr_1",
            model: "deepseek-reasoner",
            mode: "agent",
            updated_at: "2026-01-01T00:00:00Z",
            archived: false,
            latest_turn_id: "turn_1",
          },
          turns: [],
          items: [],
          latest_seq: 0,
        });
      }

      if (url.includes("/v1/threads/thr_2") && !url.includes("/turns")) {
        if (!online) {
          return new Response(JSON.stringify({ error: { message: "Runtime down", status: 503 } }), {
            status: 503,
            headers: { "content-type": "application/json" },
          });
        }
        return okJson({
          thread: {
            id: "thr_2",
            model: "deepseek-chat",
            mode: "agent",
            updated_at: "2026-01-01T00:00:00Z",
            archived: false,
            latest_turn_id: "turn_2",
          },
          turns: [],
          items: [],
          latest_seq: 0,
        });
      }

      if (url.includes("/v1/threads/thr_1/turns") && method === "POST") {
        const payload = JSON.parse(String(init?.body ?? "{}")) as { prompt?: string };
        startTurnCalls.push(payload.prompt ?? "");
        return okJson({ turn: { id: "turn_new" } });
      }

      if (url.includes("/v1/tasks")) {
        return okJson({ tasks: [], counts: { queued: 0, running: 0, completed: 0, failed: 0, canceled: 0 } });
      }

      return okJson([]);
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<Home />);

    await waitFor(() => {
      expect(screen.getByText("First")).toBeInTheDocument();
      expect(screen.getByText("Second")).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(screen.getAllByText("Send blocked: runtime offline.").length).toBeGreaterThan(0);
    });

    const textarea = screen.getByPlaceholderText("Type a prompt…");
    fireEvent.change(textarea, { target: { value: "offline first draft" } });

    // Send button is disabled when offline
    expect(screen.getByRole("button", { name: /^send$/i })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: /^send$/i }));
    expect(startTurnCalls).toHaveLength(0);
    expect(screen.getAllByText("Send blocked: runtime offline.").length).toBeGreaterThan(0);

    // Switch to second thread, draft is preserved per-thread
    const secondThreadButton = screen.getByText("Second").closest("button");
    expect(secondThreadButton).toBeTruthy();
    if (!secondThreadButton) {
      return;
    }
    fireEvent.click(secondThreadButton);
    await waitFor(() => {
      expect((screen.getByPlaceholderText("Type a prompt…") as HTMLTextAreaElement).value).toBe("");
    });

    fireEvent.change(screen.getByPlaceholderText("Type a prompt…"), {
      target: { value: "second thread draft" },
    });

    // Switch back to first thread, draft is restored
    const firstThreadButton = screen.getByText("First").closest("button");
    expect(firstThreadButton).toBeTruthy();
    if (!firstThreadButton) {
      return;
    }
    fireEvent.click(firstThreadButton);
    await waitFor(() => {
      expect((screen.getByPlaceholderText("Type a prompt…") as HTMLTextAreaElement).value).toBe("offline first draft");
    });

    // Come back online - Send button re-enables, draft is still in textarea
    online = true;
    fireEvent.click(screen.getByRole("button", { name: /retry now/i }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /^send$/i })).not.toBeDisabled();
    });

    expect(startTurnCalls).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: /^send$/i }));

    await waitFor(() => {
      expect(startTurnCalls).toEqual(["offline first draft"]);
    });
  });
});
