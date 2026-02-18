import { Activity, CirclePause, Compass, ForkKnife, Hand } from "lucide-react";

type LiveEventsPanelProps = {
  events: string[];
  canResume: boolean;
  canFork: boolean;
  canInterrupt: boolean;
  canCompact: boolean;
  steerText: string;
  onSteerTextChange: (value: string) => void;
  onResume: () => void;
  onFork: () => void;
  onInterrupt: () => void;
  onCompact: () => void;
  onSteer: () => void;
};

export function LiveEventsPanel({
  events,
  canResume,
  canFork,
  canInterrupt,
  canCompact,
  steerText,
  onSteerTextChange,
  onResume,
  onFork,
  onInterrupt,
  onCompact,
  onSteer,
}: LiveEventsPanelProps) {
  return (
    <section className="live-events">
      <header className="live-events-head">
        <div>
          <h3>Live events</h3>
          <p>Streamed thread lifecycle and tool activity.</p>
        </div>
        <div className="inline-actions">
          <button className="btn btn-ghost btn-sm" disabled={!canResume} onClick={onResume}>
            <CirclePause size={14} />
            <span>Resume</span>
          </button>
          <button className="btn btn-ghost btn-sm" disabled={!canFork} onClick={onFork}>
            <ForkKnife size={14} />
            <span>Fork</span>
          </button>
          <button className="btn btn-ghost btn-sm" disabled={!canCompact} onClick={onCompact}>
            <Compass size={14} />
            <span>Compact</span>
          </button>
          <button className="btn btn-danger btn-sm" disabled={!canInterrupt} onClick={onInterrupt}>
            <Hand size={14} />
            <span>Interrupt</span>
          </button>
        </div>
      </header>

      <div className="event-list" aria-live="polite">
        {events.length === 0 ? (
          <div className="empty-state compact">Waiting for events…</div>
        ) : (
          events.map((event, index) => (
            <div key={`${event}-${index}`} className="event-row">
              <Activity size={13} />
              <span>{event}</span>
            </div>
          ))
        )}
      </div>

      <div className="steer-row">
        <input
          value={steerText}
          onChange={(event) => onSteerTextChange(event.target.value)}
          placeholder="Steer active turn…"
        />
        <button className="btn btn-secondary" disabled={!steerText.trim() || !canInterrupt} onClick={onSteer}>
          Send steer
        </button>
      </div>
    </section>
  );
}
