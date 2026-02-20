import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";

import { TaskDetailPanel } from "@/components/tasks/TaskDetailPanel";
import type { TaskRecord } from "@/lib/runtime-api";

afterEach(cleanup);

const sampleTask: TaskRecord = {
  id: "task_abc123",
  status: "completed",
  prompt_summary: "Write a test suite",
  prompt: "Please write a comprehensive test suite for the auth module.",
  model: "wagmii-chat",
  mode: "agent",
  workspace: "/tmp/test-workspace",
  allow_shell: false,
  trust_mode: false,
  auto_approve: true,
  created_at: "2025-06-15T10:00:00Z",
  started_at: "2025-06-15T10:00:01Z",
  ended_at: "2025-06-15T10:01:00Z",
  duration_ms: 59000,
  thread_id: "thr_xyz789",
  turn_id: "turn_abc",
  result_summary: "All 12 tests pass.",
  result_detail_path: null,
  runtime_event_count: 24,
  error: null,
  tool_calls: [
    {
      id: "tc_1",
      name: "file_write",
      status: "success",
      started_at: "2025-06-15T10:00:10Z",
      ended_at: "2025-06-15T10:00:15Z",
      duration_ms: 5000,
      input_summary: "Write auth.test.ts",
      output_summary: "File written successfully",
      detail_path: null,
      patch_ref: null,
    },
  ],
  timeline: [
    {
      timestamp: "2025-06-15T10:00:00Z",
      kind: "task_queued",
      summary: "Task queued for execution",
      detail_path: null,
    },
    {
      timestamp: "2025-06-15T10:01:00Z",
      kind: "task_completed",
      summary: "Task completed successfully",
      detail_path: null,
    },
  ],
};

describe("TaskDetailPanel", () => {
  it("shows empty state when no task selected", () => {
    render(<TaskDetailPanel task={null} onClose={vi.fn()} />);
    expect(screen.getByText("Select a task to view details.")).toBeTruthy();
  });

  it("shows loading state", () => {
    render(<TaskDetailPanel task={null} loading={true} onClose={vi.fn()} />);
    expect(screen.getByText("Loading task...")).toBeTruthy();
  });

  it("renders task metadata", () => {
    render(<TaskDetailPanel task={sampleTask} onClose={vi.fn()} />);
    const panel = screen.getByLabelText("Task task_abc123");
    expect(within(panel).getByText("task_abc123")).toBeTruthy();
    expect(within(panel).getByText("wagmii-chat")).toBeTruthy();
    expect(within(panel).getByText("agent")).toBeTruthy();
  });

  it("renders prompt text", () => {
    render(<TaskDetailPanel task={sampleTask} onClose={vi.fn()} />);
    const panel = screen.getByLabelText("Task task_abc123");
    const promptElements = within(panel).getAllByText(
      "Please write a comprehensive test suite for the auth module."
    );
    expect(promptElements.length).toBeGreaterThanOrEqual(1);
  });

  it("renders result summary", () => {
    render(<TaskDetailPanel task={sampleTask} onClose={vi.fn()} />);
    const panel = screen.getByLabelText("Task task_abc123");
    expect(within(panel).getByText("All 12 tests pass.")).toBeTruthy();
  });

  it("renders timeline entries", () => {
    render(<TaskDetailPanel task={sampleTask} onClose={vi.fn()} />);
    const panel = screen.getByLabelText("Task task_abc123");
    expect(within(panel).getByText("Timeline (2)")).toBeTruthy();
    expect(within(panel).getByText("Task queued for execution")).toBeTruthy();
    expect(within(panel).getByText("Task completed successfully")).toBeTruthy();
  });

  it("renders tool calls", () => {
    render(<TaskDetailPanel task={sampleTask} onClose={vi.fn()} />);
    const panel = screen.getByLabelText("Task task_abc123");
    expect(within(panel).getByText("Tool calls (1)")).toBeTruthy();
    expect(within(panel).getByText("file_write")).toBeTruthy();
  });

  it("shows open thread button and calls handler", () => {
    const onOpenThread = vi.fn();
    render(
      <TaskDetailPanel task={sampleTask} onClose={vi.fn()} onOpenThread={onOpenThread} />
    );
    const openBtn = screen.getByText(/Open thread/);
    fireEvent.click(openBtn);
    expect(onOpenThread).toHaveBeenCalledWith("thr_xyz789");
  });

  it("calls onClose when close button clicked", () => {
    const onClose = vi.fn();
    render(<TaskDetailPanel task={sampleTask} onClose={onClose} />);
    const closeBtn = screen.getByLabelText("Close task detail");
    fireEvent.click(closeBtn);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
