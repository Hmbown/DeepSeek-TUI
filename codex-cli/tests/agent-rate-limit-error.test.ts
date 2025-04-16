import { describe, it, expect, vi } from "vitest";

// ---------------------------------------------------------------------------
//  Utility: fake OpenAI SDK with programmable behaviour per test case.
// ---------------------------------------------------------------------------

// Same helper as used in agent-network-errors.test.ts – duplicated here to keep
// the test file self‑contained.
// Exported so that the strict TypeScript compiler does not flag it as unused –
// individual tests may import it for ad‑hoc diagnostics when debugging.
export function _createStream(events: Array<any>) {
  return new (class {
    public controller = { abort: vi.fn() };

    async *[Symbol.asyncIterator]() {
      for (const ev of events) {
        yield ev;
      }
    }
  })();
}

// Holders so tests can access spies/state injected by the mock.
const openAiState: { createSpy?: ReturnType<typeof vi.fn> } = {};

vi.mock("openai", () => {
  class RateLimitError extends Error {
    public code = "rate_limit_exceeded";
    constructor(message: string) {
      super(message);
      this.name = "RateLimitError";
    }
  }

  // Re‑export the timeout error as well so other tests that expect it continue
  // to work regardless of execution order.
  class APIConnectionTimeoutError extends Error {}

  class FakeOpenAI {
    public responses = {
      // `createSpy` will be swapped out per test.
      create: (...args: Array<any>) => openAiState.createSpy!(...args),
    };
  }

  return {
    __esModule: true,
    default: FakeOpenAI,
    RateLimitError,
    APIConnectionTimeoutError,
  };
});

// Stub approvals / formatting helpers – not relevant to rate‑limit handling.
vi.mock("@lib/approvals.js", () => ({
  __esModule: true,
  alwaysApprovedCommands: new Set<string>(),
  canAutoApprove: () => ({ type: "auto-approve", runInSandbox: false } as any),
  isSafeCommand: () => null,
}));

vi.mock("@lib/format-command.js", () => ({
  __esModule: true,
  formatCommandForDisplay: (c: Array<string>) => c.join(" "),
}));

// Silence debug logging from agent‑loop so test output remains clean.
vi.mock("../src/utils/agent/log.js", () => ({
  __esModule: true,
  log: () => {},
  isLoggingEnabled: () => false,
}));

import { AgentLoop } from "../src/utils/agent/agent-loop.js";

describe("AgentLoop – OpenAI rate limit errors", () => {
  it("surfaces a user‑friendly system message instead of throwing on RateLimitError (TDD – expected to fail)", async () => {
    // Arrange fake OpenAI: every call fails with a rate‑limit error.
    const rateLimitErrMsg =
      "Rate limit reached: Limit 20, Used 20, Requested 1. Please try again.";

    openAiState.createSpy = vi.fn(async () => {
      // Simulate the SDK throwing before any streaming begins.
      // In real life this happens when the HTTP response status is 429.
      const err: any = new Error(rateLimitErrMsg);
      err.code = "rate_limit_exceeded";
      throw err;
    });

    const received: Array<any> = [];

    const agent = new AgentLoop({
      model: "any",
      instructions: "",
      approvalPolicy: { mode: "auto" } as any,
      onItem: (i) => received.push(i),
      onLoading: () => {},
      getCommandConfirmation: async () => ({ review: "yes" } as any),
      onLastResponseId: () => {},
    });

    const userMsg = [
      {
        type: "message",
        role: "user",
        content: [{ type: "input_text", text: "hello" }],
      },
    ];

    // The desired behaviour (not yet implemented): AgentLoop should catch the
    // rate‑limit error, emit a helpful system message and resolve without
    // throwing so callers can let the user retry.
    await expect(agent.run(userMsg as any)).resolves.not.toThrow();

    // Let flush timers run.
    await new Promise((r) => setTimeout(r, 20));

    const sysMsg = received.find(
      (i) =>
        i.role === "system" &&
        typeof i.content?.[0]?.text === "string" &&
        i.content[0].text.includes("Rate limit"),
    );

    expect(sysMsg).toBeTruthy();
  });
});
