import type { ResponseItem } from "openai/resources/responses/responses.mjs";

import { parseToolCall } from "../../utils/parsers.js";
import { useMemo } from "react";

/**
 * Represents a grouped sequence of response items (e.g., function call batches).
 */
export type GroupedResponseItem = {
  label: string;
  items: Array<ResponseItem>;
};

/**
 * Custom hook to group recent response items for display batching.
 * Returns counts of auto-approved tool call groups, the latest batch,
 * and the count of user messages in the visible window.
 */
export function useMessageGrouping(visibleItems: Array<ResponseItem>): {
  groupCounts: Record<string, number>;
  batch: Array<{ item?: ResponseItem; group?: GroupedResponseItem }>;
  userMsgCount: number;
} {
  return useMemo(() => {
    // The grouping logic only depends on the subset of messages that are
    // currently rendered (visibleItems).  Using that as the sole dependency
    // keeps recomputations to a minimum and avoids unnecessary work when the
    // full list of `items` changes outside of the visible window.
    let userMsgCount = 0;
    const groupCounts: Record<string, number> = {};
    visibleItems.forEach((m) => {
      if (m.type === "function_call") {
        const toolCall = parseToolCall(m);
        if (toolCall?.autoApproval) {
          const group = toolCall.autoApproval.group;
          groupCounts[group] = (groupCounts[group] || 0) + 1;
        }
      }
      if (m.type === "message" && m.role === "user") {
        userMsgCount++;
      }
    });
    const lastFew = visibleItems.slice(-3);
    const batch: Array<{ item?: ResponseItem; group?: GroupedResponseItem }> =
      [];
    if (lastFew[0]?.type === "function_call") {
      const toolCall = parseToolCall(lastFew[0]);
      batch.push({
        group: {
          label: toolCall?.autoApproval?.group || "Running command",
          items: lastFew,
        },
      });
      if (lastFew[2]?.type === "message") {
        batch.push({ item: lastFew[2] });
      }
    } else if (lastFew[1]?.type === "function_call") {
      const toolCall = parseToolCall(lastFew[1]);
      batch.push({
        group: {
          label: toolCall?.autoApproval?.group || "Running command",
          items: lastFew.slice(1),
        },
      });
    } else if (lastFew[2]?.type === "function_call") {
      const toolCall = parseToolCall(lastFew[2]);
      batch.push({
        group: {
          label: toolCall?.autoApproval?.group || "Running command",
          items: [lastFew[2]],
        },
      });
    } else {
      lastFew.forEach((item) => batch.push({ item }));
    }
    return { groupCounts, batch, userMsgCount };
    // `items` is stable across renders while `visibleItems` changes based on
    // the scroll window. Including only `visibleItems` avoids unnecessary
    // recomputations while still producing correct results.
  }, [visibleItems]);
}
