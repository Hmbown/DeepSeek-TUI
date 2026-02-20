import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Composer } from "@/components/chat/Composer";

afterEach(cleanup);

const baseProps = {
  value: "",
  onValueChange: vi.fn(),
  onSend: vi.fn(),
  onRetrySend: vi.fn(),
  sending: false as const,
  selectedThreadId: "thr_123",
  activeTurnId: null,
  mode: "agent",
  onModeChange: vi.fn(),
};

describe("Composer file attachments", () => {
  it("shows attachment chips when files are attached", () => {
    render(
      <Composer
        {...baseProps}
        attachedFiles={[
          { name: "foo.ts", path: "foo.ts" },
          { name: "bar.py", path: "bar.py" },
        ]}
        onAttachedFilesChange={vi.fn()}
      />
    );

    expect(screen.getByText("foo.ts")).toBeInTheDocument();
    expect(screen.getByText("bar.py")).toBeInTheDocument();
  });

  it("calls onAttachedFilesChange when removing a chip", () => {
    const onChange = vi.fn();
    render(
      <Composer
        {...baseProps}
        attachedFiles={[
          { name: "foo.ts", path: "foo.ts" },
          { name: "bar.py", path: "bar.py" },
        ]}
        onAttachedFilesChange={onChange}
      />
    );

    const removeBtn = screen.getByLabelText("Remove foo.ts");
    fireEvent.click(removeBtn);
    expect(onChange).toHaveBeenCalledWith([{ name: "bar.py", path: "bar.py" }]);
  });

  it("shows attach button when onAttachedFilesChange is provided", () => {
    render(
      <Composer
        {...baseProps}
        attachedFiles={[]}
        onAttachedFilesChange={vi.fn()}
      />
    );

    expect(screen.getByLabelText("Attach files")).toBeInTheDocument();
  });

  it("hides attach button when onAttachedFilesChange is not provided", () => {
    render(<Composer {...baseProps} />);

    expect(screen.queryByLabelText("Attach files")).toBeNull();
  });

  it("shows no chips when attachedFiles is empty", () => {
    render(
      <Composer
        {...baseProps}
        attachedFiles={[]}
        onAttachedFilesChange={vi.fn()}
      />
    );

    expect(screen.queryByLabelText(/Remove /)).toBeNull();
  });

  it("deduplicates by existing attachment names and keeps file path when available", () => {
    const onChange = vi.fn();
    const { container } = render(
      <Composer
        {...baseProps}
        attachedFiles={[{ name: "foo.ts", path: "foo.ts" }]}
        onAttachedFilesChange={onChange}
      />
    );

    const input = container.querySelector('input[type="file"]');
    expect(input).not.toBeNull();

    const duplicateName = new File(["const a = 1;"], "foo.ts", { type: "text/plain" });
    const withPath = new File(["print('hi')"], "bar.py", { type: "text/plain" });
    Object.defineProperty(withPath, "path", { value: "/tmp/bar.py" });

    fireEvent.change(input as HTMLInputElement, {
      target: { files: [duplicateName, withPath] },
    });

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith([
      { name: "foo.ts", path: "foo.ts" },
      { name: "bar.py", path: "/tmp/bar.py" },
    ]);
  });

  it("caps added attachments at the maximum", () => {
    const onChange = vi.fn();
    const existing = Array.from({ length: 9 }, (_, i) => ({
      name: `file-${i + 1}.txt`,
      path: `file-${i + 1}.txt`,
    }));

    const { container } = render(
      <Composer
        {...baseProps}
        attachedFiles={existing}
        onAttachedFilesChange={onChange}
      />
    );

    const input = container.querySelector('input[type="file"]');
    expect(input).not.toBeNull();

    const files = [
      new File(["a"], "new-a.txt", { type: "text/plain" }),
      new File(["b"], "new-b.txt", { type: "text/plain" }),
      new File(["c"], "new-c.txt", { type: "text/plain" }),
    ];

    fireEvent.change(input as HTMLInputElement, {
      target: { files },
    });

    expect(onChange).toHaveBeenCalledTimes(1);
    const updated = onChange.mock.calls[0][0] as Array<{ name: string; path: string }>;
    expect(updated).toHaveLength(10);
    expect(updated.at(-1)).toEqual({ name: "new-a.txt", path: "new-a.txt" });
  });

  it("supports drag-and-drop and clears dragging state after drop", () => {
    const onChange = vi.fn();
    const { container } = render(
      <Composer
        {...baseProps}
        attachedFiles={[]}
        onAttachedFilesChange={onChange}
      />
    );

    const composer = container.querySelector(".composer");
    expect(composer).not.toBeNull();

    fireEvent.dragOver(composer as HTMLElement);
    expect((composer as HTMLElement).classList.contains("is-dragging")).toBe(true);

    const dropped = new File(["demo"], "drop.txt", { type: "text/plain" });
    fireEvent.drop(composer as HTMLElement, {
      dataTransfer: { files: [dropped] },
    });

    expect((composer as HTMLElement).classList.contains("is-dragging")).toBe(false);
    expect(onChange).toHaveBeenCalledWith([{ name: "drop.txt", path: "drop.txt" }]);
  });
});
