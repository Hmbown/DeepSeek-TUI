import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { TopBar } from "@/components/topbar/TopBar";
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

describe("Git integration - TopBar", () => {
  it("shows branch name when git repo exists", () => {
    render(<TopBar workspace={baseWorkspace} />);

    expect(screen.getByText("main")).toBeInTheDocument();
  });

  it("hides git info when no git repo", () => {
    const noGitWorkspace = { ...baseWorkspace, git_repo: false };
    render(<TopBar workspace={noGitWorkspace} />);

    expect(screen.queryByText("main")).toBeNull();
  });

  it("shows staged count in git status", () => {
    const stagedWorkspace = { ...baseWorkspace, staged: 5 };
    render(<TopBar workspace={stagedWorkspace} />);

    expect(screen.getByText("+5")).toBeInTheDocument();
  });

  it("shows commit button only when staged > 0 and onCommit provided", () => {
    const stagedWorkspace = { ...baseWorkspace, staged: 2 };
    render(<TopBar workspace={stagedWorkspace} onCommit={vi.fn()} />);

    expect(screen.getByText("Commit")).toBeInTheDocument();
  });

  it("hides commit button when staged is 0", () => {
    render(<TopBar workspace={baseWorkspace} onCommit={vi.fn()} />);

    expect(screen.queryByText("Commit")).toBeNull();
  });
});
