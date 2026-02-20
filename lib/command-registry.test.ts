import { describe, expect, it, vi } from "vitest";
import type { MouseEvent } from "react";

import {
  buildCommandPaletteItems,
  buildSessionPaletteItems,
} from "@/lib/command-registry";

describe("command-registry", () => {
  it("keeps sessions mode behavior with resume and delete actions", () => {
    const resume = vi.fn();
    const remove = vi.fn();
    const items = buildSessionPaletteItems({
      sessions: [
        {
          id: "sess_1",
          title: "Session 1",
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
          message_count: 4,
          total_tokens: 100,
          model: "wagmii-chat",
          workspace: "/repo",
          mode: "agent",
        },
      ],
      formatRelative: () => "just now",
      onResumeSession: resume,
      onDeleteSession: remove,
    });

    expect(items).toHaveLength(1);
    expect(items[0]?.label).toBe("Session 1");
    items[0]?.action();
    expect(resume).toHaveBeenCalledTimes(1);

    const mockEvent = {
      stopPropagation: vi.fn(),
    } as unknown as MouseEvent;
    items[0]?.secondaryAction?.action(mockEvent);
    expect(remove).toHaveBeenCalledTimes(1);
  });

  it("includes contextual commands for approvals, thread actions, automations, and pane focus", () => {
    const items = buildCommandPaletteItems({
      pendingApprovalCount: 2,
      selectedThreadId: "thr_1",
      activeTurnId: "turn_1",
      currentAutomation: {
        id: "auto_1",
        name: "Daily review",
        prompt: "test",
        rrule: "FREQ=WEEKLY;BYDAY=MO;BYHOUR=9;BYMINUTE=0",
        cwds: [],
        status: "active",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
      onNewThread: vi.fn(),
      onFocusThreads: vi.fn(),
      onFocusComposer: vi.fn(),
      onFocusEvents: vi.fn(),
      onOpenSection: vi.fn(),
      onOpenSessions: vi.fn(),
      onReviewApprovals: vi.fn(),
      onResumeThread: vi.fn(),
      onForkThread: vi.fn(),
      onCompactThread: vi.fn(),
      onInterruptTurn: vi.fn(),
      onRunAutomation: vi.fn(),
    });

    const ids = new Set(items.map((item) => item.id));
    expect(ids.has("view-pending-approvals")).toBe(true);
    expect(ids.has("resume-thread")).toBe(true);
    expect(ids.has("fork-thread")).toBe(true);
    expect(ids.has("compact-thread")).toBe(true);
    expect(ids.has("interrupt-turn")).toBe(true);
    expect(ids.has("run-selected-automation")).toBe(true);
    expect(ids.has("focus-threads")).toBe(true);
    expect(ids.has("focus-composer")).toBe(true);
    expect(ids.has("focus-events")).toBe(true);
  });
});
