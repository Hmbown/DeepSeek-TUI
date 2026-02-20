import { describe, expect, it } from "vitest";

import { buildTranscript, filterThreadSummaries, findActiveTurnId } from "@/lib/thread-utils";

describe("thread-utils", () => {
  it("filters thread summaries", () => {
    const filtered = filterThreadSummaries(
      [
        {
          id: "thr_1",
          title: "First thread",
          preview: "hello",
          model: "wagmii-reasoner",
          mode: "agent",
          archived: false,
          updated_at: "2026-01-01T00:00:00Z",
        },
        {
          id: "thr_2",
          title: "Second",
          preview: "world",
          model: "wagmii-chat",
          mode: "agent",
          archived: false,
          updated_at: "2026-01-01T00:00:00Z",
        },
      ],
      "first"
    );

    expect(filtered).toHaveLength(1);
    expect(filtered[0]?.id).toBe("thr_1");
  });

  it("builds transcript with user/assistant and operational items", () => {
    const transcript = buildTranscript({
      thread: {
        id: "thr_1",
        model: "wagmii-reasoner",
        mode: "agent",
        archived: false,
        updated_at: "2026-01-01T00:00:00Z",
      },
      turns: [
        {
          id: "turn_1",
          status: "completed",
          input_summary: "hello",
          created_at: "2026-01-01T00:00:00Z",
        },
      ],
      items: [
        {
          id: "item_user",
          turn_id: "turn_1",
          kind: "user_message",
          status: "completed",
          summary: "hello",
          detail: "hello",
        },
        {
          id: "item_tool",
          turn_id: "turn_1",
          kind: "tool_call",
          status: "completed",
          summary: "read_file completed",
          detail: "{\"path\":\"README.md\"}",
        },
        {
          id: "item_assistant",
          turn_id: "turn_1",
          kind: "agent_message",
          status: "completed",
          summary: "world",
          detail: "world",
        },
      ],
      latest_seq: 1,
    });

    expect(transcript).toHaveLength(3);
    expect(transcript[0]?.role).toBe("user");
    expect(transcript[1]?.role).toBe("tool");
    expect(transcript[2]?.role).toBe("assistant");
  });

  it("falls back to system transcript when no items exist", () => {
    const transcript = buildTranscript({
      thread: {
        id: "thr_1",
        model: "wagmii-reasoner",
        mode: "agent",
        archived: false,
        updated_at: "2026-01-01T00:00:00Z",
      },
      turns: [
        {
          id: "turn_1",
          status: "completed",
          input_summary: "fallback",
          created_at: "2026-01-01T00:00:00Z",
        },
      ],
      items: [],
      latest_seq: 1,
    });

    expect(transcript).toHaveLength(1);
    expect(transcript[0]?.role).toBe("system");
  });

  it("finds active turn", () => {
    const turnId = findActiveTurnId({
      thread: {
        id: "thr_1",
        model: "wagmii-reasoner",
        mode: "agent",
        archived: false,
        updated_at: "2026-01-01T00:00:00Z",
      },
      turns: [
        {
          id: "turn_1",
          status: "completed",
          input_summary: "done",
          created_at: "2026-01-01T00:00:00Z",
        },
        {
          id: "turn_2",
          status: "in_progress",
          input_summary: "working",
          created_at: "2026-01-01T01:00:00Z",
        },
      ],
      items: [],
      latest_seq: 2,
    });

    expect(turnId).toBe("turn_2");
  });
});
