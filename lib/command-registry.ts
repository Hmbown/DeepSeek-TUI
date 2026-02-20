import type { Section } from "@/components/types";
import type { CommandPaletteItem } from "@/components/palette/CommandPalette";
import type { AutomationRecord, SessionMetadata } from "@/lib/runtime-api";

export type CommandRegistryContext = {
  pendingApprovalCount: number;
  selectedThreadId: string | null;
  activeTurnId: string | null;
  currentAutomation: AutomationRecord | null;
  onNewThread: () => void;
  onFocusThreads: () => void;
  onFocusComposer: () => void;
  onFocusEvents: () => void;
  onOpenSection: (section: Section) => void;
  onOpenSessions: () => void;
  onReviewApprovals: () => void;
  onResumeThread: () => void;
  onForkThread: () => void;
  onCompactThread: () => void;
  onInterruptTurn: () => void;
  onRunAutomation: (automationId: string) => void;
};

export type SessionRegistryContext = {
  sessions: SessionMetadata[];
  formatRelative: (value?: string | null) => string;
  onResumeSession: (session: SessionMetadata) => void;
  onDeleteSession: (session: SessionMetadata) => void;
};

export function buildSessionPaletteItems(context: SessionRegistryContext): CommandPaletteItem[] {
  return context.sessions.map((session) => ({
    id: session.id,
    label: session.title,
    description: `${session.model} · ${context.formatRelative(session.updated_at)} · ${session.message_count} messages`,
    keywords: [session.workspace, session.mode ?? "", session.model].filter(Boolean),
    group: "Sessions",
    action: () => {
      context.onResumeSession(session);
    },
    secondaryAction: {
      label: "Delete",
      action: (event) => {
        event.stopPropagation();
        context.onDeleteSession(session);
      },
    },
  }));
}

export function buildCommandPaletteItems(context: CommandRegistryContext): CommandPaletteItem[] {
  const items: CommandPaletteItem[] = [
    {
      id: "new-thread",
      label: "New thread",
      description: "Start a fresh conversation thread",
      shortcut: "Ctrl/Cmd+N",
      keywords: ["create", "conversation", "thread"],
      group: "Thread",
      action: context.onNewThread,
    },
    {
      id: "focus-threads",
      label: "Focus threads pane",
      description: "Jump focus to thread list and search",
      shortcut: "Ctrl/Cmd+1",
      keywords: ["pane", "left", "threads"],
      group: "Focus",
      action: context.onFocusThreads,
    },
    {
      id: "focus-composer",
      label: "Focus composer",
      description: "Jump focus to chat composer",
      shortcut: "Ctrl/Cmd+2",
      keywords: ["pane", "message", "input"],
      group: "Focus",
      action: context.onFocusComposer,
    },
    {
      id: "focus-events",
      label: "Focus live events",
      description: "Jump focus to live events and steer input",
      shortcut: "Ctrl/Cmd+3",
      keywords: ["pane", "events", "steer"],
      group: "Focus",
      action: context.onFocusEvents,
    },
    {
      id: "open-chat",
      label: "Open chat",
      keywords: ["section", "thread", "conversation"],
      group: "Navigation",
      action: () => context.onOpenSection("chat"),
    },
    {
      id: "open-automations",
      label: "Open automations",
      keywords: ["section", "schedules", "runs"],
      group: "Navigation",
      action: () => context.onOpenSection("automations"),
    },
    {
      id: "open-skills",
      label: "Open skills & apps",
      keywords: ["section", "mcp", "tools", "skills"],
      group: "Navigation",
      action: () => context.onOpenSection("skills"),
    },
    {
      id: "open-settings",
      label: "Open settings",
      keywords: ["section", "runtime", "endpoint", "tasks"],
      group: "Navigation",
      action: () => context.onOpenSection("settings"),
    },
    {
      id: "open-sessions",
      label: "Search sessions",
      shortcut: "Ctrl/Cmd+R",
      keywords: ["history", "resume", "sessions"],
      group: "Sessions",
      action: context.onOpenSessions,
    },
  ];

  if (context.pendingApprovalCount > 0) {
    items.push({
      id: "view-pending-approvals",
      label: "Review pending approvals",
      description: `${context.pendingApprovalCount} approval request${context.pendingApprovalCount === 1 ? "" : "s"} pending`,
      keywords: ["approval", "security", "pending"],
      group: "Approvals",
      action: context.onReviewApprovals,
    });
  }

  if (context.selectedThreadId) {
    items.push(
      {
        id: "resume-thread",
        label: "Resume current thread",
        keywords: ["thread", "continue", "resume"],
        group: "Thread",
        action: context.onResumeThread,
      },
      {
        id: "fork-thread",
        label: "Fork current thread",
        keywords: ["thread", "duplicate", "fork"],
        group: "Thread",
        action: context.onForkThread,
      },
      {
        id: "compact-thread",
        label: "Compact current thread",
        keywords: ["thread", "compact", "summarize"],
        group: "Thread",
        action: context.onCompactThread,
      }
    );

    if (context.activeTurnId) {
      items.push({
        id: "interrupt-turn",
        label: "Interrupt active turn",
        keywords: ["stop", "cancel", "interrupt"],
        group: "Thread",
        action: context.onInterruptTurn,
      });
    }
  }

  if (context.currentAutomation) {
    const automationId = context.currentAutomation.id;
    items.push({
      id: "run-selected-automation",
      label: "Run selected automation",
      description: context.currentAutomation.name,
      keywords: ["automation", "run now", "task"],
      group: "Automations",
      action: () => {
        context.onRunAutomation(automationId);
      },
    });
  }

  return items;
}
