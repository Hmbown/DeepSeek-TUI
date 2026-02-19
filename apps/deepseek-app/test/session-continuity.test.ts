import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { loadPersistedUiState, resolveRestoredThreadId } from "@/lib/ui-persistence";

describe("session continuity helpers", () => {
  beforeEach(() => {
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: {
        getItem: vi.fn((key: string) => {
          const values: Record<string, string> = {
            "deepseek.app.lastSection": "chat",
            "deepseek.app.lastPane": "events",
            "deepseek.app.lastThreadId": "thr_saved",
          };
          return values[key] ?? null;
        }),
        setItem: vi.fn(),
        removeItem: vi.fn(),
      },
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("loads persisted UI values", () => {
    const state = loadPersistedUiState();
    expect(state.section).toBe("chat");
    expect(state.pane).toBe("events");
    expect(state.threadId).toBe("thr_saved");
  });

  it("falls back to first thread when persisted id is stale", () => {
    const selected = resolveRestoredThreadId("thr_missing", [
      {
        id: "thr_first",
        title: "First",
        preview: "preview",
        model: "deepseek-chat",
        mode: "agent",
        archived: false,
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    expect(selected).toBe("thr_first");
  });
});
