export type PaletteMode = "commands" | "sessions";

export type EscapeContext = {
  paletteOpen: boolean;
  paletteMode: PaletteMode;
  hasTaskDetail: boolean;
  hasFocusedElement: boolean;
  hasNotice: boolean;
};

export type EscapeAction =
  | "close-palette"
  | "switch-palette-mode"
  | "close-task-detail"
  | "blur-focused-element"
  | "clear-notices"
  | "noop";

export function resolveEscapeAction(context: EscapeContext): EscapeAction {
  if (context.paletteOpen) {
    if (context.paletteMode === "sessions") {
      return "switch-palette-mode";
    }
    return "close-palette";
  }
  if (context.hasTaskDetail) {
    return "close-task-detail";
  }
  if (context.hasFocusedElement) {
    return "blur-focused-element";
  }
  if (context.hasNotice) {
    return "clear-notices";
  }
  return "noop";
}
