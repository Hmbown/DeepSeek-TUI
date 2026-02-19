import { describe, expect, it } from "vitest";

import { KEYBOARD_SHORTCUTS } from "@/lib/keyboard-shortcuts";

describe("keyboard shortcuts catalog", () => {
  it("lists global, palette, thread list, and composer shortcuts", () => {
    const ids = new Set(KEYBOARD_SHORTCUTS.map((shortcut) => shortcut.id));

    expect(ids.has("open-palette")).toBe(true);
    expect(ids.has("open-sessions")).toBe(true);
    expect(ids.has("new-thread")).toBe(true);
    expect(ids.has("focus-threads")).toBe(true);
    expect(ids.has("focus-composer")).toBe(true);
    expect(ids.has("focus-events")).toBe(true);
    expect(ids.has("escape")).toBe(true);
    expect(ids.has("palette-up")).toBe(true);
    expect(ids.has("palette-down")).toBe(true);
    expect(ids.has("palette-enter")).toBe(true);
    expect(ids.has("palette-tab")).toBe(true);
    expect(ids.has("thread-list-home")).toBe(true);
    expect(ids.has("thread-list-end")).toBe(true);
    expect(ids.has("composer-send")).toBe(true);
    expect(ids.has("composer-shift-enter")).toBe(true);
    expect(ids.has("composer-alt-enter")).toBe(true);
    expect(ids.has("composer-ctrl-j")).toBe(true);
  });
});
