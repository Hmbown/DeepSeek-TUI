import type { CompactLiveEvent } from "@/lib/live-event-compaction";
import { eventIsCritical } from "@/lib/live-event-compaction";
import type { EventPayload } from "@/lib/runtime-api";

export type LiveEventFilter = "all" | "critical" | "tool" | "turn";

export function isToolEvent(eventName: string): boolean {
  return eventName.startsWith("item.");
}

export function isTurnLifecycleEvent(eventName: string): boolean {
  return eventName.startsWith("turn.") || eventName.startsWith("thread.");
}

export function matchesLiveEventFilter(event: CompactLiveEvent, filter: LiveEventFilter): boolean {
  return matchesLiveEventFilterByParts(event.event, event.critical, filter);
}

export function matchesLiveEventPayloadFilter(event: EventPayload, filter: LiveEventFilter): boolean {
  return matchesLiveEventFilterByParts(event.event, eventIsCritical(event.event), filter);
}

function matchesLiveEventFilterByParts(eventName: string, critical: boolean, filter: LiveEventFilter): boolean {
  if (filter === "all") {
    return true;
  }
  if (filter === "critical") {
    return critical;
  }
  if (filter === "tool") {
    return isToolEvent(eventName);
  }
  return isTurnLifecycleEvent(eventName);
}

export function filterLiveEventRows(
  events: CompactLiveEvent[],
  filter: LiveEventFilter
): CompactLiveEvent[] {
  if (filter === "all") {
    return events;
  }
  return events.filter((event) => matchesLiveEventFilter(event, filter));
}

export function filterLiveEvents(
  events: EventPayload[],
  filter: LiveEventFilter
): EventPayload[] {
  if (filter === "all") {
    return events;
  }
  return events.filter((event) => matchesLiveEventPayloadFilter(event, filter));
}
