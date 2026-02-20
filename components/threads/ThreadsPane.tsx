import { useState, type RefObject, type UIEvent } from "react";
import {
  Archive,
  ArchiveRestore,
  Check,
  ChevronDown,
  ChevronRight,
  GitBranch,
  Loader2,
  Minus,
  Search,
  X,
} from "lucide-react";

import type { ThreadFilter } from "@/components/types";
import type { ThreadSummary, WorkspaceSummary } from "@/lib/runtime-api";
import {
  formatRelativeTime,
  groupThreadsByWorkspace,
  threadStatusIcon,
  type ThreadGroup,
} from "@/lib/thread-utils";

type ThreadsPaneProps = {
  threads: ThreadSummary[];
  selectedThreadId: string | null;
  threadSearch: string;
  threadFilter: ThreadFilter;
  workspaces?: WorkspaceSummary[];
  collapsedFolders?: string[];
  className?: string;
  listRef?: RefObject<HTMLDivElement | null>;
  onScrollPositionChange?: (scrollTop: number) => void;
  onThreadSearchChange: (value: string) => void;
  onThreadFilterChange: (value: ThreadFilter) => void;
  onThreadSelect: (id: string) => void;
  onThreadArchiveToggle: (thread: ThreadSummary) => void;
  onToggleFolder?: (folderId: string) => void;
};

const FILTER_OPTIONS: Array<{ value: ThreadFilter; label: string }> = [
  { value: "active", label: "Active" },
  { value: "archived", label: "Archived" },
  { value: "all", label: "All" },
];

function statusLabel(value?: string | null): string {
  if (!value) {
    return "idle";
  }
  return value.replaceAll("_", " ");
}

function StatusIcon({ status }: { status?: string | null }) {
  const icon = threadStatusIcon(status);
  switch (icon) {
    case "loader":
      return <Loader2 size={13} className="thread-status-icon spin" />;
    case "check":
      return <Check size={13} className="thread-status-icon status-success" />;
    case "x":
      return <X size={13} className="thread-status-icon status-danger" />;
    case "minus":
    default:
      return <Minus size={13} className="thread-status-icon status-muted" />;
  }
}

function DiffStatChip({ stat }: { stat: { additions: number; deletions: number } }) {
  return (
    <span className="diff-stat-chip">
      <span className="diff-add">+{stat.additions}</span>
      <span className="diff-del">−{stat.deletions}</span>
    </span>
  );
}

function ThreadCard({
  thread,
  selected,
  onSelect,
  onArchiveToggle,
}: {
  thread: ThreadSummary;
  selected: boolean;
  onSelect: () => void;
  onArchiveToggle: () => void;
}) {
  return (
    <article className={`thread-card ${selected ? "is-selected" : ""}`}>
      <button className="thread-main" onClick={onSelect}>
        <div className="thread-row-layout">
          <StatusIcon status={thread.latest_turn_status} />
          <strong className="thread-title thread-title-truncate">{thread.title}</strong>
          {thread.diff_stat ? <DiffStatChip stat={thread.diff_stat} /> : null}
          <span className="thread-timestamp">{formatRelativeTime(thread.updated_at)}</span>
        </div>
        <div className="thread-preview">{thread.preview}</div>
        <div className="thread-meta">
          <span>{thread.model}</span>
          {thread.branch ? (
            <span className="branch-badge">
              <GitBranch size={11} />
              {thread.branch}
            </span>
          ) : null}
          <span className={`status-chip status-${thread.latest_turn_status ?? "idle"}`}>
            {statusLabel(thread.latest_turn_status)}
          </span>
        </div>
      </button>
      <div className="thread-actions">
        <button
          className="btn btn-ghost btn-sm"
          onClick={onArchiveToggle}
          aria-label={thread.archived ? "Unarchive thread" : "Archive thread"}
        >
          {thread.archived ? <ArchiveRestore size={14} /> : <Archive size={14} />}
          <span>{thread.archived ? "Unarchive" : "Archive"}</span>
        </button>
      </div>
    </article>
  );
}

function FolderGroup({
  group,
  collapsed,
  selectedThreadId,
  onToggle,
  onThreadSelect,
  onThreadArchiveToggle,
}: {
  group: ThreadGroup;
  collapsed: boolean;
  selectedThreadId: string | null;
  onToggle: () => void;
  onThreadSelect: (id: string) => void;
  onThreadArchiveToggle: (thread: ThreadSummary) => void;
}) {
  return (
    <div className="folder-group">
      <button className="folder-header" onClick={onToggle} aria-expanded={!collapsed}>
        {collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
        <span className="folder-name">{group.workspace.name}</span>
        <span className="folder-badge">{group.threads.length}</span>
      </button>
      {!collapsed ? (
        <div className="folder-threads">
          {group.threads.map((thread) => (
            <ThreadCard
              key={thread.id}
              thread={thread}
              selected={thread.id === selectedThreadId}
              onSelect={() => onThreadSelect(thread.id)}
              onArchiveToggle={() => onThreadArchiveToggle(thread)}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

export function ThreadsPane({
  threads,
  selectedThreadId,
  threadSearch,
  threadFilter,
  workspaces,
  collapsedFolders: collapsedFoldersProp,
  className,
  listRef,
  onScrollPositionChange,
  onThreadSearchChange,
  onThreadFilterChange,
  onThreadSelect,
  onThreadArchiveToggle,
  onToggleFolder,
}: ThreadsPaneProps) {
  const [localCollapsed, setLocalCollapsed] = useState<string[]>([]);
  const collapsedFolders = collapsedFoldersProp ?? localCollapsed;

  const hasWorkspaces = workspaces && workspaces.length > 0;
  const groups = hasWorkspaces ? groupThreadsByWorkspace(threads, workspaces) : null;

  const selectedIndex = Math.max(
    0,
    threads.findIndex((thread) => thread.id === selectedThreadId)
  );

  const handleListScroll = (event: UIEvent<HTMLDivElement>) => {
    if (!onScrollPositionChange) {
      return;
    }
    onScrollPositionChange(event.currentTarget.scrollTop);
  };

  const handleToggleFolder = (folderId: string) => {
    if (onToggleFolder) {
      onToggleFolder(folderId);
    } else {
      setLocalCollapsed((prev) =>
        prev.includes(folderId) ? prev.filter((id) => id !== folderId) : [...prev, folderId]
      );
    }
  };

  return (
    <section className={`threads-pane ${className ?? ""}`} id="threads-panel">
      <div className="pane-title-row">
        <h2 className="pane-title">Threads</h2>
        <div className="inline-chip-row" role="tablist" aria-label="Thread filters">
          {FILTER_OPTIONS.map((option) => (
            <button
              key={option.value}
              role="tab"
              aria-selected={threadFilter === option.value}
              className={`chip-button ${threadFilter === option.value ? "is-selected" : ""}`}
              onClick={() => onThreadFilterChange(option.value)}
            >
              {option.label}
            </button>
          ))}
        </div>
      </div>

      <label className="search-field">
        <Search size={14} />
        <input
          value={threadSearch}
          onChange={(event) => onThreadSearchChange(event.target.value)}
          placeholder="Search threads"
          aria-label="Search threads"
        />
      </label>

      <div
        className="threads-list"
        ref={listRef}
        onScroll={handleListScroll}
        onKeyDown={(event) => {
          if (threads.length === 0) {
            return;
          }
          if (event.key === "ArrowDown") {
            event.preventDefault();
            const next = Math.min(selectedIndex + 1, threads.length - 1);
            onThreadSelect(threads[next].id);
          }
          if (event.key === "ArrowUp") {
            event.preventDefault();
            const next = Math.max(selectedIndex - 1, 0);
            onThreadSelect(threads[next].id);
          }
          if (event.key === "Home") {
            event.preventDefault();
            onThreadSelect(threads[0].id);
          }
          if (event.key === "End") {
            event.preventDefault();
            onThreadSelect(threads[threads.length - 1].id);
          }
        }}
      >
        {threads.length === 0 ? (
          <div className="empty-state compact">No threads match this filter.</div>
        ) : groups ? (
          groups.map((group) => (
            <FolderGroup
              key={group.workspace.id}
              group={group}
              collapsed={collapsedFolders.includes(group.workspace.id)}
              selectedThreadId={selectedThreadId}
              onToggle={() => handleToggleFolder(group.workspace.id)}
              onThreadSelect={onThreadSelect}
              onThreadArchiveToggle={onThreadArchiveToggle}
            />
          ))
        ) : (
          threads.map((thread) => (
            <ThreadCard
              key={thread.id}
              thread={thread}
              selected={thread.id === selectedThreadId}
              onSelect={() => onThreadSelect(thread.id)}
              onArchiveToggle={() => onThreadArchiveToggle(thread)}
            />
          ))
        )}
      </div>
    </section>
  );
}
