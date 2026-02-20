import { describe, expect, it } from "vitest";

import { resolveEscapeAction } from "@/lib/escape-behavior";

describe("resolveEscapeAction", () => {
  const base = {
    paletteOpen: false,
    paletteMode: "commands" as const,
    hasTaskDetail: false,
    hasFocusedElement: false,
    hasNotice: false,
  };

  it("switches sessions palette mode before closing palette", () => {
    const action = resolveEscapeAction({
      ...base,
      paletteOpen: true,
      paletteMode: "sessions",
      hasTaskDetail: true,
      hasFocusedElement: true,
      hasNotice: true,
    });
    expect(action).toBe("switch-palette-mode");
  });

  it("closes palette when open in commands mode", () => {
    const action = resolveEscapeAction({
      ...base,
      paletteOpen: true,
      paletteMode: "commands",
    });
    expect(action).toBe("close-palette");
  });

  it("closes task detail before blur or notices", () => {
    const action = resolveEscapeAction({
      ...base,
      hasTaskDetail: true,
      hasFocusedElement: true,
      hasNotice: true,
    });
    expect(action).toBe("close-task-detail");
  });

  it("blurs focused element before clearing notices", () => {
    const action = resolveEscapeAction({
      ...base,
      hasFocusedElement: true,
      hasNotice: true,
    });
    expect(action).toBe("blur-focused-element");
  });

  it("clears notices when no higher-priority action exists", () => {
    const action = resolveEscapeAction({
      ...base,
      hasNotice: true,
    });
    expect(action).toBe("clear-notices");
  });

  it("returns noop when all flags are false", () => {
    const action = resolveEscapeAction(base);
    expect(action).toBe("noop");
  });
});
