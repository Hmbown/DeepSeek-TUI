import type { RefObject, UIEvent } from "react";
import { Activity, CirclePause, Compass, ForkKnife, Hand, Pin } from "lucide-react";

import type { DesktopRunStateDetail } from "@/lib/run-state";
import type { CompactLiveEvent } from "@/lib/live-event-compaction";
import type { LiveEventFilter } from "@/lib/live-event-filters";

type LiveEventsPanelProps = {
  events: CompactLiveEvent[];
  pinnedCritical: CompactLiveEvent[];
  overflowCount: number;
  showAllEvents: boolean;
  eventFilter: LiveEventFilter;
  runState: DesktopRunStateDetail;
  canResume: boolean;
  canFork: boolean;
  canInterrupt: boolean;
  canCompact: boolean;
  eventListRef?: RefObject<HTMLDivElement | null>;
  onEventListScroll?: (scrollTop: number) => void;
  steerText: string;
  onSteerTextChange: (value: string) => void;
  onResume: () => void;
  onFork: () => void;
  onInterrupt: () => void;
  onCompact: () => void;
  onSteer: () => void;
  onEventFilterChange: (value: LiveEventFilter) => void;
  onToggleEventOverflow: () => void;
};

export function LiveEventsPanel({
  events,
  pinnedCritical,
  overflowCount,
  showAllEvents,
  eventFilter,
  runState,
  canResume,
  canFork,
  canInterrupt,
  canCompact,
  eventListRef,
  onEventListScroll,
  steerText,
  onSteerTextChange,
  onResume,
  onFork,
  onInterrupt,
  onCompact,
  onSteer,
  onEventFilterChange,
  onToggleEventOverflow,
}: LiveEventsPanelProps) {
  const handleEventListScroll = (event: UIEvent<HTMLDivElement>) => {
    if (!onEventListScroll) {
      return;
    }
    onEventListScroll(event.currentTarget.scrollTop);
  };

  return (
    <section className="live-events" id="live-events-panel" tabIndex={-1}>
      <header className="live-events-head">
        <div>
          <h3>Live events</h3>
          <p>Streamed thread lifecycle and tool activity.</p>
        </div>
        <div className="live-events-right">
          <span className={`status-chip status-${runState.state}`}>{runState.label}</span>
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
        </div>
      </header>

      <div className="inline-chip-row" role="tablist" aria-label="Event timeline filters">
        <button
          className={`chip-button ${eventFilter === "all" ? "is-selected" : ""}`}
          role="tab"
          aria-selected={eventFilter === "all"}
          aria-label="Show all live events"
          onClick={() => onEventFilterChange("all")}
        >
          All
        </button>
        <button
          className={`chip-button ${eventFilter === "critical" ? "is-selected" : ""}`}
          role="tab"
          aria-selected={eventFilter === "critical"}
          aria-label="Show critical events only"
          onClick={() => onEventFilterChange("critical")}
        >
          Critical
        </button>
        <button
          className={`chip-button ${eventFilter === "tool" ? "is-selected" : ""}`}
          role="tab"
          aria-selected={eventFilter === "tool"}
          aria-label="Show tool events only"
          onClick={() => onEventFilterChange("tool")}
        >
          Tool
        </button>
        <button
          className={`chip-button ${eventFilter === "turn" ? "is-selected" : ""}`}
          role="tab"
          aria-selected={eventFilter === "turn"}
          aria-label="Show turn lifecycle events only"
          onClick={() => onEventFilterChange("turn")}
        >
          Turn
        </button>
      </div>

      {pinnedCritical.length > 0 ? (
        <div className="critical-notices">
          {pinnedCritical.map((event) => (
            <div key={`pin-${event.id}`} className="critical-notice-row" role="note">
              <Pin size={12} />
              <span>{event.summary}</span>
            </div>
          ))}
        </div>
      ) : null}

      <div className="event-list" aria-live="polite" ref={eventListRef} onScroll={handleEventListScroll}>
        {events.length === 0 ? (
          <div className="empty-state compact">Waiting for events…</div>
        ) : (
          events.map((event) => (
            <div
              key={event.id}
              className={`event-row ${event.critical ? "is-critical" : ""}`}
              data-event={event.event}
            >
              <Activity size={13} />
              <span>{event.summary}</span>
              {event.count > 1 ? (
                <span className="status-chip status-checking" aria-label={`Compacted count ${event.count}`}>
                  x{event.count}
                </span>
              ) : null}
            </div>
          ))
        )}
      </div>

      {overflowCount > 0 ? (
        <div className="event-overflow-row">
          <span className="subtle">{overflowCount} more event groups hidden.</span>
          <button className="btn btn-ghost btn-sm" onClick={onToggleEventOverflow}>
            {showAllEvents ? "Show compact view" : "Show all events"}
          </button>
        </div>
      ) : null}

      <div className="steer-row">
        <input
          id="steer-input"
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
