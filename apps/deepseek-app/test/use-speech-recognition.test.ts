import { renderHook, act } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

import { useSpeechRecognition } from "@/hooks/use-speech-recognition";

// Mock SpeechRecognition
class MockSpeechRecognition {
  continuous = false;
  interimResults = false;
  lang = "";
  onresult: ((event: unknown) => void) | null = null;
  onend: (() => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;

  start = vi.fn();
  stop = vi.fn(() => {
    this.onend?.();
  });
  abort = vi.fn();
}

describe("useSpeechRecognition", () => {
  let originalSpeechRecognition: unknown;

  beforeEach(() => {
    originalSpeechRecognition = (globalThis as Record<string, unknown>).webkitSpeechRecognition;
  });

  afterEach(() => {
    if (originalSpeechRecognition === undefined) {
      delete (globalThis as Record<string, unknown>).webkitSpeechRecognition;
    } else {
      (globalThis as Record<string, unknown>).webkitSpeechRecognition = originalSpeechRecognition;
    }
  });

  it("reports unavailable when API is missing", () => {
    delete (globalThis as Record<string, unknown>).webkitSpeechRecognition;
    delete (globalThis as Record<string, unknown>).SpeechRecognition;

    const { result } = renderHook(() => useSpeechRecognition());

    expect(result.current.isAvailable).toBe(false);
    expect(result.current.isListening).toBe(false);
  });

  it("reports available when API exists", () => {
    (globalThis as Record<string, unknown>).webkitSpeechRecognition = MockSpeechRecognition;

    const { result } = renderHook(() => useSpeechRecognition());

    expect(result.current.isAvailable).toBe(true);
  });

  it("sets isListening to true on start", () => {
    (globalThis as Record<string, unknown>).webkitSpeechRecognition = MockSpeechRecognition;

    const { result } = renderHook(() => useSpeechRecognition());

    act(() => {
      result.current.start();
    });

    expect(result.current.isListening).toBe(true);
  });

  it("sets isListening to false on stop", () => {
    (globalThis as Record<string, unknown>).webkitSpeechRecognition = MockSpeechRecognition;

    const { result } = renderHook(() => useSpeechRecognition());

    act(() => {
      result.current.start();
    });

    act(() => {
      result.current.stop();
    });

    expect(result.current.isListening).toBe(false);
  });
});
