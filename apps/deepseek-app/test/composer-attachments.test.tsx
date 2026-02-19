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
});
