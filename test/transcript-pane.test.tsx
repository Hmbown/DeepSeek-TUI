import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";

import { TranscriptPane } from "@/components/chat/TranscriptPane";
import type { TranscriptCell } from "@/lib/thread-utils";

afterEach(cleanup);

const shortMessage: TranscriptCell = {
  id: "item_1",
  role: "user",
  title: "You",
  content: "Hello, world!",
  status: "completed",
  startedAt: "2025-01-01T00:00:00Z",
};

const assistantMessage: TranscriptCell = {
  id: "item_2",
  role: "assistant",
  title: "Assistant",
  content: "Hi there! How can I help?",
  status: "completed",
};

const longToolOutput: TranscriptCell = {
  id: "item_3",
  role: "tool",
  title: "Tool",
  content: Array.from({ length: 20 }, (_, i) => `output line ${i + 1}`).join("\n"),
  status: "completed",
};

describe("TranscriptPane", () => {
  it("shows empty state when no thread selected", () => {
    render(<TranscriptPane transcript={[]} selectedThreadId={null} />);
    expect(screen.getByText("Create or select a thread to begin.")).toBeTruthy();
  });

  it("shows empty state when transcript is empty", () => {
    render(<TranscriptPane transcript={[]} selectedThreadId="thr_1" />);
    expect(screen.getByText("No messages yet.")).toBeTruthy();
  });

  it("renders transcript cells with role-based classes", () => {
    const { container } = render(
      <TranscriptPane
        transcript={[shortMessage, assistantMessage]}
        selectedThreadId="thr_1"
      />
    );
    expect(container.querySelector(".role-user")).toBeTruthy();
    expect(container.querySelector(".role-assistant")).toBeTruthy();
  });

  it("shows copy buttons for each transcript item", () => {
    render(
      <TranscriptPane
        transcript={[shortMessage, assistantMessage]}
        selectedThreadId="thr_1"
      />
    );
    const copyButtons = screen.getAllByLabelText("Copy to clipboard");
    expect(copyButtons.length).toBe(2);
  });

  it("collapses long tool output by default", () => {
    render(
      <TranscriptPane
        transcript={[longToolOutput]}
        selectedThreadId="thr_1"
      />
    );
    expect(screen.getByLabelText("Expand")).toBeTruthy();
  });

  it("expands collapsed content on click", () => {
    render(
      <TranscriptPane
        transcript={[longToolOutput]}
        selectedThreadId="thr_1"
      />
    );
    const expandBtn = screen.getByLabelText("Expand");
    fireEvent.click(expandBtn);
    expect(screen.getByLabelText("Collapse")).toBeTruthy();
  });

  it("filters transcript by role using filter toolbar", () => {
    render(
      <TranscriptPane
        transcript={[shortMessage, assistantMessage, longToolOutput]}
        selectedThreadId="thr_1"
      />
    );

    const toolbar = screen.getByRole("toolbar");
    const userFilterBtn = within(toolbar).getByText("User");
    fireEvent.click(userFilterBtn);

    expect(screen.getByText("Hello, world!")).toBeTruthy();
    expect(screen.queryByText("Hi there! How can I help?")).toBeNull();
  });

  it("shows empty filter state when no items match", () => {
    render(
      <TranscriptPane
        transcript={[shortMessage]}
        selectedThreadId="thr_1"
      />
    );

    const toolbar = screen.getByRole("toolbar");
    const errorFilterBtn = within(toolbar).getByText("Error");
    fireEvent.click(errorFilterBtn);

    expect(screen.getByText("No items match the current filter or search.")).toBeTruthy();
  });

  it("filters transcript by search query", () => {
    render(
      <TranscriptPane
        transcript={[shortMessage, assistantMessage]}
        selectedThreadId="thr_1"
      />
    );

    const searchInput = screen.getByPlaceholderText("Search transcript...");
    fireEvent.change(searchInput, { target: { value: "hello" } });
    expect(screen.getByText("Hello, world!")).toBeTruthy();
    expect(screen.queryByText("Hi there! How can I help?")).toBeNull();
  });

  it("combines role filter and search", () => {
    render(
      <TranscriptPane
        transcript={[shortMessage, assistantMessage, longToolOutput]}
        selectedThreadId="thr_1"
      />
    );

    const toolbar = screen.getByRole("toolbar");
    const assistantBtn = within(toolbar).getByText("Assistant");
    fireEvent.click(assistantBtn);

    const searchInput = screen.getByPlaceholderText("Search transcript...");
    fireEvent.change(searchInput, { target: { value: "help" } });
    expect(screen.getByText(/Hi there/)).toBeTruthy();
    expect(screen.queryByText("Hello, world!")).toBeNull();
  });

  it("resets to all when 'All' chip clicked", () => {
    render(
      <TranscriptPane
        transcript={[shortMessage, assistantMessage]}
        selectedThreadId="thr_1"
      />
    );

    const toolbar = screen.getByRole("toolbar");
    const userFilterBtn = within(toolbar).getByText("User");
    fireEvent.click(userFilterBtn);
    expect(screen.queryByText("Hi there! How can I help?")).toBeNull();

    const allFilterBtn = within(toolbar).getByText("All");
    fireEvent.click(allFilterBtn);
    expect(screen.getByText("Hi there! How can I help?")).toBeTruthy();
  });
});
