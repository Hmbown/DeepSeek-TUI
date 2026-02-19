import type { Section } from "@/components/types";
import type { ThreadSummary } from "@/lib/runtime-api";

export type CompactPane = "threads" | "transcript" | "events";

type PersistedUiState = {
  section: Section;
  threadId: string | null;
  pane: CompactPane;
};

const KEYS = {
  section: "deepseek.app.lastSection",
  threadId: "deepseek.app.lastThreadId",
  pane: "deepseek.app.lastPane",
} as const;

const VALID_SECTIONS: Set<Section> = new Set(["chat", "automations", "skills", "settings"]);
const VALID_PANES: Set<CompactPane> = new Set(["threads", "transcript", "events"]);

function canUseStorage(): boolean {
  return typeof window !== "undefined" && typeof window.localStorage !== "undefined";
}

export function loadPersistedUiState(): Partial<PersistedUiState> {
  if (!canUseStorage()) {
    return {};
  }

  const sectionRaw = window.localStorage.getItem(KEYS.section);
  const paneRaw = window.localStorage.getItem(KEYS.pane);
  const threadIdRaw = window.localStorage.getItem(KEYS.threadId);

  return {
    section: sectionRaw && VALID_SECTIONS.has(sectionRaw as Section) ? (sectionRaw as Section) : undefined,
    pane: paneRaw && VALID_PANES.has(paneRaw as CompactPane) ? (paneRaw as CompactPane) : undefined,
    threadId: threadIdRaw?.trim() ? threadIdRaw.trim() : null,
  };
}

export function persistLastSection(section: Section): void {
  if (!canUseStorage()) {
    return;
  }
  window.localStorage.setItem(KEYS.section, section);
}

export function persistLastThreadId(threadId: string | null): void {
  if (!canUseStorage()) {
    return;
  }
  if (!threadId) {
    window.localStorage.removeItem(KEYS.threadId);
    return;
  }
  window.localStorage.setItem(KEYS.threadId, threadId);
}

export function persistLastPane(pane: CompactPane): void {
  if (!canUseStorage()) {
    return;
  }
  window.localStorage.setItem(KEYS.pane, pane);
}

export function resolveRestoredThreadId(
  preferredThreadId: string | null,
  threads: ThreadSummary[]
): string | null {
  if (threads.length === 0) {
    return null;
  }
  if (preferredThreadId && threads.some((thread) => thread.id === preferredThreadId)) {
    return preferredThreadId;
  }
  return threads[0].id;
}
