import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import Home from "@/app/page";

class MockEventSource {
  onerror: ((event: Event) => void) | null = null;

  addEventListener() {
    // no-op for tests
  }

  close() {
    // no-op for tests
  }
}

function okJson(payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status: 200,
    headers: {
      "content-type": "application/json",
    },
  });
}

beforeEach(() => {
  vi.stubGlobal("EventSource", MockEventSource as unknown as typeof EventSource);
  Object.defineProperty(window, "localStorage", {
    configurable: true,
    value: {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
      removeItem: vi.fn(),
    },
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Automation UI", () => {
  it("validates CWD entries and applies hourly RRULE builder output", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      const method = (init?.method ?? "GET").toUpperCase();

      if (url.endsWith("/health")) {
        return okJson({ status: "ok", service: "wagmii-runtime-api", mode: "local" });
      }
      if (url.includes("/v1/workspace/status")) {
        return okJson({
          workspace: "/repo",
          git_repo: true,
          branch: "main",
          staged: 1,
          unstaged: 2,
          untracked: 0,
          ahead: 0,
          behind: 0,
        });
      }
      if (url.includes("/v1/threads/summary")) {
        return okJson([]);
      }
      if (url.includes("/v1/automations") && method === "GET") {
        return okJson([]);
      }

      return okJson({});
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<Home />);

    fireEvent.click(screen.getByRole("button", { name: "Automations" }));

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Create Automation" })).toBeInTheDocument();
    });

    fireEvent.change(screen.getByPlaceholderText("/path/to/workspace"), {
      target: { value: "https://example.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: /add/i }));

    expect(
      screen.getByText("CWD must be a local path and cannot include URL schemes.")
    ).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("/path/to/workspace"), {
      target: { value: "/tmp/work" },
    });
    fireEvent.click(screen.getByRole("button", { name: /add/i }));

    expect(screen.getByText("/tmp/work")).toBeInTheDocument();

    const scheduleSelect = screen
      .getAllByRole("combobox")
      .find((node) => node.textContent?.includes("Hourly interval"));
    expect(scheduleSelect).toBeDefined();
    if (!scheduleSelect) {
      return;
    }
    fireEvent.change(scheduleSelect, { target: { value: "hourly" } });

    fireEvent.change(screen.getByPlaceholderText("Interval (hours)"), {
      target: { value: "3" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Use builder value" }));

    const rruleInput = screen.getByDisplayValue(/FREQ=HOURLY/);
    expect(rruleInput).toBeInTheDocument();
  });
});
