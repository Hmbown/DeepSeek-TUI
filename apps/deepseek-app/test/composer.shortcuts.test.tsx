import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Composer } from "@/components/chat/Composer";

afterEach(cleanup);

describe("Composer keyboard behavior", () => {
  it("sends on Enter and does not send on Shift+Enter", () => {
    const onSend = vi.fn();
    const onValueChange = vi.fn();

    render(
      <Composer
        value="hello"
        onValueChange={onValueChange}
        onSend={onSend}
        onRetrySend={vi.fn()}
        sending={false}
        selectedThreadId={"thr_123"}
        activeTurnId={null}
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
        value="hello"
        onValueChange={vi.fn()}
        onSend={onSend}
        onRetrySend={onRetrySend}
        sending={false}
        selectedThreadId={"thr_123"}
        activeTurnId={null}
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
        value="hello"
        onValueChange={vi.fn()}
        onSend={vi.fn()}
        onRetrySend={vi.fn()}
        sending={false}
        selectedThreadId={"thr_123"}
        activeTurnId={null}
        blockedSendReason="Offline"
      />
    );

    expect(screen.getByRole("button", { name: /^send$/i })).toBeDisabled();
  });
});
