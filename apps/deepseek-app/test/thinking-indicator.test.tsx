import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { TranscriptPane } from "@/components/chat/TranscriptPane";
import type { TranscriptCell } from "@/lib/thread-utils";

afterEach(cleanup);

const sampleTranscript: TranscriptCell[] = [
  {
    id: "msg-1",
    role: "user",
    title: "You",
    content: "Hello",
    turnId: "turn-1",
    status: "completed",
  },
  {
    id: "msg-2",
    role: "assistant",
    title: "Assistant",
    content: "Hi there! How can I help?",
    turnId: "turn-1",
    status: "completed",
    reasoning_content: "The user greeted me. I should respond in a friendly manner.",
  },
];

describe("ThinkingIndicator", () => {
  it("shows thinking indicator when activeTurnId is set", () => {
    render(
      <TranscriptPane
        transcript={sampleTranscript}
        selectedThreadId="thr-1"
        activeTurnId="turn-active"
      />
    );

    expect(screen.getByText("Thinking...")).toBeInTheDocument();
  });

  it("hides thinking indicator when activeTurnId is null", () => {
    render(
      <TranscriptPane
        transcript={sampleTranscript}
        selectedThreadId="thr-1"
        activeTurnId={null}
      />
    );

    expect(screen.queryByText("Thinking...")).toBeNull();
  });
});

describe("ReasoningBlock", () => {
  it("shows reasoning toggle for cells with reasoning_content", () => {
    render(
      <TranscriptPane
        transcript={sampleTranscript}
        selectedThreadId="thr-1"
      />
    );

    expect(screen.getByText("Reasoning")).toBeInTheDocument();
  });

  it("expands reasoning content on click", () => {
    render(
      <TranscriptPane
        transcript={sampleTranscript}
        selectedThreadId="thr-1"
      />
    );

    expect(screen.queryByText(/The user greeted me/)).toBeNull();

    const toggle = screen.getByRole("button", { name: /reasoning/i });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");

    expect(
      screen.getByText(/The user greeted me/)
    ).toBeInTheDocument();
  });
});
