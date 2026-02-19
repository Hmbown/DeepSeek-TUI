import type { HealthResponse, PendingApproval } from "@/lib/runtime-api";

export type ApprovalCapabilitySource = "health" | "payload" | "none";

export type ApprovalCapability = {
  supportsApprove: boolean;
  supportsDeny: boolean;
  supported: boolean;
  source: ApprovalCapabilitySource;
};

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function pickPath(root: unknown, path: string[]): unknown {
  let cursor: unknown = root;
  for (const key of path) {
    const record = asRecord(cursor);
    if (!record) {
      return undefined;
    }
    cursor = record[key];
  }
  return cursor;
}

function pickBoolean(root: unknown, candidates: string[][]): boolean | undefined {
  for (const path of candidates) {
    const value = pickPath(root, path);
    if (typeof value === "boolean") {
      return value;
    }
  }
  return undefined;
}

function pickFromActionList(root: unknown, candidates: string[][]): { approve?: boolean; deny?: boolean } {
  for (const path of candidates) {
    const value = pickPath(root, path);
    if (!Array.isArray(value)) {
      continue;
    }
    let approve: boolean | undefined;
    let deny: boolean | undefined;
    for (const entry of value) {
      if (typeof entry !== "string") {
        continue;
      }
      const normalized = entry.trim().toLowerCase();
      if (normalized === "approve" || normalized === "allow") {
        approve = true;
      }
      if (normalized === "deny" || normalized === "reject" || normalized === "block") {
        deny = true;
      }
    }
    if (approve !== undefined || deny !== undefined) {
      return { approve, deny };
    }
  }
  return {};
}

function deriveFromHealth(health: HealthResponse | null): { approve?: boolean; deny?: boolean } {
  if (!health) {
    return {};
  }

  const approve = pickBoolean(health, [
    ["capabilities", "approvals", "approve"],
    ["capabilities", "approval", "approve"],
    ["features", "approvals", "approve"],
    ["features", "approval", "approve"],
    ["approvals", "approve"],
    ["approval", "approve"],
  ]);
  const deny = pickBoolean(health, [
    ["capabilities", "approvals", "deny"],
    ["capabilities", "approval", "deny"],
    ["features", "approvals", "deny"],
    ["features", "approval", "deny"],
    ["approvals", "deny"],
    ["approval", "deny"],
  ]);

  const fromActions = pickFromActionList(health, [
    ["capabilities", "approvals", "actions"],
    ["features", "approvals", "actions"],
    ["approvals", "actions"],
    ["approval", "actions"],
  ]);

  return {
    approve: approve ?? fromActions.approve,
    deny: deny ?? fromActions.deny,
  };
}

function deriveFromApprovals(approvals: PendingApproval[]): { approve?: boolean; deny?: boolean } {
  let approve: boolean | undefined;
  let deny: boolean | undefined;

  for (const approval of approvals) {
    if (approval.actionHints?.approve?.available) {
      approve = true;
    }
    if (approval.actionHints?.deny?.available) {
      deny = true;
    }
  }

  return { approve, deny };
}

export function deriveApprovalCapability(input: {
  health: HealthResponse | null;
  approvals: PendingApproval[];
}): ApprovalCapability {
  const fromHealth = deriveFromHealth(input.health);
  const fromApprovals = deriveFromApprovals(input.approvals);

  const supportsApprove = fromHealth.approve ?? fromApprovals.approve ?? false;
  const supportsDeny = fromHealth.deny ?? fromApprovals.deny ?? false;

  let source: ApprovalCapabilitySource = "none";
  if (fromHealth.approve !== undefined || fromHealth.deny !== undefined) {
    source = "health";
  } else if (fromApprovals.approve !== undefined || fromApprovals.deny !== undefined) {
    source = "payload";
  }

  return {
    supportsApprove,
    supportsDeny,
    supported: supportsApprove && supportsDeny,
    source,
  };
}
