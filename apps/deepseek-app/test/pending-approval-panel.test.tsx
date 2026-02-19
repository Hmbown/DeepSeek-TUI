import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PendingApprovalPanel } from "@/components/chat/PendingApprovalPanel";

const approvals = [
  {
    id: "approval_1",
    event: "approval.required" as const,
    status: "pending" as const,
    requestType: "Shell command",
    scope: "workspace write",
    consequence: "Runtime paused",
    createdAt: "2026-01-01T00:00:00Z",
    rawSnippet: "{}",
  },
];

describe("PendingApprovalPanel", () => {
  it("renders unsupported runtime message when approve/deny API is unavailable", () => {
    render(
      <PendingApprovalPanel
        approvals={approvals}
        approvalCapability={{
          supportsApprove: false,
          supportsDeny: false,
          supported: false,
          source: "none",
        }}
        onApprove={vi.fn()}
        onDeny={vi.fn()}
        onDismiss={vi.fn()}
        onDismissAll={vi.fn()}
      />
    );

    expect(
      screen.getByText("runtime does not expose approve/deny API yet")
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /approve shell command/i })).toBeNull();
  });

  it("shows approve and deny actions when capability is supported", () => {
    const onApprove = vi.fn();
    const onDeny = vi.fn();

    render(
      <PendingApprovalPanel
        approvals={approvals}
        approvalCapability={{
          supportsApprove: true,
          supportsDeny: true,
          supported: true,
          source: "health",
        }}
        onApprove={onApprove}
        onDeny={onDeny}
        onDismiss={vi.fn()}
        onDismissAll={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /approve shell command/i }));
    fireEvent.click(screen.getByRole("button", { name: /deny shell command/i }));
    expect(onApprove).toHaveBeenCalledWith("approval_1");
    expect(onDeny).toHaveBeenCalledWith("approval_1");
  });
});
