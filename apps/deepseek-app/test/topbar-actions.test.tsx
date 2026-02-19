import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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

describe("TopBar editable title", () => {
  it("shows title text by default", () => {
    render(
      <TopBar workspace={baseWorkspace} threadTitle="My Thread" onTitleChange={vi.fn()} />
    );

    expect(screen.getByText("My Thread")).toBeInTheDocument();
  });

  it("enters edit mode on click and saves on Enter", () => {
    const onTitleChange = vi.fn();
    render(
      <TopBar workspace={baseWorkspace} threadTitle="Old Title" onTitleChange={onTitleChange} />
    );

    fireEvent.click(screen.getByText("Old Title"));

    const input = screen.getByDisplayValue("Old Title");
    fireEvent.change(input, { target: { value: "New Title" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onTitleChange).toHaveBeenCalledWith("New Title");
  });

  it("cancels editing on Escape", () => {
    const onTitleChange = vi.fn();
    render(
      <TopBar workspace={baseWorkspace} threadTitle="My Title" onTitleChange={onTitleChange} />
    );

    fireEvent.click(screen.getByText("My Title"));

    const input = screen.getByDisplayValue("My Title");
    fireEvent.change(input, { target: { value: "Changed" } });
    fireEvent.keyDown(input, { key: "Escape" });

    expect(onTitleChange).not.toHaveBeenCalled();
    expect(screen.getByText("My Title")).toBeInTheDocument();
  });
});

describe("TopBar overflow menu", () => {
  it("triggers onFork callback from overflow menu", () => {
    const onFork = vi.fn();
    render(
      <TopBar workspace={baseWorkspace} onFork={onFork} />
    );

    const moreBtn = screen.getByLabelText("More actions");
    fireEvent.click(moreBtn);

    const forkItem = screen.getByText("Fork thread");
    fireEvent.click(forkItem);

    expect(onFork).toHaveBeenCalled();
  });

  it("triggers onDelete callback from overflow menu", () => {
    const onDelete = vi.fn();
    render(
      <TopBar workspace={baseWorkspace} onDelete={onDelete} />
    );

    const moreBtn = screen.getByLabelText("More actions");
    fireEvent.click(moreBtn);

    const deleteItem = screen.getByText("Delete thread");
    fireEvent.click(deleteItem);

    expect(onDelete).toHaveBeenCalled();
  });
});

describe("TopBar commit button", () => {
  it("shows commit button when staged changes exist", () => {
    const stagedWorkspace = { ...baseWorkspace, staged: 3 };
    render(
      <TopBar workspace={stagedWorkspace} onCommit={vi.fn()} />
    );

    expect(screen.getByLabelText("Commit staged changes")).toBeInTheDocument();
  });

  it("hides commit button when no staged changes", () => {
    render(
      <TopBar workspace={baseWorkspace} onCommit={vi.fn()} />
    );

    expect(screen.queryByLabelText("Commit staged changes")).toBeNull();
  });
});
