import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Composer } from "@/components/chat/Composer";

describe("Composer keyboard behavior", () => {
  it("sends on Enter and does not send on Shift+Enter", () => {
    const onSend = vi.fn();
    const onValueChange = vi.fn();

    render(
      <Composer
        value="hello"
        onValueChange={onValueChange}
        onSend={onSend}
        sending={false}
        selectedThreadId={"thr_123"}
        activeTurnId={null}
      />
    );

    const textarea = screen.getByPlaceholderText("Type a prompt…");
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true });
    expect(onSend).toHaveBeenCalledTimes(1);
  });
});
