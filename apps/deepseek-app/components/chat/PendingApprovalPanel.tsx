import { AlertTriangle, CheckCircle2, Clock3, ShieldAlert, X } from "lucide-react";

import type { PendingApproval } from "@/lib/runtime-api";

type PendingApprovalPanelProps = {
  approvals: PendingApproval[];
  onDismiss: (id: string) => void;
  onDismissAll: () => void;
};

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString();
}

export function PendingApprovalPanel({
  approvals,
  onDismiss,
  onDismissAll,
}: PendingApprovalPanelProps) {
  if (approvals.length === 0) {
    return null;
  }

  return (
    <section className="approval-panel" aria-live="polite">
      <header className="approval-panel-head">
        <div>
          <h3>Pending approvals</h3>
          <p>Approval must be completed in runtime session.</p>
        </div>
        <button className="btn btn-ghost btn-sm" onClick={onDismissAll}>
          <CheckCircle2 size={14} />
          Acknowledge all
        </button>
      </header>

      <div className="approval-list">
        {approvals.map((approval) => (
          <article key={approval.id} className={`approval-row is-${approval.status}`}>
            <div className="approval-row-main">
              <div className="thread-header">
                <strong>{approval.requestType}</strong>
                <span className={`status-chip status-${approval.status}`}>
                  {approval.status === "pending" ? "pending approval" : "denied"}
                </span>
              </div>
              <div className="approval-meta">
                <span>
                  <ShieldAlert size={12} />
                  {approval.scope}
                </span>
                <span>
                  <Clock3 size={12} />
                  {formatTimestamp(approval.createdAt)}
                </span>
              </div>
              <p className="approval-consequence">
                <AlertTriangle size={13} />
                <span>{approval.consequence}</span>
              </p>
              <details className="approval-raw">
                <summary>Raw payload</summary>
                <code>{approval.rawSnippet}</code>
              </details>
            </div>
            <button className="btn btn-ghost btn-sm" onClick={() => onDismiss(approval.id)} aria-label="Dismiss approval notice">
              <X size={14} />
              Dismiss
            </button>
          </article>
        ))}
      </div>
    </section>
  );
}
