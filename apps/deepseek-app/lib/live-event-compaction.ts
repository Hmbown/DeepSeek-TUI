import type { EventPayload } from "@/lib/runtime-api";

export type LiveEventRecord = EventPayload & {
  id: string;
  summary: string;
  critical: boolean;
};

export type CompactLiveEvent = {
  id: string;
  event: string;
  summary: string;
  count: number;
  critical: boolean;
  latestTimestamp?: string;
  seq?: number;
};

export type CompactLiveEventResult = {
  rows: CompactLiveEvent[];
  overflowCount: number;
  pinnedCritical: CompactLiveEvent[];
};

const CRITICAL_EVENTS: Set<string> = new Set([
  "approval.required",
  "sandbox.denied",
  "stream.disconnected",
  "stream.connected",
]);

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function payloadSummary(payload: unknown): string | null {
  const record = asRecord(payload);
  if (!record) {
    if (typeof payload === "string" && payload.trim()) {
      return payload.trim();
    }
    return null;
  }
  const direct = ["summary", "message", "reason", "status", "detail"];
  for (const key of direct) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }
  return null;
}

export function summarizeEvent(event: EventPayload): string {
  const eventName = event.event;
  if (eventName === "approval.required") {
    return payloadSummary(event.payload) ?? "Approval required";
  }
  if (eventName === "sandbox.denied") {
    return payloadSummary(event.payload) ?? "Sandbox denied request";
  }
  if (eventName === "stream.disconnected") {
    return payloadSummary(event.payload) ?? "Live stream disconnected";
  }
  if (eventName === "stream.connected") {
    return "Live stream connected";
  }

  const summary = payloadSummary(event.payload);
  if (summary) {
    return summary;
  }

  if (event.seq != null) {
    return `${eventName} #${event.seq}`;
  }
  return eventName;
}

export function toLiveEventRecord(event: EventPayload): LiveEventRecord {
  const seqPart = event.seq != null ? `seq-${event.seq}` : `ts-${event.timestamp ?? Date.now()}`;
  return {
    ...event,
    id: `${event.event}-${seqPart}`,
    summary: summarizeEvent(event),
    critical: CRITICAL_EVENTS.has(event.event),
  };
}

function compactRows(records: LiveEventRecord[]): CompactLiveEvent[] {
  const compacted: CompactLiveEvent[] = [];
  for (const record of records) {
    const previous = compacted[compacted.length - 1];
    if (
      previous &&
      !record.critical &&
      !previous.critical &&
      previous.event === record.event &&
      previous.summary === record.summary
    ) {
      previous.count += 1;
      if (record.timestamp) {
        previous.latestTimestamp = record.timestamp;
      }
      if (record.seq != null) {
        previous.seq = record.seq;
      }
      continue;
    }

    compacted.push({
      id: record.id,
      event: record.event,
      summary: record.summary,
      count: 1,
      critical: record.critical,
      latestTimestamp: record.timestamp,
      seq: record.seq,
    });
  }
  return compacted;
}

export function compactLiveEvents(
  events: EventPayload[],
  limit = 40,
  pinnedLimit = 4
): CompactLiveEventResult {
  const records = events.map(toLiveEventRecord);
  const rows = compactRows(records);

  const visible = rows.filter((row, index) => row.critical || index < limit);
  const overflowCount = Math.max(0, rows.length - visible.length);
  const pinnedCritical = rows.filter((row) => row.critical).slice(0, pinnedLimit);

  return {
    rows: visible,
    overflowCount,
    pinnedCritical,
  };
}

export function eventIsCritical(eventName: string): boolean {
  return CRITICAL_EVENTS.has(eventName);
}
