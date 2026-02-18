import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ThreadsPane } from "@/components/threads/ThreadsPane";

describe("ThreadsPane keyboard and actions", () => {
  it("moves selection with arrow keys and invokes archive action", () => {
    const onSelect = vi.fn();
    const onArchive = vi.fn();

    render(
      <ThreadsPane
        threads={[
          {
            id: "thr_1",
            title: "First",
            preview: "A",
            model: "deepseek-reasoner",
            mode: "agent",
            archived: false,
            updated_at: "2026-01-01T00:00:00Z",
            latest_turn_status: "completed",
          },
          {
            id: "thr_2",
            title: "Second",
            preview: "B",
            model: "deepseek-chat",
            mode: "plan",
            archived: false,
            updated_at: "2026-01-01T00:00:00Z",
            latest_turn_status: "in_progress",
          },
        ]}
        selectedThreadId={"thr_1"}
        threadSearch={""}
        threadFilter={"active"}
        onThreadSearchChange={vi.fn()}
        onThreadFilterChange={vi.fn()}
        onThreadSelect={onSelect}
        onThreadArchiveToggle={onArchive}
      />
    );

    const list = document.querySelector(".threads-list");
    expect(list).not.toBeNull();
    if (list) {
      fireEvent.keyDown(list, { key: "ArrowDown" });
    }
    expect(onSelect).toHaveBeenCalledWith("thr_2");

    fireEvent.click(screen.getAllByRole("button", { name: "Archive thread" })[0]);
    expect(onArchive).toHaveBeenCalled();
  });
});
