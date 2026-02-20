import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Composer } from "@/components/chat/Composer";

afterEach(cleanup);

const baseProps = {
  onValueChange: vi.fn(),
  onRetrySend: vi.fn(),
  sending: false as const,
  selectedThreadId: "thr_123",
  activeTurnId: null,
  mode: "agent",
  onModeChange: vi.fn(),
};

describe("Composer keyboard behavior", () => {
  it("sends on Enter and does not send on Shift+Enter", () => {
    const onSend = vi.fn();

    render(
      <Composer
        {...baseProps}
        value="hello"
        onSend={onSend}
      />
    );

    const textarea = screen.getByPlaceholderText("Type a prompt\u2026");
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true });
    expect(onSend).toHaveBeenCalledTimes(1);
  });

  it("shows blocked-send reason and allows manual retry", () => {
    const onSend = vi.fn();
    const onRetrySend = vi.fn();
    render(
      <Composer
        {...baseProps}
        value="hello"
        onSend={onSend}
        onRetrySend={onRetrySend}
        blockedSendReason="Send blocked: runtime offline."
        canRetryBlockedSend={true}
      />
    );

    expect(screen.getByText("Send blocked: runtime offline.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^send$/i })).toBeDisabled();

    const textarea = screen.getByPlaceholderText("Type a prompt\u2026");
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).toHaveBeenCalledTimes(0);

    fireEvent.click(screen.getByRole("button", { name: /retry send/i }));
    expect(onRetrySend).toHaveBeenCalledTimes(1);
  });

  it("disables Send button when blockedSendReason is set", () => {
    render(
      <Composer
        {...baseProps}
        value="hello"
        onSend={vi.fn()}
        blockedSendReason="Offline"
      />
    );

    expect(screen.getByRole("button", { name: /^send$/i })).toBeDisabled();
  });

  it("adds a newline on Ctrl/Cmd+J and Alt+Enter", () => {
    const onValueChange = vi.fn();

    render(
      <Composer
        {...baseProps}
        value="hello"
        onValueChange={onValueChange}
        onSend={vi.fn()}
      />
    );

    const textarea = screen.getByPlaceholderText("Type a prompt…");
    fireEvent.keyDown(textarea, { key: "j", ctrlKey: true });
    expect(onValueChange).toHaveBeenCalledWith("hello\n");

    fireEvent.keyDown(textarea, { key: "j", metaKey: true });
    expect(onValueChange).toHaveBeenCalledWith("hello\n");

    fireEvent.keyDown(textarea, { key: "Enter", altKey: true });
    expect(onValueChange).toHaveBeenCalledWith("hello\n");
  });
});
