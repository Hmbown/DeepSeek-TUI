export type KeyboardShortcut = {
  id: string;
  keys: string;
  description: string;
  context: "global" | "palette" | "composer" | "thread-list";
};

export const KEYBOARD_SHORTCUTS: KeyboardShortcut[] = [
  {
    id: "open-palette",
    keys: "Ctrl/Cmd+K",
    description: "Open command palette",
    context: "global",
  },
  {
    id: "open-sessions",
    keys: "Ctrl/Cmd+R",
    description: "Search sessions",
    context: "global",
  },
  {
    id: "new-thread",
    keys: "Ctrl/Cmd+N",
    description: "Create thread",
    context: "global",
  },
  {
    id: "focus-threads",
    keys: "Ctrl/Cmd+1",
    description: "Focus threads pane",
    context: "global",
  },
  {
    id: "focus-composer",
    keys: "Ctrl/Cmd+2",
    description: "Focus composer",
    context: "global",
  },
  {
    id: "focus-events",
    keys: "Ctrl/Cmd+3",
    description: "Focus live events",
    context: "global",
  },
  {
    id: "escape",
    keys: "Esc",
    description: "Close overlays / clear notices",
    context: "global",
  },
  {
    id: "palette-up",
    keys: "ArrowUp",
    description: "Select previous palette item",
    context: "palette",
  },
  {
    id: "palette-down",
    keys: "ArrowDown",
    description: "Select next palette item",
    context: "palette",
  },
  {
    id: "palette-enter",
    keys: "Enter",
    description: "Run selected palette item",
    context: "palette",
  },
  {
    id: "palette-tab",
    keys: "Tab",
    description: "Cycle focus inside palette",
    context: "palette",
  },
  {
    id: "thread-list-up",
    keys: "ArrowUp",
    description: "Select previous thread",
    context: "thread-list",
  },
  {
    id: "thread-list-down",
    keys: "ArrowDown",
    description: "Select next thread",
    context: "thread-list",
  },
  {
    id: "thread-list-home",
    keys: "Home",
    description: "Jump to first thread",
    context: "thread-list",
  },
  {
    id: "thread-list-end",
    keys: "End",
    description: "Jump to last thread",
    context: "thread-list",
  },
  {
    id: "composer-send",
    keys: "Enter",
    description: "Send composer input",
    context: "composer",
  },
  {
    id: "composer-shift-enter",
    keys: "Shift+Enter",
    description: "Insert newline",
    context: "composer",
  },
  {
    id: "composer-alt-enter",
    keys: "Alt+Enter",
    description: "Insert newline",
    context: "composer",
  },
  {
    id: "composer-ctrl-j",
    keys: "Ctrl/Cmd+J",
    description: "Insert newline",
    context: "composer",
  },
];
