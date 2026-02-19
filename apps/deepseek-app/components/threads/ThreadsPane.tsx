import { Archive, ArchiveRestore, Search } from "lucide-react";

import type { ThreadFilter } from "@/components/types";
import type { ThreadSummary } from "@/lib/runtime-api";

type ThreadsPaneProps = {
  threads: ThreadSummary[];
  selectedThreadId: string | null;
  threadSearch: string;
  threadFilter: ThreadFilter;
  className?: string;
  onThreadSearchChange: (value: string) => void;
  onThreadFilterChange: (value: ThreadFilter) => void;
  onThreadSelect: (id: string) => void;
  onThreadArchiveToggle: (thread: ThreadSummary) => void;
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

export function ThreadsPane({
  threads,
  selectedThreadId,
  threadSearch,
  threadFilter,
  className,
  onThreadSearchChange,
  onThreadFilterChange,
  onThreadSelect,
  onThreadArchiveToggle,
}: ThreadsPaneProps) {
  const selectedIndex = Math.max(
    0,
    threads.findIndex((thread) => thread.id === selectedThreadId)
  );

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
        ) : (
          threads.map((thread) => {
            const selected = thread.id === selectedThreadId;
            return (
              <article key={thread.id} className={`thread-card ${selected ? "is-selected" : ""}`}>
                <button className="thread-main" onClick={() => onThreadSelect(thread.id)}>
                  <div className="thread-header">
                    <strong className="thread-title">{thread.title}</strong>
                    <span className={`status-chip status-${thread.latest_turn_status ?? "idle"}`}>
                      {statusLabel(thread.latest_turn_status)}
                    </span>
                  </div>
                  <div className="thread-preview">{thread.preview}</div>
                  <div className="thread-meta">
                    <span>{thread.model}</span>
                    <span>{new Date(thread.updated_at).toLocaleString()}</span>
                  </div>
                </button>
                <div className="thread-actions">
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={() => onThreadArchiveToggle(thread)}
                    aria-label={thread.archived ? "Unarchive thread" : "Archive thread"}
                  >
                    {thread.archived ? <ArchiveRestore size={14} /> : <Archive size={14} />}
                    <span>{thread.archived ? "Unarchive" : "Archive"}</span>
                  </button>
                </div>
              </article>
            );
          })
        )}
      </div>
    </section>
  );
}
