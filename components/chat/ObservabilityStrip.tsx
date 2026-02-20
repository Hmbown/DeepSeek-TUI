function formatCompleted(value?: string | null): string {
  if (!value) {
    return "none";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleTimeString();
}

type ObservabilityStripProps = {
  runningTaskCount: number;
  queuedTaskCount: number;
  lastCompletedAt?: string | null;
  reconnectAttempt?: number;
  reconnectDelayMs?: number | null;
  isReconnecting: boolean;
};

export function ObservabilityStrip({
  runningTaskCount,
  queuedTaskCount,
  lastCompletedAt,
  reconnectAttempt,
  reconnectDelayMs,
  isReconnecting,
}: ObservabilityStripProps) {
  return (
    <div className="observability-strip" role="status" aria-live="polite" aria-label="Task and run observability">
      <span className="status-chip status-running">{`running ${runningTaskCount}`}</span>
      <span className="status-chip status-queued">{`queued ${queuedTaskCount}`}</span>
      <span className="status-chip status-completed">{`last completed ${formatCompleted(lastCompletedAt)}`}</span>
      {isReconnecting ? (
        <span className="status-chip status-reconnecting">
          {`reconnect attempt ${reconnectAttempt ?? 1}${reconnectDelayMs ? ` in ${(reconnectDelayMs / 1000).toFixed(1)}s` : ""}`}
        </span>
      ) : null}
    </div>
  );
}
