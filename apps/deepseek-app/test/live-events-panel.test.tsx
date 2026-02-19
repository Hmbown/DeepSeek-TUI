import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { LiveEventsPanel } from "@/components/chat/LiveEventsPanel";

const runState = {
  state: "running" as const,
  tone: "warning" as const,
  label: "Running",
  reason: "Turn or task in progress",
  reasonSource: "active-turn" as const,
};

afterEach(cleanup);

describe("LiveEventsPanel", () => {
  it("renders pinned critical notices and overflow controls", () => {
    const toggle = vi.fn();
    const filterChange = vi.fn();
    render(
      <LiveEventsPanel
        events={[
          {
            id: "evt-1",
            event: "item.delta",
            summary: "Tool output chunk",
            count: 3,
            critical: false,
          },
        ]}
        pinnedCritical={[
          {
            id: "evt-critical",
            event: "approval.required",
            summary: "Approval required",
            count: 1,
            critical: true,
          },
        ]}
        overflowCount={5}
        showAllEvents={false}
        eventFilter={"all"}
        runState={runState}
        canResume={true}
        canFork={true}
        canInterrupt={true}
        canCompact={true}
        steerText={"keep it concise"}
        onEventFilterChange={filterChange}
        onSteerTextChange={vi.fn()}
        onResume={vi.fn()}
        onFork={vi.fn()}
        onInterrupt={vi.fn()}
        onCompact={vi.fn()}
        onSteer={vi.fn()}
        onToggleEventOverflow={toggle}
      />
    );

    expect(screen.getByText("Approval required")).toBeInTheDocument();
    expect(screen.getByText("x3")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: /show tool events only/i }));
    expect(filterChange).toHaveBeenCalledWith("tool");
    fireEvent.click(screen.getByRole("button", { name: /show all events/i }));
    expect(toggle).toHaveBeenCalledTimes(1);
  });

  it("disables steer when interrupt is unavailable", () => {
    render(
      <LiveEventsPanel
        events={[]}
        pinnedCritical={[]}
        overflowCount={0}
        showAllEvents={false}
        eventFilter={"all"}
        runState={runState}
        canResume={false}
        canFork={false}
        canInterrupt={false}
        canCompact={false}
        steerText={"try again"}
        onEventFilterChange={vi.fn()}
        onSteerTextChange={vi.fn()}
        onResume={vi.fn()}
        onFork={vi.fn()}
        onInterrupt={vi.fn()}
        onCompact={vi.fn()}
        onSteer={vi.fn()}
        onToggleEventOverflow={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: /send steer/i })).toBeDisabled();
  });
});
