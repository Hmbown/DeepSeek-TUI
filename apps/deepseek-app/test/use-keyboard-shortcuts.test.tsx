import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useKeyboardShortcuts } from "@/hooks/use-keyboard-shortcuts";

function Harness(props: {
  onOpenPalette: () => void;
  onOpenSessions: () => void;
  onNewThread: () => void;
  onFocusThreads: () => void;
  onFocusComposer: () => void;
  onFocusEvents: () => void;
  onEscape: () => void;
}) {
  useKeyboardShortcuts(props);
  return <div>shortcut harness</div>;
}

describe("useKeyboardShortcuts", () => {
  it("triggers command handlers for configured shortcuts", () => {
    const handlers = {
      onOpenPalette: vi.fn(),
      onOpenSessions: vi.fn(),
      onNewThread: vi.fn(),
      onFocusThreads: vi.fn(),
      onFocusComposer: vi.fn(),
      onFocusEvents: vi.fn(),
      onEscape: vi.fn(),
    };
    render(<Harness {...handlers} />);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "r", ctrlKey: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "n", ctrlKey: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "1", ctrlKey: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "2", ctrlKey: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "3", ctrlKey: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));

    expect(handlers.onOpenPalette).toHaveBeenCalledTimes(1);
    expect(handlers.onOpenSessions).toHaveBeenCalledTimes(1);
    expect(handlers.onNewThread).toHaveBeenCalledTimes(1);
    expect(handlers.onFocusThreads).toHaveBeenCalledTimes(1);
    expect(handlers.onFocusComposer).toHaveBeenCalledTimes(1);
    expect(handlers.onFocusEvents).toHaveBeenCalledTimes(1);
    expect(handlers.onEscape).toHaveBeenCalledTimes(1);
  });
});
