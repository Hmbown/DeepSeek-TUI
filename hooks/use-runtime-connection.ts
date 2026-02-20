import { useCallback, useEffect, useState } from "react";

import { getHealth, parseApiError, type HealthResponse } from "@/lib/runtime-api";

export type ConnectionState = "checking" | "online" | "offline" | "reconnecting";

function healthErrorMessage(error: unknown): string {
  const parsed = parseApiError(error);
  return `${parsed.message} (${parsed.status})`;
}

export function useRuntimeConnection(baseUrl: string, pollMs = 5000) {
  const [state, setState] = useState<ConnectionState>("checking");
  const [message, setMessage] = useState("Checking runtime API...");
  const [lastCheckedAt, setLastCheckedAt] = useState<number | null>(null);
  const [lastHealth, setLastHealth] = useState<HealthResponse | null>(null);

  const refreshHealth = useCallback(async () => {
    try {
      const health = await getHealth(baseUrl);
      if (health.status === "ok") {
        setState("online");
        setMessage("Runtime online");
      } else {
        setState("offline");
        setMessage("Runtime unhealthy");
      }
      setLastHealth(health);
    } catch (error) {
      setState((prev) => (prev === "reconnecting" ? "reconnecting" : "offline"));
      setMessage(`Runtime unavailable: ${healthErrorMessage(error)}`);
      setLastHealth(null);
    } finally {
      setLastCheckedAt(Date.now());
    }
  }, [baseUrl]);

  const retryNow = useCallback(async () => {
    setState("checking");
    setMessage("Rechecking runtime API...");
    await refreshHealth();
  }, [refreshHealth]);

  const markStreamDisconnected = useCallback((reason?: string) => {
    setState((prev) => (prev === "offline" ? "offline" : "reconnecting"));
    setMessage(reason?.trim() ? reason : "Live stream disconnected. Reconnecting...");
  }, []);

  const markStreamConnected = useCallback(() => {
    setState("online");
    setMessage("Live stream connected");
  }, []);

  useEffect(() => {
    setState("checking");
    setMessage("Checking runtime API...");
    void refreshHealth();

    const timer = window.setInterval(() => {
      void refreshHealth();
    }, pollMs);
    return () => window.clearInterval(timer);
  }, [refreshHealth, pollMs]);

  return {
    state,
    message,
    lastCheckedAt,
    refreshHealth,
    retryNow,
    markStreamDisconnected,
    markStreamConnected,
    isHealthy: state === "online",
    lastHealth,
  };
}
