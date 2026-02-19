import { useCallback, useMemo, useState, type RefObject, type UIEvent } from "react";
import clsx from "clsx";
import { Check, ChevronDown, ChevronRight, Copy, Search } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type { TranscriptCell, TranscriptCellRole } from "@/lib/thread-utils";

const COLLAPSIBLE_ROLES: Set<TranscriptCellRole> = new Set([
  "tool",
  "command",
  "file",
  "compaction",
  "status",
]);

const COLLAPSE_LINE_THRESHOLD = 6;

const FILTER_OPTIONS: { value: TranscriptCellRole | "all"; label: string }[] = [
  { value: "all", label: "All" },
  { value: "user", label: "User" },
  { value: "assistant", label: "Assistant" },
  { value: "tool", label: "Tool" },
  { value: "command", label: "Command" },
  { value: "file", label: "File" },
  { value: "status", label: "Status" },
  { value: "error", label: "Error" },
  { value: "compaction", label: "Compaction" },
];

type TranscriptPaneProps = {
  transcript: TranscriptCell[];
  selectedThreadId: string | null;
  scrollRef?: RefObject<HTMLDivElement | null>;
  onScrollPositionChange?: (scrollTop: number) => void;
};

function formatTime(value?: string | null): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString();
}

function isLongContent(content: string): boolean {
  return content.split("\n").length > COLLAPSE_LINE_THRESHOLD;
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Fallback for environments without clipboard API
    }
  }, [text]);

  return (
    <button
      className="btn btn-ghost btn-sm transcript-copy-btn"
      onClick={handleCopy}
      aria-label="Copy to clipboard"
      title="Copy to clipboard"
    >
      {copied ? <Check size={12} /> : <Copy size={12} />}
    </button>
  );
}

function TranscriptCard({ cell }: { cell: TranscriptCell }) {
  const canCollapse = COLLAPSIBLE_ROLES.has(cell.role) && isLongContent(cell.content);
  const [expanded, setExpanded] = useState(!canCollapse);

  const displayContent = expanded
    ? cell.content
    : cell.content.split("\n").slice(0, COLLAPSE_LINE_THRESHOLD).join("\n") + "\n...";

  return (
    <article className={clsx("transcript-card", `role-${cell.role}`)}>
      <header className="transcript-card-head">
        <div className="transcript-card-title">
          {canCollapse ? (
            <button
              className="collapse-toggle"
              onClick={() => setExpanded((v) => !v)}
              aria-label={expanded ? "Collapse" : "Expand"}
              aria-expanded={expanded}
            >
              {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              {cell.title}
            </button>
          ) : (
            cell.title
          )}
        </div>
        <div className="transcript-card-meta">
          <CopyButton text={cell.content} />
          {cell.status ? <span className="status-chip">{cell.status.replaceAll("_", " ")}</span> : null}
          {cell.startedAt ? <span>{formatTime(cell.startedAt)}</span> : null}
        </div>
      </header>
      <div className={clsx("markdown-body", { "is-collapsed": !expanded && canCollapse })}>
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={{
            code(props) {
              const { children, className } = props;
              const isInline = !className;
              return isInline ? (
                <code className="inline-code">{children}</code>
              ) : (
                <pre className="code-block">
                  <code>{children}</code>
                </pre>
              );
            },
          }}
        >
          {displayContent}
        </ReactMarkdown>
      </div>
    </article>
  );
}

export function TranscriptPane({
  transcript,
  selectedThreadId,
  scrollRef,
  onScrollPositionChange,
}: TranscriptPaneProps) {
  const [activeFilter, setActiveFilter] = useState<TranscriptCellRole | "all">("all");
  const [searchQuery, setSearchQuery] = useState("");

  const filtered = useMemo(() => {
    let result = activeFilter === "all"
      ? transcript
      : transcript.filter((cell) => cell.role === activeFilter);
    const q = searchQuery.trim().toLowerCase();
    if (q) {
      result = result.filter((cell) => cell.content.toLowerCase().includes(q));
    }
    return result;
  }, [transcript, activeFilter, searchQuery]);

  const handleScroll = (event: UIEvent<HTMLDivElement>) => {
    if (!onScrollPositionChange) {
      return;
    }
    onScrollPositionChange(event.currentTarget.scrollTop);
  };

  if (!selectedThreadId) {
    return <div className="empty-state">Create or select a thread to begin.</div>;
  }

  return (
    <div
      className="transcript-pane"
      role="log"
      aria-live="polite"
      aria-label="Thread transcript"
      ref={scrollRef}
      onScroll={handleScroll}
    >
      <div className="transcript-search-bar">
        <Search size={14} className="transcript-search-icon" aria-hidden="true" />
        <input
          className="transcript-search-input"
          type="search"
          placeholder="Search transcript..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          aria-label="Search transcript"
        />
      </div>

      <div className="transcript-filter-bar" role="toolbar" aria-label="Filter transcript items">
        {FILTER_OPTIONS.map((opt) => (
          <button
            key={opt.value}
            className={clsx("chip-button", { "is-selected": activeFilter === opt.value })}
            onClick={() => setActiveFilter(opt.value)}
            aria-pressed={activeFilter === opt.value}
          >
            {opt.label}
          </button>
        ))}
      </div>

      {filtered.length === 0 ? (
        <div className="empty-state compact">
          {transcript.length === 0 ? "No messages yet." : "No items match the current filter or search."}
        </div>
      ) : (
        filtered.map((cell) => <TranscriptCard key={cell.id} cell={cell} />)
      )}
    </div>
  );
}
