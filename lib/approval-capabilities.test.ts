import { describe, expect, it } from "vitest";

import { deriveApprovalCapability } from "@/lib/approval-capabilities";
import type { PendingApproval } from "@/lib/runtime-api";

function approvalWithHints(
  hints: PendingApproval["actionHints"]
): PendingApproval {
  return {
    id: "approval-1",
    event: "approval.required",
    status: "pending",
    requestType: "Shell command",
    scope: "workspace",
    consequence: "Pending",
    createdAt: "2026-01-01T00:00:00Z",
    rawSnippet: "{}",
    actionHints: hints,
  };
}

describe("deriveApprovalCapability", () => {
  it("returns none when neither health nor payload exposes hints", () => {
    const result = deriveApprovalCapability({
      health: null,
      approvals: [],
    });

    expect(result.supported).toBe(false);
    expect(result.source).toBe("none");
  });

  it("detects support from health capabilities", () => {
    const result = deriveApprovalCapability({
      health: {
        status: "ok",
        capabilities: {
          approvals: {
            approve: true,
            deny: true,
          },
        },
      },
      approvals: [],
    });

    expect(result.supported).toBe(true);
    expect(result.source).toBe("health");
  });

  it("detects support from approval payload hints", () => {
    const result = deriveApprovalCapability({
      health: null,
      approvals: [
        approvalWithHints({
          approve: { available: true },
          deny: { available: true },
        }),
      ],
    });

    expect(result.supported).toBe(true);
    expect(result.source).toBe("payload");
  });

  it("prioritizes explicit health booleans over payload hints", () => {
    const result = deriveApprovalCapability({
      health: {
        status: "ok",
        capabilities: {
          approvals: {
            approve: false,
            deny: true,
          },
        },
      },
      approvals: [
        approvalWithHints({
          approve: { available: true },
          deny: { available: true },
        }),
      ],
    });

    expect(result.supportsApprove).toBe(false);
    expect(result.supportsDeny).toBe(true);
    expect(result.supported).toBe(false);
    expect(result.source).toBe("health");
  });
});
