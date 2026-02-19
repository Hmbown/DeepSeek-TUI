import { describe, expect, it } from "vitest";

import { groupThreadsByWorkspace, formatRelativeTime, threadStatusIcon } from "@/lib/thread-utils";
import type { ThreadSummary, WorkspaceSummary } from "@/lib/runtime-api";

function makeThread(overrides: Partial<ThreadSummary> = {}): ThreadSummary {
  return {
    id: "t1",
    title: "Test thread",
    preview: "hello",
    model: "deepseek-chat",
    mode: "agent",
    archived: false,
    updated_at: new Date().toISOString(),
    ...overrides,
  };
}

describe("groupThreadsByWorkspace", () => {
  it("groups threads by workspace path", () => {
    const workspaces: WorkspaceSummary[] = [
      { id: "ws1", path: "/home/project-a", name: "project-a", thread_count: 0 },
      { id: "ws2", path: "/home/project-b", name: "project-b", thread_count: 0 },
    ];
    const threads: ThreadSummary[] = [
      makeThread({ id: "t1", workspace: "/home/project-a" }),
      makeThread({ id: "t2", workspace: "/home/project-b" }),
      makeThread({ id: "t3", workspace: "/home/project-a" }),
    ];

    const groups = groupThreadsByWorkspace(threads, workspaces);

    expect(groups).toHaveLength(2);
    expect(groups[0].workspace.name).toBe("project-a");
    expect(groups[0].threads).toHaveLength(2);
    expect(groups[1].workspace.name).toBe("project-b");
    expect(groups[1].threads).toHaveLength(1);
  });

  it("puts threads without workspace into Ungrouped", () => {
    const workspaces: WorkspaceSummary[] = [
      { id: "ws1", path: "/home/project-a", name: "project-a", thread_count: 0 },
    ];
    const threads: ThreadSummary[] = [
      makeThread({ id: "t1", workspace: "/home/project-a" }),
      makeThread({ id: "t2", workspace: null }),
      makeThread({ id: "t3" }),
    ];

    const groups = groupThreadsByWorkspace(threads, workspaces);

    expect(groups).toHaveLength(2);
    expect(groups[0].workspace.name).toBe("project-a");
    expect(groups[0].threads).toHaveLength(1);
    expect(groups[1].workspace.name).toBe("Ungrouped");
    expect(groups[1].threads).toHaveLength(2);
  });

  it("returns empty array for no threads", () => {
    const workspaces: WorkspaceSummary[] = [
      { id: "ws1", path: "/a", name: "a", thread_count: 0 },
    ];
    const groups = groupThreadsByWorkspace([], workspaces);
    expect(groups).toHaveLength(0);
  });

  it("preserves order within groups", () => {
    const workspaces: WorkspaceSummary[] = [
      { id: "ws1", path: "/home/p", name: "p", thread_count: 0 },
    ];
    const threads = [
      makeThread({ id: "t1", workspace: "/home/p", title: "First" }),
      makeThread({ id: "t2", workspace: "/home/p", title: "Second" }),
    ];

    const groups = groupThreadsByWorkspace(threads, workspaces);
    expect(groups[0].threads[0].title).toBe("First");
    expect(groups[0].threads[1].title).toBe("Second");
  });
});

describe("threadStatusIcon", () => {
  it("returns loader for in_progress and queued", () => {
    expect(threadStatusIcon("in_progress")).toBe("loader");
    expect(threadStatusIcon("queued")).toBe("loader");
  });

  it("returns check for completed", () => {
    expect(threadStatusIcon("completed")).toBe("check");
  });

  it("returns x for failed", () => {
    expect(threadStatusIcon("failed")).toBe("x");
  });

  it("returns minus for interrupted, canceled, null, undefined", () => {
    expect(threadStatusIcon("interrupted")).toBe("minus");
    expect(threadStatusIcon("canceled")).toBe("minus");
    expect(threadStatusIcon(null)).toBe("minus");
    expect(threadStatusIcon(undefined)).toBe("minus");
  });
});

describe("formatRelativeTime", () => {
  it("returns 'now' for less than 60 seconds ago", () => {
    const now = new Date().toISOString();
    expect(formatRelativeTime(now)).toBe("now");
  });

  it("returns minutes for less than 60 minutes ago", () => {
    const fiveMinAgo = new Date(Date.now() - 5 * 60_000).toISOString();
    expect(formatRelativeTime(fiveMinAgo)).toBe("5m");
  });

  it("returns hours for less than 24 hours ago", () => {
    const twoHoursAgo = new Date(Date.now() - 2 * 3600_000).toISOString();
    expect(formatRelativeTime(twoHoursAgo)).toBe("2h");
  });

  it("returns days for more than 24 hours ago", () => {
    const threeDaysAgo = new Date(Date.now() - 3 * 86_400_000).toISOString();
    expect(formatRelativeTime(threeDaysAgo)).toBe("3d");
  });

  it("returns '-' for null/undefined", () => {
    expect(formatRelativeTime(null)).toBe("-");
    expect(formatRelativeTime(undefined)).toBe("-");
  });
});
