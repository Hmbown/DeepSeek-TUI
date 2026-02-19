import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Composer } from "@/components/chat/Composer";

afterEach(cleanup);

const baseProps = {
  value: "hello",
  onValueChange: vi.fn(),
  onSend: vi.fn(),
  onRetrySend: vi.fn(),
  sending: false as const,
  selectedThreadId: "thr_123",
  activeTurnId: null,
};

describe("Composer plan mode toggle", () => {
  it("toggles mode to plan on click", () => {
    const onModeChange = vi.fn();
    render(
      <Composer
        {...baseProps}
        mode="agent"
        onModeChange={onModeChange}
      />
    );

    const planBtn = screen.getByRole("button", { name: /plan/i });
    fireEvent.click(planBtn);
    expect(onModeChange).toHaveBeenCalledWith("plan");
  });

  it("restores previous mode when clicking plan again", () => {
    const onModeChange = vi.fn();
    render(
      <Composer
        {...baseProps}
        mode="plan"
        onModeChange={onModeChange}
      />
    );

    const planBtn = screen.getByRole("button", { name: /plan/i });
    expect(planBtn.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(planBtn);
    // Restores to "agent" (the default previousMode when mode is already "plan" on mount)
    expect(onModeChange).toHaveBeenCalledWith("agent");
  });

  it("shows active styling when mode is plan", () => {
    render(
      <Composer
        {...baseProps}
        mode="plan"
        onModeChange={vi.fn()}
      />
    );

    const planBtn = screen.getByRole("button", { name: /plan/i });
    expect(planBtn.classList.contains("is-active")).toBe(true);
  });
});
