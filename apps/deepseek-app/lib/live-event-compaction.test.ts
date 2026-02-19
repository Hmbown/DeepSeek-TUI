import { describe, expect, it } from "vitest";

import { compactLiveEvents } from "@/lib/live-event-compaction";

describe("compactLiveEvents", () => {
  it("compacts adjacent non-critical events", () => {
    const result = compactLiveEvents(
      [
        { event: "item.delta", payload: "chunk" },
        { event: "item.delta", payload: "chunk" },
        { event: "item.delta", payload: "chunk" },
      ],
      40
    );

    expect(result.rows).toHaveLength(1);
    expect(result.rows[0].count).toBe(3);
  });

  it("keeps critical events visible beyond limit", () => {
    const many = Array.from({ length: 45 }, (_, index) => ({
      event: "item.delta",
      payload: `event-${index}`,
      seq: index,
    }));
    const events = [
      ...many,
      {
        event: "approval.required",
        payload: { summary: "Need approval" },
        seq: 999,
      },
    ];

    const result = compactLiveEvents(events, 40);
    expect(result.rows.some((row) => row.event === "approval.required")).toBe(true);
    expect(result.pinnedCritical).toHaveLength(1);
    expect(result.overflowCount).toBeGreaterThan(0);
  });
});
