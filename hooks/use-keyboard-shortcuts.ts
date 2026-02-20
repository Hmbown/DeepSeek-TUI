import { useEffect } from "react";

export type KeyboardShortcutHandlers = {
  onOpenPalette: () => void;
  onOpenSessions: () => void;
  onNewThread: () => void;
  onFocusThreads?: () => void;
  onFocusComposer?: () => void;
  onFocusEvents?: () => void;
  onEscape: () => void;
};

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  if (target.isContentEditable) {
    return true;
  }
  const tag = target.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select";
}

export function useKeyboardShortcuts(handlers: KeyboardShortcutHandlers): void {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      const hasMod = event.metaKey || event.ctrlKey;
      const editable = isEditableTarget(event.target);

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
        if (editable) {
          return;
        }
        event.preventDefault();
        handlers.onNewThread();
        return;
      }
      if (hasMod && key === "1") {
        if (editable) {
          return;
        }
        event.preventDefault();
        handlers.onFocusThreads?.();
        return;
      }
      if (hasMod && key === "2") {
        event.preventDefault();
        handlers.onFocusComposer?.();
        return;
      }
      if (hasMod && key === "3") {
        if (editable) {
          return;
        }
        event.preventDefault();
        handlers.onFocusEvents?.();
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
