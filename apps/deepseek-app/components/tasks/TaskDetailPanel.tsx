import {
  ArrowRight,
  CheckCircle2,
  Clock,
  Terminal,
  X,
  XCircle,
} from "lucide-react";
import clsx from "clsx";

import type { TaskRecord } from "@/lib/runtime-api";

type TaskDetailPanelProps = {
  task: TaskRecord | null;
  loading?: boolean;
  onClose: () => void;
  onOpenThread?: (threadId: string) => void;
};

function formatTimestamp(value?: string | null): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function durationLabel(ms?: number | null): string {
  if (ms == null) return "-";
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60_000).toFixed(1)}m`;
}

function statusIcon(status: string) {
  switch (status) {
    case "completed":
    case "success":
      return <CheckCircle2 size={14} />;
    case "failed":
    case "error":
    case "canceled":
      return <XCircle size={14} />;
    case "running":
    case "queued":
      return <Clock size={14} />;
    default:
      return <Terminal size={14} />;
  }
}

export function TaskDetailPanel({ task, loading, onClose, onOpenThread }: TaskDetailPanelProps) {
  if (!task) {
    return (
      <section className="task-detail-panel" aria-label="Task detail">
        <div className="task-detail-head">
          <h3>Task detail</h3>
          <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label="Close">
            <X size={14} />
          </button>
        </div>
        <div className="empty-state compact">
          {loading ? "Loading task..." : "Select a task to view details."}
        </div>
      </section>
    );
  }

  return (
    <section className="task-detail-panel" aria-label={`Task ${task.id}`}>
      <div className="task-detail-head">
        <h3>
          <span className={`status-chip status-${task.status}`}>
            {statusIcon(task.status)} {task.status}
          </span>
        </h3>
        <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label="Close task detail">
          <X size={14} />
        </button>
      </div>

      <div className="task-detail-meta meta-grid">
        <div><span className="meta-label">ID:</span> <code>{task.id}</code></div>
        <div><span className="meta-label">Model:</span> {task.model}</div>
        <div><span className="meta-label">Mode:</span> {task.mode}</div>
        <div><span className="meta-label">Created:</span> {formatTimestamp(task.created_at)}</div>
        <div><span className="meta-label">Started:</span> {formatTimestamp(task.started_at)}</div>
        <div><span className="meta-label">Ended:</span> {formatTimestamp(task.ended_at)}</div>
        <div><span className="meta-label">Duration:</span> {durationLabel(task.duration_ms)}</div>
      </div>

      <div className="task-detail-prompt">
        <span className="field-label">Prompt</span>
        <div className="task-prompt-text">{task.prompt}</div>
      </div>

      {task.result_summary ? (
        <div className="task-detail-result">
          <span className="field-label">Result</span>
          <div className="task-result-text">{task.result_summary}</div>
        </div>
      ) : null}

      {task.error ? (
        <div className="task-detail-error inline-error">{task.error}</div>
      ) : null}

      {(task.thread_id || task.turn_id) ? (
        <div className="task-detail-links">
          {task.thread_id ? (
            <button
              className="btn btn-ghost btn-sm"
              onClick={() => onOpenThread?.(task.thread_id!)}
            >
              <ArrowRight size={14} />
              Open thread {task.thread_id}
            </button>
          ) : null}
        </div>
      ) : null}

      {task.timeline.length > 0 ? (
        <div className="task-detail-timeline">
          <span className="field-label">Timeline ({task.timeline.length})</span>
          <div className="timeline-list">
            {task.timeline.map((entry, idx) => (
              <div key={idx} className="timeline-entry">
                <span className="timeline-kind">{entry.kind}</span>
                <span className="timeline-summary">{entry.summary}</span>
                <span className="timeline-time">{formatTimestamp(entry.timestamp)}</span>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {task.tool_calls.length > 0 ? (
        <div className="task-detail-tools">
          <span className="field-label">Tool calls ({task.tool_calls.length})</span>
          <div className="tool-call-list">
            {task.tool_calls.map((call) => (
              <div key={call.id} className={clsx("tool-call-entry", `status-bg-${call.status}`)}>
                <div className="tool-call-head">
                  {statusIcon(call.status)}
                  <strong>{call.name}</strong>
                  <span className="subtle">{durationLabel(call.duration_ms)}</span>
                </div>
                {call.input_summary ? (
                  <div className="tool-call-io subtle">{call.input_summary}</div>
                ) : null}
                {call.output_summary ? (
                  <div className="tool-call-io subtle">{call.output_summary}</div>
                ) : null}
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </section>
  );
}
