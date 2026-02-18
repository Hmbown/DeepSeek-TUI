import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ConnectionBanner } from "@/components/chat/ConnectionBanner";

describe("ConnectionBanner", () => {
  it("renders reconnecting state actions", () => {
    const retry = vi.fn();
    const openSettings = vi.fn();

    render(
      <ConnectionBanner
        state="reconnecting"
        message="Live stream disconnected"
        baseUrl="http://127.0.0.1:7878"
        onRetryNow={retry}
        onOpenSettings={openSettings}
      />
    );

    expect(screen.getByText("Live stream disconnected")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /retry now/i }));
    fireEvent.click(screen.getByRole("button", { name: /runtime:/i }));
    expect(retry).toHaveBeenCalledTimes(1);
    expect(openSettings).toHaveBeenCalledTimes(1);
  });
});
