import { describe, expect, it } from "vitest";

import {
  filterLiveEvents,
  filterLiveEventRows,
  isToolEvent,
  isTurnLifecycleEvent,
  matchesLiveEventFilter,
} from "@/lib/live-event-filters";

const SAMPLE = [
  {
    id: "evt-tool",
    event: "item.completed",
    summary: "Tool finished",
    count: 1,
    critical: false,
  },
  {
    id: "evt-turn",
    event: "turn.lifecycle",
    summary: "Turn progressed",
    count: 1,
    critical: false,
  },
  {
    id: "evt-critical",
    event: "approval.required",
    summary: "Approval needed",
    count: 1,
    critical: true,
  },
];

describe("live-event-filters", () => {
  it("categorizes tool and turn lifecycle events", () => {
    expect(isToolEvent("item.delta")).toBe(true);
    expect(isToolEvent("turn.lifecycle")).toBe(false);
    expect(isTurnLifecycleEvent("turn.lifecycle")).toBe(true);
    expect(isTurnLifecycleEvent("thread.updated")).toBe(true);
    expect(isTurnLifecycleEvent("item.completed")).toBe(false);
  });

  it("matches filters deterministically", () => {
    expect(matchesLiveEventFilter(SAMPLE[0], "tool")).toBe(true);
    expect(matchesLiveEventFilter(SAMPLE[0], "turn")).toBe(false);
    expect(matchesLiveEventFilter(SAMPLE[1], "turn")).toBe(true);
    expect(matchesLiveEventFilter(SAMPLE[2], "critical")).toBe(true);
  });

  it("filters rows by selected mode", () => {
    expect(filterLiveEventRows(SAMPLE, "all")).toHaveLength(3);
    expect(filterLiveEventRows(SAMPLE, "tool")).toHaveLength(1);
    expect(filterLiveEventRows(SAMPLE, "turn")).toHaveLength(1);
    expect(filterLiveEventRows(SAMPLE, "critical")).toHaveLength(1);
  });

  it("filters raw payloads by selected mode", () => {
    const raw = [
      {
        event: "item.completed",
        payload: {},
      },
      {
        event: "turn.lifecycle",
        payload: {},
      },
      {
        event: "approval.required",
        payload: {},
      },
    ];

    expect(filterLiveEvents(raw, "tool")).toHaveLength(1);
    expect(filterLiveEvents(raw, "turn")).toHaveLength(1);
    expect(filterLiveEvents(raw, "critical")).toHaveLength(1);
  });
});
