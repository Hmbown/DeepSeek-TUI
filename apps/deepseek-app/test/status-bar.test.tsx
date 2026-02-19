import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { StatusBar } from "@/components/chat/StatusBar";
import type { WorkspaceStatus } from "@/lib/runtime-api";

afterEach(cleanup);

const baseWorkspace: WorkspaceStatus = {
  workspace: "/home/user/project",
  git_repo: true,
  branch: "main",
  staged: 0,
  unstaged: 0,
  untracked: 0,
};

describe("StatusBar", () => {
  it("shows 'Full access' for yolo mode", () => {
    render(<StatusBar mode="yolo" workspace={baseWorkspace} onInitGit={vi.fn()} />);
    expect(screen.getByText("Full access")).toBeInTheDocument();
  });

  it("shows 'Agent mode' for agent mode", () => {
    render(<StatusBar mode="agent" workspace={baseWorkspace} onInitGit={vi.fn()} />);
    expect(screen.getByText("Agent mode")).toBeInTheDocument();
  });

  it("shows 'Plan mode' for plan mode", () => {
    render(<StatusBar mode="plan" workspace={baseWorkspace} onInitGit={vi.fn()} />);
    expect(screen.getByText("Plan mode")).toBeInTheDocument();
  });

  it("shows 'Approval required' for normal mode", () => {
    render(<StatusBar mode="normal" workspace={baseWorkspace} onInitGit={vi.fn()} />);
    expect(screen.getByText("Approval required")).toBeInTheDocument();
  });

  it("shows git init button when no git repo", () => {
    const noGitWorkspace = { ...baseWorkspace, git_repo: false };
    render(<StatusBar mode="agent" workspace={noGitWorkspace} onInitGit={vi.fn()} />);
    expect(screen.getByText("Create git repository")).toBeInTheDocument();
  });

  it("calls onInitGit when git init button is clicked", () => {
    const noGitWorkspace = { ...baseWorkspace, git_repo: false };
    const onInitGit = vi.fn();
    render(<StatusBar mode="agent" workspace={noGitWorkspace} onInitGit={onInitGit} />);

    fireEvent.click(screen.getByRole("button", { name: /create git repository/i }));
    expect(onInitGit).toHaveBeenCalledTimes(1);
  });

  it("hides git init button when git repo exists", () => {
    render(<StatusBar mode="agent" workspace={baseWorkspace} onInitGit={vi.fn()} />);
    expect(screen.queryByText("Create git repository")).toBeNull();
  });
});
