import { useEffect } from "react";

export type KeyboardShortcutHandlers = {
  onOpenPalette: () => void;
  onOpenSessions: () => void;
  onNewThread: () => void;
  onEscape: () => void;
};

export function useKeyboardShortcuts(handlers: KeyboardShortcutHandlers): void {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      const hasMod = event.metaKey || event.ctrlKey;

      if (hasMod && key === "k") {
        event.preventDefault();
        handlers.onOpenPalette();
        return;
      }
      if (hasMod && key === "r") {
        event.preventDefault();
        handlers.onOpenSessions();
        return;
      }
      if (hasMod && key === "n") {
        event.preventDefault();
        handlers.onNewThread();
        return;
      }
      if (key === "escape") {
        handlers.onEscape();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [handlers]);
}
