import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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

describe("Small window layout", () => {
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
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1000 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 760 });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("shows compact pane switcher and toggles panes", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/health")) {
        return okJson({ status: "ok", service: "deepseek-runtime-api", mode: "local" });
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
            title: "Thread",
            preview: "Preview",
            model: "deepseek-reasoner",
            mode: "agent",
            archived: false,
            updated_at: "2026-01-01T00:00:00Z",
            latest_turn_status: "completed",
          },
        ]);
      }
      if (url.includes("/v1/threads/thr_1/events")) {
        return okJson({});
      }
      if (url.includes("/v1/threads/thr_1")) {
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
      if (url.includes("/v1/tasks")) {
        return okJson({ tasks: [], counts: { queued: 0, running: 0, completed: 0, failed: 0, canceled: 0 } });
      }
      return okJson([]);
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<Home />);

    await waitFor(() => {
      expect(screen.getByRole("tablist", { name: /compact pane switcher/i })).toBeInTheDocument();
    });
    expect(document.querySelector(".app-shell")?.className).toContain("is-short-height");

    fireEvent.click(screen.getByRole("tab", { name: "Events" }));
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Live events" })).toBeInTheDocument();
    });

    const eventList = document.querySelector(".event-list") as HTMLDivElement;
    eventList.scrollTop = 42;
    fireEvent.scroll(eventList, { target: { scrollTop: 42 } });

    fireEvent.click(screen.getByRole("tab", { name: "Transcript" }));
    fireEvent.click(screen.getByRole("tab", { name: "Events" }));

    await waitFor(() => {
      const restored = document.querySelector(".event-list") as HTMLDivElement;
      expect(restored.scrollTop).toBe(42);
    });
  });
});
