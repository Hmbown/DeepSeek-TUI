import { ArrowUp, CornerDownLeft, Sparkles } from "lucide-react";

type ComposerProps = {
  value: string;
  onValueChange: (value: string) => void;
  onSend: () => void;
  onRetrySend: () => void;
  sending: boolean;
  selectedThreadId: string | null;
  activeTurnId: string | null;
  blockedSendReason?: string | null;
  canRetryBlockedSend?: boolean;
};

export function Composer({
  value,
  onValueChange,
  onSend,
  onRetrySend,
  sending,
  selectedThreadId,
  activeTurnId,
  blockedSendReason,
  canRetryBlockedSend,
}: ComposerProps) {
  return (
    <div className="composer">
      <label className="label" htmlFor="composer-input">
        Message
      </label>
      <textarea
        id="composer-input"
        rows={4}
        value={value}
        onChange={(event) => onValueChange(event.target.value)}
        placeholder="Type a prompt…"
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey) {
            event.preventDefault();
            if (!sending && value.trim() && !blockedSendReason) {
              onSend();
            }
          }

          if (event.key === "j" && (event.ctrlKey || event.metaKey)) {
            event.preventDefault();
            onValueChange(`${value}\n`);
          }

          if (event.key === "Enter" && event.altKey) {
            event.preventDefault();
            onValueChange(`${value}\n`);
          }
        }}
      />

      <div className="composer-footer">
        <div className="composer-hint-row">
          <span>
            <Sparkles size={13} />
            <span>
              {selectedThreadId ? `Thread ${selectedThreadId.slice(0, 8)}` : "New thread"}
              {activeTurnId ? ` · active ${activeTurnId.slice(0, 8)}` : ""}
            </span>
          </span>
          <span>
            <CornerDownLeft size={13} />
            <span>Enter send · Shift+Enter newline · Ctrl/Cmd+J newline</span>
          </span>
          {blockedSendReason ? (
            <span className="inline-error" role="alert">
              <span>{blockedSendReason}</span>
            </span>
          ) : null}
        </div>

        <div className="inline-actions composer-actions">
          {canRetryBlockedSend ? (
            <button className="btn btn-secondary" onClick={onRetrySend} aria-label="Retry send">
              Retry send
            </button>
          ) : null}
          <button
            className="btn btn-primary"
            disabled={sending || !value.trim() || !!blockedSendReason}
            onClick={onSend}
          >
            <ArrowUp size={14} />
            <span>{sending ? "Sending..." : "Send"}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
