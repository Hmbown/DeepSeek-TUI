"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  ChevronRight,
  CircleX,
  Plus,
  RefreshCcw,
  Search,
  Trash2,
  X,
} from "lucide-react";

import { CommandPalette, type CommandPaletteItem } from "@/components/palette/CommandPalette";
import { LiveEventsPanel } from "@/components/chat/LiveEventsPanel";
import { ConnectionBanner } from "@/components/chat/ConnectionBanner";
import { Composer } from "@/components/chat/Composer";
import { TranscriptPane } from "@/components/chat/TranscriptPane";
import { LeftRail } from "@/components/layout/LeftRail";
import { TopBar } from "@/components/topbar/TopBar";
import { ThreadsPane } from "@/components/threads/ThreadsPane";
import { TaskDetailPanel } from "@/components/tasks/TaskDetailPanel";
import type { SchedulePreset, Section, ThreadFilter } from "@/components/types";
import { useKeyboardShortcuts } from "@/hooks/use-keyboard-shortcuts";
import { useRuntimeConnection } from "@/hooks/use-runtime-connection";
import {
  DEFAULT_RUNTIME_BASE_URL,
  compactThread,
  createAutomation,
  createThread,
  deleteAutomation,
  deleteSession,
  forkThread,
  getTask,
  getThreadDetail,
  getWorkspaceStatus,
  interruptTurn,
  listAutomationRuns,
  listAutomations,
  listMcpServers,
  listMcpTools,
  listSessions,
  listSkills,
  resumeSessionThread,
  listTasks,
  listThreadSummaries,
  loadRuntimeBaseUrl,
  openThreadEvents,
  parseApiError,
  pauseAutomation,
  persistRuntimeBaseUrl,
  resumeAutomation,
  resumeThread,
  runAutomation,
  startTurn,
  steerTurn,
  updateAutomation,
  updateThread,
  type AutomationRecord,
  type AutomationRunRecord,
  type McpServerEntry,
  type McpToolEntry,
  type SessionMetadata,
  type SkillsResponse,
  type TaskSummary,
  type ThreadDetail,
  type ThreadSummary,
  type WorkspaceStatus,
} from "@/lib/runtime-api";
import { buildTranscript, filterThreadSummaries, findActiveTurnId } from "@/lib/thread-utils";

const MODE_OPTIONS = ["agent", "plan", "normal", "yolo"];
const MODEL_OPTIONS = ["deepseek-reasoner", "deepseek-chat"];
const DEFAULT_RRULE = "FREQ=WEEKLY;BYDAY=MO,WE,FR;BYHOUR=9;BYMINUTE=0";
const WEEKDAY_OPTIONS = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
const MAX_LIVE_EVENTS = 40;

type PaletteMode = "commands" | "sessions";

function normalizeDayList(days: string[]): string[] {
  const valid = new Set(WEEKDAY_OPTIONS);
  return Array.from(new Set(days.filter((day) => valid.has(day))));
}

function clampNumber(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function buildWeeklyRRule(days: string[], hour: number, minute: number): string {
  const normalizedDays = normalizeDayList(days);
  const byday = (normalizedDays.length > 0 ? normalizedDays : ["MO"]).join(",");
  const byhour = clampNumber(hour, 0, 23);
  const byminute = clampNumber(minute, 0, 59);
  return `FREQ=WEEKLY;BYDAY=${byday};BYHOUR=${byhour};BYMINUTE=${byminute}`;
}

function buildHourlyRRule(intervalHours: number, days: string[]): string {
  const interval = Math.max(1, Math.floor(intervalHours || 1));
  const normalizedDays = normalizeDayList(days);
  if (normalizedDays.length === 0) {
    return `FREQ=HOURLY;INTERVAL=${interval}`;
  }
  return `FREQ=HOURLY;INTERVAL=${interval};BYDAY=${normalizedDays.join(",")}`;
}

function parseSupportedRRule(rrule: string): {
  preset: SchedulePreset;
  weeklyDays: string[];
  weeklyHour: number;
  weeklyMinute: number;
  hourlyInterval: number;
  hourlyDays: string[];
} | null {
  const source = rrule.trim().toUpperCase();
  if (!source) {
    return null;
  }

  const parts = new Map<string, string>();
  for (const part of source.split(";")) {
    const [rawKey, rawValue] = part.split("=", 2);
    if (!rawKey || !rawValue) {
      continue;
    }
    parts.set(rawKey, rawValue);
  }

  const freq = parts.get("FREQ");
  if (freq === "WEEKLY") {
    const days = normalizeDayList((parts.get("BYDAY") ?? "").split(",").filter(Boolean));
    const hour = Number(parts.get("BYHOUR") ?? "9");
    const minute = Number(parts.get("BYMINUTE") ?? "0");
    return {
      preset: "weekly",
      weeklyDays: days.length > 0 ? days : ["MO"],
      weeklyHour: Number.isFinite(hour) ? clampNumber(hour, 0, 23) : 9,
      weeklyMinute: Number.isFinite(minute) ? clampNumber(minute, 0, 59) : 0,
      hourlyInterval: 1,
      hourlyDays: [],
    };
  }

  if (freq === "HOURLY") {
    const interval = Number(parts.get("INTERVAL") ?? "1");
    const days = normalizeDayList((parts.get("BYDAY") ?? "").split(",").filter(Boolean));
    return {
      preset: "hourly",
      weeklyDays: ["MO"],
      weeklyHour: 9,
      weeklyMinute: 0,
      hourlyInterval: Number.isFinite(interval) ? Math.max(1, Math.floor(interval)) : 1,
      hourlyDays: days,
    };
  }

  return null;
}

function formatTimestamp(value?: string | null): string {
  if (!value) {
    return "-";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString();
}

function formatRelative(value?: string | null): string {
  if (!value) {
    return "-";
  }
  const then = new Date(value).getTime();
  if (Number.isNaN(then)) {
    return value;
  }
  const diffMs = Date.now() - then;
  const abs = Math.abs(diffMs);
  const mins = Math.floor(abs / 60000);
  if (mins < 1) {
    return "just now";
  }
  if (mins < 60) {
    return `${mins}m`;
  }
  const hours = Math.floor(mins / 60);
  if (hours < 24) {
    return `${hours}h`;
  }
  const days = Math.floor(hours / 24);
  return `${days}d`;
}

function errorText(error: unknown): string {
  const parsed = parseApiError(error);
  return `${parsed.message} (${parsed.status})`;
}

function humanRrule(rrule: string): string {
  const parsed = parseSupportedRRule(rrule);
  if (!parsed) {
    return "Custom schedule";
  }
  if (parsed.preset === "weekly") {
    return `Weekly on ${parsed.weeklyDays.join(", ")} at ${String(parsed.weeklyHour).padStart(2, "0")}:${String(parsed.weeklyMinute).padStart(2, "0")}`;
  }
  if (parsed.hourlyDays.length > 0) {
    return `Every ${parsed.hourlyInterval} hour(s) on ${parsed.hourlyDays.join(", ")}`;
  }
  return `Every ${parsed.hourlyInterval} hour(s)`;
}

function isValidLocalCwd(path: string): boolean {
  const trimmed = path.trim();
  if (!trimmed) {
    return false;
  }
  return !trimmed.includes("://");
}

function applyThreadFilter(items: ThreadSummary[], filter: ThreadFilter): ThreadSummary[] {
  if (filter === "all") {
    return items;
  }
  if (filter === "archived") {
    return items.filter((item) => item.archived);
  }
  return items.filter((item) => !item.archived);
}

export default function Home() {
  const [section, setSection] = useState<Section>("chat");

  const [baseUrlInput, setBaseUrlInput] = useState(DEFAULT_RUNTIME_BASE_URL);
  const [baseUrl, setBaseUrl] = useState(DEFAULT_RUNTIME_BASE_URL);
  const [workspace, setWorkspace] = useState<WorkspaceStatus | null>(null);

  const [threadSearch, setThreadSearch] = useState("");
  const [threadFilter, setThreadFilter] = useState<ThreadFilter>("active");
  const [threads, setThreads] = useState<ThreadSummary[]>([]);
  const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null);
  const [threadDetail, setThreadDetail] = useState<ThreadDetail | null>(null);
  const [liveEvents, setLiveEvents] = useState<string[]>([]);

  const [composerText, setComposerText] = useState("");
  const [steerText, setSteerText] = useState("");
  const [model, setModel] = useState("deepseek-reasoner");
  const [mode, setMode] = useState("agent");
  const [sending, setSending] = useState(false);

  const [automations, setAutomations] = useState<AutomationRecord[]>([]);
  const [selectedAutomationId, setSelectedAutomationId] = useState<string | null>(null);
  const [automationRuns, setAutomationRuns] = useState<AutomationRunRecord[]>([]);
  const [editingAutomationId, setEditingAutomationId] = useState<string | null>(null);
  const [automationName, setAutomationName] = useState("Daily Review");
  const [automationPrompt, setAutomationPrompt] = useState(
    "Summarize my outstanding work and list the top 3 priorities for today."
  );
  const [automationRrule, setAutomationRrule] = useState(DEFAULT_RRULE);
  const [automationStatus, setAutomationStatus] = useState<"active" | "paused">("active");
  const [automationCwds, setAutomationCwds] = useState<string[]>([]);
  const [newCwdInput, setNewCwdInput] = useState("");
  const [schedulePreset, setSchedulePreset] = useState<SchedulePreset>("weekly");
  const [weeklyDays, setWeeklyDays] = useState<string[]>(["MO", "WE", "FR"]);
  const [weeklyHour, setWeeklyHour] = useState(9);
  const [weeklyMinute, setWeeklyMinute] = useState(0);
  const [hourlyInterval, setHourlyInterval] = useState(1);
  const [hourlyDays, setHourlyDays] = useState<string[]>([]);
  const [automationBusyId, setAutomationBusyId] = useState<string | null>(null);
  const [confirmDeleteAutomationId, setConfirmDeleteAutomationId] = useState<string | null>(null);
  const [automationValidationError, setAutomationValidationError] = useState<string | null>(null);

  const [skills, setSkills] = useState<SkillsResponse | null>(null);
  const [mcpServers, setMcpServers] = useState<McpServerEntry[]>([]);
  const [mcpTools, setMcpTools] = useState<McpToolEntry[]>([]);
  const [skillsSearch, setSkillsSearch] = useState("");
  const [toolsSearch, setToolsSearch] = useState("");
  const [serverFilter, setServerFilter] = useState<string>("all");

  const [sessions, setSessions] = useState<SessionMetadata[]>([]);
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  const [selectedTaskDetail, setSelectedTaskDetail] = useState<import("@/lib/runtime-api").TaskRecord | null>(null);
  const [taskDetailLoading, setTaskDetailLoading] = useState(false);

  const [notice, setNotice] = useState<string | null>(null);
  const [errorNotice, setErrorNotice] = useState<string | null>(null);

  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteMode, setPaletteMode] = useState<PaletteMode>("commands");
  const [paletteQuery, setPaletteQuery] = useState("");

  const eventSourceRef = useRef<EventSource | null>(null);
  const detailRefreshTimer = useRef<number | null>(null);
  const reconnectTimer = useRef<number | null>(null);
  const reconnectAttempt = useRef(0);
  const lastSeq = useRef(0);

  const {
    state: connectionState,
    message: connectionMessage,
    retryNow,
    refreshHealth,
    markStreamDisconnected,
    markStreamConnected,
  } = useRuntimeConnection(baseUrl);

  const isEditingAutomation = editingAutomationId != null;

  const filteredThreads = useMemo(() => {
    const searched = filterThreadSummaries(threads, threadSearch);
    return applyThreadFilter(searched, threadFilter);
  }, [threadFilter, threadSearch, threads]);

  const transcript = useMemo(() => buildTranscript(threadDetail), [threadDetail]);
  const activeTurnId = useMemo(() => findActiveTurnId(threadDetail), [threadDetail]);
  const automationDraftFromBuilder = useMemo(
    () =>
      schedulePreset === "weekly"
        ? buildWeeklyRRule(weeklyDays, weeklyHour, weeklyMinute)
        : buildHourlyRRule(hourlyInterval, hourlyDays),
    [schedulePreset, weeklyDays, weeklyHour, weeklyMinute, hourlyInterval, hourlyDays]
  );
  const currentAutomation = useMemo(
    () => automations.find((item) => item.id === selectedAutomationId) ?? null,
    [automations, selectedAutomationId]
  );

  const filteredSkills = useMemo(() => {
    const source = skills?.skills ?? [];
    const query = skillsSearch.trim().toLowerCase();
    if (!query) {
      return source;
    }
    return source.filter((skill) => {
      return `${skill.name} ${skill.description} ${skill.path}`.toLowerCase().includes(query);
    });
  }, [skills?.skills, skillsSearch]);

  const filteredTools = useMemo(() => {
    const query = toolsSearch.trim().toLowerCase();
    return mcpTools.filter((tool) => {
      if (serverFilter !== "all" && tool.server !== serverFilter) {
        return false;
      }
      if (!query) {
        return true;
      }
      return `${tool.prefixed_name} ${tool.description ?? ""}`.toLowerCase().includes(query);
    });
  }, [mcpTools, serverFilter, toolsSearch]);

  const refreshWorkspace = useCallback(async () => {
    try {
      const data = await getWorkspaceStatus(baseUrl);
      setWorkspace(data);
    } catch {
      setWorkspace(null);
    }
  }, [baseUrl]);

  const refreshThreads = useCallback(async (filterOverride?: ThreadFilter) => {
    try {
      const effectiveFilter = filterOverride ?? threadFilter;
      const includeArchived = effectiveFilter !== "active";
      const list = await listThreadSummaries(baseUrl, { limit: 180, includeArchived });
      setThreads(list);
      const filtered = applyThreadFilter(list, effectiveFilter);
      if (!selectedThreadId && filtered.length > 0) {
        setSelectedThreadId(filtered[0].id);
      }
      if (selectedThreadId && !list.some((item) => item.id === selectedThreadId)) {
        setSelectedThreadId(filtered[0]?.id ?? null);
      }
    } catch (error) {
      setErrorNotice(`Failed to load threads: ${errorText(error)}`);
    }
  }, [baseUrl, selectedThreadId, threadFilter]);

  const refreshThreadDetail = useCallback(
    async (threadId: string) => {
      const detail = await getThreadDetail(baseUrl, threadId);
      setThreadDetail(detail);
    },
    [baseUrl]
  );

  const refreshAutomations = useCallback(async () => {
    try {
      const list = await listAutomations(baseUrl);
      setAutomations(list);
      if (!selectedAutomationId && list.length > 0) {
        setSelectedAutomationId(list[0].id);
      } else if (selectedAutomationId && !list.some((item) => item.id === selectedAutomationId)) {
        setSelectedAutomationId(list[0]?.id ?? null);
      }
    } catch (error) {
      setErrorNotice(`Failed to load automations: ${errorText(error)}`);
    }
  }, [baseUrl, selectedAutomationId]);

  const refreshAutomationRuns = useCallback(
    async (automationId: string) => {
      try {
        const runs = await listAutomationRuns(baseUrl, automationId, 40);
        setAutomationRuns(runs);
      } catch (error) {
        setErrorNotice(`Failed to load automation runs: ${errorText(error)}`);
      }
    },
    [baseUrl]
  );

  const refreshSkillsAndApps = useCallback(async () => {
    try {
      const [skillsResp, serversResp, toolsResp] = await Promise.all([
        listSkills(baseUrl),
        listMcpServers(baseUrl),
        listMcpTools(baseUrl),
      ]);
      setSkills(skillsResp);
      setMcpServers(serversResp.servers);
      setMcpTools(toolsResp.tools);
      setServerFilter((current) => {
        if (current === "all") {
          return current;
        }
        return serversResp.servers.some((item) => item.name === current) ? current : "all";
      });
    } catch (error) {
      setErrorNotice(`Failed to load skills/apps: ${errorText(error)}`);
    }
  }, [baseUrl]);

  const refreshSessions = useCallback(async () => {
    try {
      const response = await listSessions(baseUrl, { limit: 60, search: paletteMode === "sessions" ? paletteQuery : undefined });
      setSessions(response.sessions);
    } catch (error) {
      setErrorNotice(`Failed to load sessions: ${errorText(error)}`);
    }
  }, [baseUrl, paletteMode, paletteQuery]);

  const refreshTasks = useCallback(async () => {
    try {
      const response = await listTasks(baseUrl, { limit: 20 });
      setTasks(response.tasks);
    } catch {
      setTasks([]);
    }
  }, [baseUrl]);

  const openTaskDetail = useCallback(
    async (taskId: string, targetBaseUrl?: string) => {
      const runtimeBaseUrl = targetBaseUrl ?? baseUrl;
      setTaskDetailLoading(true);
      try {
        const detail = await getTask(runtimeBaseUrl, taskId);
        setSelectedTaskDetail(detail);
      } catch (error) {
        setErrorNotice(`Failed to load task: ${errorText(error)}`);
        setSelectedTaskDetail(null);
      } finally {
        setTaskDetailLoading(false);
      }
    },
    [baseUrl]
  );

  useEffect(() => {
    const isActive =
      selectedTaskDetail &&
      (selectedTaskDetail.status === "running" || selectedTaskDetail.status === "queued");
    if (!isActive) return;
    const id = selectedTaskDetail.id;
    const timer = window.setInterval(async () => {
      try {
        const refreshed = await getTask(baseUrl, id);
        setSelectedTaskDetail(refreshed);
      } catch {
        /* ignore transient polling errors */
      }
    }, 3000);
    return () => window.clearInterval(timer);
  }, [selectedTaskDetail?.id, selectedTaskDetail?.status, baseUrl]); // eslint-disable-line react-hooks/exhaustive-deps

  const resetAutomationForm = useCallback(() => {
    setEditingAutomationId(null);
    setAutomationName("Daily Review");
    setAutomationPrompt("Summarize my outstanding work and list the top 3 priorities for today.");
    setAutomationRrule(DEFAULT_RRULE);
    setAutomationStatus("active");
    setAutomationCwds([]);
    setNewCwdInput("");
    setSchedulePreset("weekly");
    setWeeklyDays(["MO", "WE", "FR"]);
    setWeeklyHour(9);
    setWeeklyMinute(0);
    setHourlyInterval(1);
    setHourlyDays([]);
    setAutomationValidationError(null);
    setConfirmDeleteAutomationId(null);
  }, []);

  const applyScheduleFromRrule = useCallback((rrule: string) => {
    const parsed = parseSupportedRRule(rrule);
    if (!parsed) {
      return;
    }
    setSchedulePreset(parsed.preset);
    setWeeklyDays(parsed.weeklyDays);
    setWeeklyHour(parsed.weeklyHour);
    setWeeklyMinute(parsed.weeklyMinute);
    setHourlyInterval(parsed.hourlyInterval);
    setHourlyDays(parsed.hourlyDays);
  }, []);

  const loadAutomationIntoForm = useCallback(
    (automation: AutomationRecord) => {
      setEditingAutomationId(automation.id);
      setAutomationName(automation.name);
      setAutomationPrompt(automation.prompt);
      setAutomationRrule(automation.rrule);
      setAutomationStatus(automation.status);
      setAutomationCwds(automation.cwds);
      setNewCwdInput("");
      setAutomationValidationError(null);
      setConfirmDeleteAutomationId(null);
      applyScheduleFromRrule(automation.rrule);
    },
    [applyScheduleFromRrule]
  );

  const toggleDay = useCallback((target: "weekly" | "hourly", day: string) => {
    if (target === "weekly") {
      setWeeklyDays((current) => {
        if (current.includes(day)) {
          return current.filter((item) => item !== day);
        }
        return normalizeDayList([...current, day]);
      });
      return;
    }

    setHourlyDays((current) => {
      if (current.includes(day)) {
        return current.filter((item) => item !== day);
      }
      return normalizeDayList([...current, day]);
    });
  }, []);

  const addCwdToForm = useCallback(() => {
    const value = newCwdInput.trim();
    if (!value) {
      return;
    }
    if (!isValidLocalCwd(value)) {
      setAutomationValidationError("CWD must be a local path and cannot include URL schemes.");
      return;
    }
    setAutomationCwds((current) => {
      if (current.includes(value)) {
        return current;
      }
      return [...current, value];
    });
    setNewCwdInput("");
    setAutomationValidationError(null);
  }, [newCwdInput]);

  const removeCwdFromForm = useCallback((cwd: string) => {
    setAutomationCwds((current) => current.filter((item) => item !== cwd));
  }, []);

  useEffect(() => {
    const stored = loadRuntimeBaseUrl();
    setBaseUrl(stored);
    setBaseUrlInput(stored);

    const params = new URLSearchParams(window.location.search);
    const taskId = params.get("task");
    if (taskId) {
      setSection("settings");
      void openTaskDetail(taskId, stored);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 4000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    if (!errorNotice) return;
    const timer = window.setTimeout(() => setErrorNotice(null), 6000);
    return () => window.clearTimeout(timer);
  }, [errorNotice]);

  useEffect(() => {
    void refreshWorkspace();
    const timer = window.setInterval(() => {
      void refreshWorkspace();
    }, 5000);
    return () => window.clearInterval(timer);
  }, [refreshWorkspace]);

  useEffect(() => {
    if (section === "chat") {
      void refreshThreads();
    }
    if (section === "automations") {
      void refreshAutomations();
    }
    if (section === "skills") {
      void refreshSkillsAndApps();
    }
    if (section === "settings") {
      void refreshTasks();
    }
  }, [section, refreshThreads, refreshAutomations, refreshSkillsAndApps, refreshTasks]);

  useEffect(() => {
    if (!selectedAutomationId || section !== "automations") {
      setAutomationRuns([]);
      return;
    }
    void refreshAutomationRuns(selectedAutomationId);
  }, [section, selectedAutomationId, refreshAutomationRuns]);

  useEffect(() => {
    if (paletteOpen && paletteMode === "sessions") {
      void refreshSessions();
    }
  }, [paletteOpen, paletteMode, paletteQuery, refreshSessions]);

  useEffect(() => {
    if (section !== "chat" || !selectedThreadId) {
      setThreadDetail(null);
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
        eventSourceRef.current = null;
      }
      if (reconnectTimer.current) {
        window.clearTimeout(reconnectTimer.current);
        reconnectTimer.current = null;
      }
      return;
    }

    let cancelled = false;

    const closeStream = () => {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
        eventSourceRef.current = null;
      }
    };

    const queueRefresh = () => {
      if (detailRefreshTimer.current) {
        window.clearTimeout(detailRefreshTimer.current);
      }
      detailRefreshTimer.current = window.setTimeout(() => {
        void refreshThreadDetail(selectedThreadId).catch(() => undefined);
        void refreshThreads();
      }, 120);
    };

    const scheduleReconnect = (reason: string) => {
      markStreamDisconnected(reason);
      reconnectAttempt.current += 1;
      const base = 500;
      const capped = Math.min(12_000, base * 2 ** (reconnectAttempt.current - 1));
      const delay = capped + Math.floor(Math.random() * 250);

      if (reconnectTimer.current) {
        window.clearTimeout(reconnectTimer.current);
      }

      reconnectTimer.current = window.setTimeout(() => {
        void connectStream();
      }, delay);
    };

    const connectStream = async () => {
      closeStream();
      try {
        const detail = await getThreadDetail(baseUrl, selectedThreadId);
        if (cancelled) {
          return;
        }

        setThreadDetail(detail);
        const sinceSeq = Math.max(lastSeq.current, 0);

        const source = openThreadEvents(
          baseUrl,
          selectedThreadId,
          sinceSeq,
          (event) => {
            if (typeof event.seq === "number") {
              if (event.seq <= lastSeq.current) {
                return;
              }
              lastSeq.current = event.seq;
            }

            const summary = `${event.event}${event.seq != null ? ` #${event.seq}` : ""}`;
            setLiveEvents((previous) => [summary, ...previous].slice(0, MAX_LIVE_EVENTS));
            queueRefresh();
          },
          () => {
            if (cancelled) {
              return;
            }
            closeStream();
            scheduleReconnect("Live stream disconnected. Reconnecting...");
          }
        );

        eventSourceRef.current = source;
        reconnectAttempt.current = 0;
        markStreamConnected();
      } catch (error) {
        if (cancelled) {
          return;
        }
        setErrorNotice(`Failed to load thread detail: ${errorText(error)}`);
        scheduleReconnect("Unable to connect to live stream. Retrying...");
      }
    };

    void connectStream();

    return () => {
      cancelled = true;
      closeStream();
      if (detailRefreshTimer.current) {
        window.clearTimeout(detailRefreshTimer.current);
        detailRefreshTimer.current = null;
      }
      if (reconnectTimer.current) {
        window.clearTimeout(reconnectTimer.current);
        reconnectTimer.current = null;
      }
    };
  }, [
    baseUrl,
    markStreamConnected,
    markStreamDisconnected,
    refreshThreadDetail,
    refreshThreads,
    section,
    selectedThreadId,
  ]);

  const applyRuntimeBaseUrl = useCallback(async () => {
    const normalized = baseUrlInput.trim() || DEFAULT_RUNTIME_BASE_URL;
    persistRuntimeBaseUrl(normalized);
    setBaseUrl(normalized);
    setNotice(`Runtime endpoint updated to ${normalized}`);
    setErrorNotice(null);
    await refreshHealth();
  }, [baseUrlInput, refreshHealth]);

  const handleCreateThread = useCallback(async () => {
    try {
      const created = await createThread(baseUrl, { model, mode });
      setSection("chat");
      setSelectedThreadId(created.id);
      setComposerText("");
      setLiveEvents([]);
      setNotice("Thread created");
      setErrorNotice(null);
      await refreshThreads();
    } catch (error) {
      setErrorNotice(`Failed to create thread: ${errorText(error)}`);
    }
  }, [baseUrl, mode, model, refreshThreads]);

  const handleForkThread = useCallback(async () => {
    if (!selectedThreadId) {
      return;
    }
    try {
      const forked = await forkThread(baseUrl, selectedThreadId);
      setSelectedThreadId(forked.id);
      setLiveEvents([]);
      await refreshThreads();
      setNotice("Thread forked");
      setErrorNotice(null);
    } catch (error) {
      setErrorNotice(`Failed to fork thread: ${errorText(error)}`);
    }
  }, [baseUrl, selectedThreadId, refreshThreads]);

  const handleResumeThread = useCallback(async () => {
    if (!selectedThreadId) {
      return;
    }
    try {
      await resumeThread(baseUrl, selectedThreadId);
      await refreshThreadDetail(selectedThreadId);
      setNotice("Thread resumed");
      setErrorNotice(null);
    } catch (error) {
      setErrorNotice(`Failed to resume thread: ${errorText(error)}`);
    }
  }, [baseUrl, refreshThreadDetail, selectedThreadId]);

  const handleCompactThread = useCallback(async () => {
    if (!selectedThreadId) {
      return;
    }
    try {
      await compactThread(baseUrl, selectedThreadId, { reason: "Manual compact from DeepSeek App" });
      setNotice("Compaction queued");
      setErrorNotice(null);
      await refreshThreadDetail(selectedThreadId);
    } catch (error) {
      setErrorNotice(`Failed to compact thread: ${errorText(error)}`);
    }
  }, [baseUrl, refreshThreadDetail, selectedThreadId]);

  const handleSend = useCallback(async () => {
    const prompt = composerText.trim();
    if (!prompt) {
      return;
    }

    setSending(true);
    setErrorNotice(null);
    setNotice(null);

    try {
      let threadId = selectedThreadId;
      if (!threadId) {
        const created = await createThread(baseUrl, { model, mode });
        threadId = created.id;
        setSelectedThreadId(threadId);
      }

      await startTurn(baseUrl, threadId, {
        prompt,
        model,
        mode,
      });

      setComposerText("");
      setLiveEvents([]);
      await refreshThreads();
      await refreshThreadDetail(threadId);
    } catch (error) {
      setErrorNotice(`Failed to send turn: ${errorText(error)}`);
    } finally {
      setSending(false);
    }
  }, [baseUrl, composerText, mode, model, refreshThreadDetail, refreshThreads, selectedThreadId]);

  const handleInterrupt = useCallback(async () => {
    if (!selectedThreadId || !activeTurnId) {
      return;
    }
    try {
      await interruptTurn(baseUrl, selectedThreadId, activeTurnId);
      setNotice("Interrupt requested");
      setErrorNotice(null);
    } catch (error) {
      setErrorNotice(`Failed to interrupt: ${errorText(error)}`);
    }
  }, [activeTurnId, baseUrl, selectedThreadId]);

  const handleSteer = useCallback(async () => {
    if (!selectedThreadId || !activeTurnId || !steerText.trim()) {
      return;
    }
    try {
      await steerTurn(baseUrl, selectedThreadId, activeTurnId, steerText.trim());
      setSteerText("");
      setNotice("Steer message sent");
      setErrorNotice(null);
    } catch (error) {
      setErrorNotice(`Failed to steer turn: ${errorText(error)}`);
    }
  }, [activeTurnId, baseUrl, selectedThreadId, steerText]);

  const handleThreadArchiveToggle = useCallback(
    async (thread: ThreadSummary) => {
      try {
        await updateThread(baseUrl, thread.id, { archived: !thread.archived });
        setNotice(thread.archived ? "Thread unarchived" : "Thread archived");
        setErrorNotice(null);
        await refreshThreads();
      } catch (error) {
        setErrorNotice(`Failed to update thread: ${errorText(error)}`);
      }
    },
    [baseUrl, refreshThreads]
  );

  const handleSaveAutomation = useCallback(async () => {
    if (!automationName.trim()) {
      setAutomationValidationError("Automation name is required.");
      return;
    }
    if (!automationPrompt.trim()) {
      setAutomationValidationError("Automation prompt is required.");
      return;
    }
    if (!automationRrule.trim()) {
      setAutomationValidationError("Automation RRULE is required.");
      return;
    }

    if (automationCwds.some((path) => !isValidLocalCwd(path))) {
      setAutomationValidationError("All CWD entries must be local paths.");
      return;
    }

    setAutomationValidationError(null);

    try {
      if (isEditingAutomation && editingAutomationId) {
        const updated = await updateAutomation(baseUrl, editingAutomationId, {
          name: automationName,
          prompt: automationPrompt,
          rrule: automationRrule,
          status: automationStatus,
          cwds: automationCwds,
        });
        setSelectedAutomationId(updated.id);
        setNotice("Automation updated");
        setErrorNotice(null);
        await refreshAutomations();
        await refreshAutomationRuns(updated.id);
      } else {
        const created = await createAutomation(baseUrl, {
          name: automationName,
          prompt: automationPrompt,
          rrule: automationRrule,
          status: automationStatus,
          cwds: automationCwds,
        });
        setSelectedAutomationId(created.id);
        setNotice("Automation created");
        setErrorNotice(null);
        await refreshAutomations();
        await refreshAutomationRuns(created.id);
      }
    } catch (error) {
      setErrorNotice(
        `Failed to ${isEditingAutomation ? "update" : "create"} automation: ${errorText(error)}`
      );
    }
  }, [
    automationCwds,
    automationName,
    automationPrompt,
    automationRrule,
    automationStatus,
    baseUrl,
    editingAutomationId,
    isEditingAutomation,
    refreshAutomationRuns,
    refreshAutomations,
  ]);

  const handleRunAutomation = useCallback(
    async (automationId: string) => {
      setAutomationBusyId(automationId);
      try {
        await runAutomation(baseUrl, automationId);
        setNotice("Automation run queued");
        setErrorNotice(null);
        await refreshAutomationRuns(automationId);
      } catch (error) {
        setErrorNotice(`Failed to run automation: ${errorText(error)}`);
      } finally {
        setAutomationBusyId(null);
      }
    },
    [baseUrl, refreshAutomationRuns]
  );

  const handleToggleAutomation = useCallback(
    async (automation: AutomationRecord) => {
      setAutomationBusyId(automation.id);
      try {
        if (automation.status === "active") {
          await pauseAutomation(baseUrl, automation.id);
        } else {
          await resumeAutomation(baseUrl, automation.id);
        }
        await refreshAutomations();
        setNotice("Automation updated");
        setErrorNotice(null);
      } catch (error) {
        setErrorNotice(`Failed to update automation: ${errorText(error)}`);
      } finally {
        setAutomationBusyId(null);
      }
    },
    [baseUrl, refreshAutomations]
  );

  const handleDeleteAutomation = useCallback(
    async (automation: AutomationRecord) => {
      setAutomationBusyId(automation.id);
      try {
        await deleteAutomation(baseUrl, automation.id);
        if (selectedAutomationId === automation.id) {
          setSelectedAutomationId(null);
          setAutomationRuns([]);
        }
        if (editingAutomationId === automation.id) {
          resetAutomationForm();
        }
        await refreshAutomations();
        setConfirmDeleteAutomationId(null);
        setNotice("Automation deleted");
        setErrorNotice(null);
      } catch (error) {
        setErrorNotice(`Failed to delete automation: ${errorText(error)}`);
      } finally {
        setAutomationBusyId(null);
      }
    },
    [baseUrl, editingAutomationId, refreshAutomations, resetAutomationForm, selectedAutomationId]
  );

  const openCommandPalette = useCallback(() => {
    setPaletteMode("commands");
    setPaletteQuery("");
    setPaletteOpen(true);
  }, []);

  const openSessionsPalette = useCallback(() => {
    setPaletteMode("sessions");
    setPaletteQuery("");
    setPaletteOpen(true);
    void refreshSessions();
  }, [refreshSessions]);

  const closePalette = useCallback(() => {
    setPaletteOpen(false);
  }, []);

  useKeyboardShortcuts({
    onOpenPalette: openCommandPalette,
    onOpenSessions: openSessionsPalette,
    onNewThread: () => {
      void handleCreateThread();
    },
    onEscape: () => {
      if (paletteOpen) {
        setPaletteOpen(false);
        return;
      }
      if (selectedTaskDetail) {
        setSelectedTaskDetail(null);
        return;
      }
      if (notice || errorNotice) {
        setNotice(null);
        setErrorNotice(null);
      }
    },
  });

  const commandItems = useMemo<CommandPaletteItem[]>(() => {
    if (paletteMode === "sessions") {
      return sessions.map((session) => ({
        id: session.id,
        label: session.title,
        description: `${session.model} · ${formatRelative(session.updated_at)} · ${session.message_count} messages`,
        action: () => {
          void (async () => {
            try {
              const result = await resumeSessionThread(baseUrl, session.id, { model, mode });
              setSection("chat");
              setSelectedThreadId(result.thread_id);
              setLiveEvents([]);
              lastSeq.current = 0;
              setNotice(result.summary);
              setErrorNotice(null);
              await refreshThreads();
            } catch (error) {
              setErrorNotice(`Failed to resume session: ${errorText(error)}`);
            }
          })();
        },
        secondaryAction: {
          label: "Delete",
          action: (e: React.MouseEvent) => {
            e.stopPropagation();
            void (async () => {
              try {
                await deleteSession(baseUrl, session.id);
                setNotice(`Deleted session "${session.title}"`);
                await refreshSessions();
              } catch (error) {
                setErrorNotice(`Failed to delete session: ${errorText(error)}`);
              }
            })();
          },
        },
      }));
    }

    const items: CommandPaletteItem[] = [
      {
        id: "new-thread",
        label: "New thread",
        description: "Start a fresh conversation thread",
        shortcut: "Ctrl/Cmd+N",
        action: () => {
          void handleCreateThread();
        },
      },
      {
        id: "open-chat",
        label: "Open chat",
        action: () => setSection("chat"),
      },
      {
        id: "open-automations",
        label: "Open automations",
        action: () => setSection("automations"),
      },
      {
        id: "open-skills",
        label: "Open skills & apps",
        action: () => setSection("skills"),
      },
      {
        id: "open-settings",
        label: "Open settings",
        action: () => setSection("settings"),
      },
      {
        id: "open-sessions",
        label: "Search sessions",
        shortcut: "Ctrl/Cmd+R",
        action: () => {
          openSessionsPalette();
        },
      },
    ];

    if (selectedThreadId) {
      items.push(
        {
          id: "resume-thread",
          label: "Resume current thread",
          action: () => {
            void handleResumeThread();
          },
        },
        {
          id: "fork-thread",
          label: "Fork current thread",
          action: () => {
            void handleForkThread();
          },
        },
        {
          id: "compact-thread",
          label: "Compact current thread",
          action: () => {
            void handleCompactThread();
          },
        }
      );
    }

    return items;
  }, [
    baseUrl,
    handleCompactThread,
    handleCreateThread,
    handleForkThread,
    handleResumeThread,
    mode,
    model,
    openSessionsPalette,
    paletteMode,
    refreshSessions,
    refreshThreads,
    selectedThreadId,
    sessions,
  ]);

  return (
    <div className="app-shell">
      <LeftRail
        section={section}
        onSectionChange={setSection}
        onNewThread={() => {
          void handleCreateThread();
        }}
        onOpenPalette={openCommandPalette}
        connectionState={connectionState}
        connectionMessage={connectionMessage}
      />

      <ThreadsPane
        threads={filteredThreads}
        selectedThreadId={selectedThreadId}
        threadSearch={threadSearch}
        threadFilter={threadFilter}
        onThreadSearchChange={setThreadSearch}
        onThreadFilterChange={(value) => {
          setThreadFilter(value);
          void refreshThreads(value);
        }}
        onThreadSelect={(id) => {
          setSection("chat");
          setSelectedThreadId(id);
          setLiveEvents([]);
          lastSeq.current = 0;
        }}
        onThreadArchiveToggle={(thread) => {
          void handleThreadArchiveToggle(thread);
        }}
      />

      <main className="main-pane">
        <TopBar
          workspace={workspace}
          model={model}
          mode={mode}
          modelOptions={MODEL_OPTIONS}
          modeOptions={MODE_OPTIONS}
          onModelChange={setModel}
          onModeChange={setMode}
        />

        {errorNotice ? (
          <div className="toast toast-error" role="alert" aria-live="assertive">
            <CircleX size={16} />
            <span>{errorNotice}</span>
            <button className="btn btn-ghost btn-sm" onClick={() => setErrorNotice(null)} aria-label="Dismiss error">
              <X size={12} />
            </button>
          </div>
        ) : null}

        {notice ? (
          <div className="toast toast-success" role="status" aria-live="polite">
            <CheckCircle2 size={16} />
            <span>{notice}</span>
            <button className="btn btn-ghost btn-sm" onClick={() => setNotice(null)} aria-label="Dismiss notice">
              <X size={12} />
            </button>
          </div>
        ) : null}

        <ConnectionBanner
          state={connectionState}
          message={connectionMessage}
          baseUrl={baseUrl}
          onRetryNow={() => {
            void retryNow();
          }}
          onOpenSettings={() => setSection("settings")}
        />

        {section === "chat" ? (
          <>
            <TranscriptPane transcript={transcript} selectedThreadId={selectedThreadId} />
            <LiveEventsPanel
              events={liveEvents}
              canResume={selectedThreadId != null}
              canFork={selectedThreadId != null}
              canInterrupt={selectedThreadId != null && activeTurnId != null}
              canCompact={selectedThreadId != null}
              steerText={steerText}
              onSteerTextChange={setSteerText}
              onResume={() => {
                void handleResumeThread();
              }}
              onFork={() => {
                void handleForkThread();
              }}
              onInterrupt={() => {
                void handleInterrupt();
              }}
              onCompact={() => {
                void handleCompactThread();
              }}
              onSteer={() => {
                void handleSteer();
              }}
            />
            <Composer
              value={composerText}
              onValueChange={setComposerText}
              onSend={() => {
                void handleSend();
              }}
              sending={sending}
              selectedThreadId={selectedThreadId}
              activeTurnId={activeTurnId}
            />
          </>
        ) : null}

        {section === "automations" ? (
          <div className="section-grid">
            <section className="panel-card">
              <div className="card-head">
                <h3>{isEditingAutomation ? "Edit Automation" : "Create Automation"}</h3>
                <button className="btn btn-ghost btn-sm" onClick={resetAutomationForm}>
                  Reset
                </button>
              </div>

              <label className="field-label">Name</label>
              <input value={automationName} onChange={(event) => setAutomationName(event.target.value)} />

              <label className="field-label">Prompt</label>
              <textarea
                rows={4}
                value={automationPrompt}
                onChange={(event) => setAutomationPrompt(event.target.value)}
              />

              <label className="field-label">Schedule builder</label>
              <div className="schedule-row">
                <select
                  value={schedulePreset}
                  onChange={(event) => setSchedulePreset(event.target.value as SchedulePreset)}
                >
                  <option value="weekly">Weekly</option>
                  <option value="hourly">Hourly interval</option>
                </select>
                <button className="btn btn-secondary" onClick={() => setAutomationRrule(automationDraftFromBuilder)}>
                  Use builder value
                </button>
              </div>

              {schedulePreset === "weekly" ? (
                <>
                  <div className="day-grid">
                    {WEEKDAY_OPTIONS.map((day) => (
                      <button
                        key={`weekly-${day}`}
                        className={`chip-button ${weeklyDays.includes(day) ? "is-selected" : ""}`}
                        onClick={() => toggleDay("weekly", day)}
                      >
                        {day}
                      </button>
                    ))}
                  </div>

                  <div className="schedule-row">
                    <input
                      type="number"
                      min={0}
                      max={23}
                      value={weeklyHour}
                      onChange={(event) => setWeeklyHour(Number(event.target.value))}
                      placeholder="Hour"
                    />
                    <input
                      type="number"
                      min={0}
                      max={59}
                      value={weeklyMinute}
                      onChange={(event) => setWeeklyMinute(Number(event.target.value))}
                      placeholder="Minute"
                    />
                  </div>
                </>
              ) : (
                <>
                  <div className="schedule-row">
                    <input
                      type="number"
                      min={1}
                      value={hourlyInterval}
                      onChange={(event) => setHourlyInterval(Number(event.target.value))}
                      placeholder="Interval (hours)"
                    />
                    <span className="subtle">Optional weekday filter</span>
                  </div>
                  <div className="day-grid">
                    {WEEKDAY_OPTIONS.map((day) => (
                      <button
                        key={`hourly-${day}`}
                        className={`chip-button ${hourlyDays.includes(day) ? "is-selected" : ""}`}
                        onClick={() => toggleDay("hourly", day)}
                      >
                        {day}
                      </button>
                    ))}
                  </div>
                </>
              )}

              <label className="field-label">RRULE</label>
              <input value={automationRrule} onChange={(event) => setAutomationRrule(event.target.value.toUpperCase())} />
              <div className="subtle">{humanRrule(automationRrule)}</div>

              <label className="field-label">Status</label>
              <select
                value={automationStatus}
                onChange={(event) => setAutomationStatus(event.target.value as "active" | "paused")}
              >
                <option value="active">Active</option>
                <option value="paused">Paused</option>
              </select>

              <label className="field-label">CWDs (local only)</label>
              <div className="cwd-row">
                <input
                  value={newCwdInput}
                  onChange={(event) => setNewCwdInput(event.target.value)}
                  placeholder="/path/to/workspace"
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      addCwdToForm();
                    }
                  }}
                />
                <button className="btn btn-secondary" onClick={addCwdToForm}>
                  <Plus size={14} />
                  Add
                </button>
              </div>

              <div className="chips-wrap">
                {automationCwds.length === 0 ? (
                  <div className="empty-state compact">No CWD restrictions.</div>
                ) : (
                  automationCwds.map((cwd) => (
                    <span key={cwd} className="cwd-chip">
                      {cwd}
                      <button aria-label={`Remove ${cwd}`} onClick={() => removeCwdFromForm(cwd)}>
                        <X size={12} />
                      </button>
                    </span>
                  ))
                )}
              </div>

              {automationValidationError ? (
                <div className="inline-error">
                  <AlertCircle size={14} />
                  <span>{automationValidationError}</span>
                </div>
              ) : null}

              <div className="inline-actions">
                <button className="btn btn-primary" onClick={() => void handleSaveAutomation()}>
                  {isEditingAutomation ? "Save changes" : "Create automation"}
                </button>
                {isEditingAutomation ? (
                  <button className="btn btn-ghost" onClick={resetAutomationForm}>
                    Cancel edit
                  </button>
                ) : null}
              </div>
            </section>

            <section className="panel-card">
              <div className="card-head">
                <h3>Automation list</h3>
                <button className="btn btn-ghost btn-sm" onClick={() => void refreshAutomations()}>
                  <RefreshCcw size={14} />
                  Refresh
                </button>
              </div>

              <div className="automation-list">
                {automations.length === 0 ? (
                  <div className="empty-state compact">No automations yet.</div>
                ) : (
                  automations.map((automation) => (
                    <article
                      key={automation.id}
                      className={`automation-card ${selectedAutomationId === automation.id ? "is-selected" : ""}`}
                    >
                      <button className="automation-main" onClick={() => setSelectedAutomationId(automation.id)}>
                        <div className="thread-header">
                          <strong>{automation.name}</strong>
                          <span className={`status-chip status-${automation.status}`}>{automation.status}</span>
                        </div>
                        <div className="thread-preview">{humanRrule(automation.rrule)}</div>
                        <div className="thread-meta">
                          <span>Next: {formatTimestamp(automation.next_run_at)}</span>
                          <span>Last: {formatTimestamp(automation.last_run_at)}</span>
                        </div>
                        <div className="subtle">{automation.cwds.length ? `${automation.cwds.length} cwd(s)` : "All workspaces"}</div>
                      </button>

                      <div className="inline-actions">
                        <button
                          className="btn btn-secondary btn-sm"
                          disabled={automationBusyId === automation.id}
                          onClick={() => void handleRunAutomation(automation.id)}
                        >
                          Run now
                        </button>
                        <button
                          className="btn btn-ghost btn-sm"
                          disabled={automationBusyId === automation.id}
                          onClick={() => void handleToggleAutomation(automation)}
                        >
                          {automation.status === "active" ? "Pause" : "Resume"}
                        </button>
                        <button
                          className="btn btn-ghost btn-sm"
                          disabled={automationBusyId === automation.id}
                          onClick={() => loadAutomationIntoForm(automation)}
                        >
                          Edit
                        </button>
                        {confirmDeleteAutomationId === automation.id ? (
                          <>
                            <button
                              className="btn btn-danger btn-sm"
                              disabled={automationBusyId === automation.id}
                              onClick={() => void handleDeleteAutomation(automation)}
                            >
                              Confirm delete
                            </button>
                            <button
                              className="btn btn-ghost btn-sm"
                              onClick={() => setConfirmDeleteAutomationId(null)}
                            >
                              Cancel
                            </button>
                          </>
                        ) : (
                          <button
                            className="btn btn-danger btn-sm"
                            disabled={automationBusyId === automation.id}
                            onClick={() => setConfirmDeleteAutomationId(automation.id)}
                          >
                            <Trash2 size={14} />
                            Delete
                          </button>
                        )}
                      </div>
                    </article>
                  ))
                )}
              </div>
            </section>

            <section className="panel-card">
              <div className="card-head">
                <h3>Run history</h3>
                {currentAutomation ? (
                  <button className="btn btn-ghost btn-sm" onClick={() => void refreshAutomationRuns(currentAutomation.id)}>
                    <RefreshCcw size={14} />
                    Refresh
                  </button>
                ) : null}
              </div>

              {!currentAutomation ? (
                <div className="empty-state compact">Select an automation to view runs.</div>
              ) : automationRuns.length === 0 ? (
                <div className="empty-state compact">No runs found for this automation.</div>
              ) : (
                <div className="run-list">
                  {automationRuns.map((run) => (
                    <article key={run.id} className="run-card">
                      <div className="thread-header">
                        <span className={`status-chip status-${run.status}`}>{run.status}</span>
                        <span>{formatTimestamp(run.created_at)}</span>
                      </div>
                      <div className="subtle">Task: {run.task_id ?? "-"}</div>
                      <div className="subtle">Thread: {run.thread_id ?? "-"}</div>
                      <div className="subtle">Turn: {run.turn_id ?? "-"}</div>
                      {run.thread_id ? (
                        <button
                          className="btn btn-ghost btn-sm"
                          onClick={() => {
                            setSection("chat");
                            setSelectedThreadId(run.thread_id ?? null);
                          }}
                        >
                          <ChevronRight size={14} />
                          Open thread
                        </button>
                      ) : null}
                      {run.error ? <div className="inline-error">{run.error}</div> : null}
                    </article>
                  ))}
                </div>
              )}
            </section>
          </div>
        ) : null}

        {section === "skills" ? (
          <div className="section-grid">
            <section className="panel-card">
              <div className="card-head">
                <h3>Skills</h3>
                <div className="subtle">Directory: {skills?.directory ?? "-"}</div>
              </div>

              <label className="search-field">
                <Search size={14} />
                <input
                  value={skillsSearch}
                  onChange={(event) => setSkillsSearch(event.target.value)}
                  placeholder="Search skills"
                />
              </label>

              {skills?.warnings?.length ? (
                <div className="warning-box">{skills.warnings.join("\n")}</div>
              ) : null}

              <div className="skill-list">
                {filteredSkills.length === 0 ? (
                  <div className="empty-state compact">No skills discovered.</div>
                ) : (
                  filteredSkills.map((skill) => (
                    <article key={skill.name} className="skill-card">
                      <div className="thread-header">
                        <strong>{skill.name}</strong>
                      </div>
                      <div>{skill.description || "No description"}</div>
                      <code>{skill.path}</code>
                    </article>
                  ))
                )}
              </div>
            </section>

            <section className="panel-card">
              <div className="card-head">
                <h3>MCP servers</h3>
                <button className="btn btn-ghost btn-sm" onClick={() => void refreshSkillsAndApps()}>
                  <RefreshCcw size={14} />
                  Refresh
                </button>
              </div>

              <div className="server-list">
                {mcpServers.length === 0 ? (
                  <div className="empty-state compact">No MCP servers configured.</div>
                ) : (
                  mcpServers.map((server) => (
                    <article key={server.name} className="server-card">
                      <div className="thread-header">
                        <strong>{server.name}</strong>
                        <span className={`status-chip status-${server.connected ? "connected" : "disconnected"}`}>
                          {server.connected ? "connected" : "disconnected"}
                        </span>
                      </div>
                      <div className="subtle">Enabled: {String(server.enabled)}</div>
                      <div className="subtle">Required: {String(server.required)}</div>
                      <div className="subtle">{server.url ?? server.command ?? "No transport configured"}</div>
                    </article>
                  ))
                )}
              </div>
            </section>

            <section className="panel-card">
              <div className="card-head">
                <h3>MCP tools</h3>
                <div className="inline-actions">
                  <select value={serverFilter} onChange={(event) => setServerFilter(event.target.value)}>
                    <option value="all">All servers</option>
                    {mcpServers.map((server) => (
                      <option key={server.name} value={server.name}>
                        {server.name}
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              <label className="search-field">
                <Search size={14} />
                <input
                  value={toolsSearch}
                  onChange={(event) => setToolsSearch(event.target.value)}
                  placeholder="Search tools"
                />
              </label>

              <div className="tool-list">
                {filteredTools.length === 0 ? (
                  <div className="empty-state compact">No MCP tools found.</div>
                ) : (
                  filteredTools.map((tool) => (
                    <article key={tool.prefixed_name} className="tool-card">
                      <strong>{tool.prefixed_name}</strong>
                      <div>{tool.description || "No description"}</div>
                    </article>
                  ))
                )}
              </div>
            </section>
          </div>
        ) : null}

        {section === "settings" ? (
          <div className="section-grid settings-grid">
            <section className="panel-card">
              <div className="card-head">
                <h3>Runtime endpoint</h3>
              </div>
              <p className="subtle">
                Local runtime API endpoint. Desktop bootstrap remains non-fatal and this app stays usable when
                runtime is offline.
              </p>
              <input value={baseUrlInput} onChange={(event) => setBaseUrlInput(event.target.value)} />
              <div className="inline-actions">
                <button className="btn btn-primary" onClick={() => void applyRuntimeBaseUrl()}>
                  Apply
                </button>
                <button
                  className="btn btn-ghost"
                  onClick={() => {
                    setBaseUrlInput(DEFAULT_RUNTIME_BASE_URL);
                    setBaseUrl(DEFAULT_RUNTIME_BASE_URL);
                    persistRuntimeBaseUrl(DEFAULT_RUNTIME_BASE_URL);
                  }}
                >
                  Reset default
                </button>
                <button className="btn btn-ghost" onClick={() => void retryNow()}>
                  Check health now
                </button>
              </div>
            </section>

            <section className="panel-card">
              <div className="card-head">
                <h3>Workspace status</h3>
              </div>
              {workspace ? (
                <div className="meta-grid">
                  <div>Path: {workspace.workspace}</div>
                  <div>Branch: {workspace.branch ?? "-"}</div>
                  <div>Staged: {workspace.staged}</div>
                  <div>Unstaged: {workspace.unstaged}</div>
                  <div>Untracked: {workspace.untracked}</div>
                  <div>
                    Ahead / Behind: {workspace.ahead ?? 0} / {workspace.behind ?? 0}
                  </div>
                </div>
              ) : (
                <div className="empty-state compact">Unavailable.</div>
              )}
            </section>

            <section className="panel-card">
              <div className="card-head">
                <h3>Tasks</h3>
                <button className="btn btn-ghost btn-sm" onClick={() => void refreshTasks()}>
                  <RefreshCcw size={14} />
                  Refresh
                </button>
              </div>

              {tasks.length === 0 ? (
                <div className="empty-state compact">No tasks found.</div>
              ) : (
                <div className="task-list">
                  {tasks.map((task) => (
                    <button
                      key={task.id}
                      className={`task-row task-row-clickable ${selectedTaskDetail?.id === task.id ? "is-selected" : ""}`}
                      onClick={() => void openTaskDetail(task.id)}
                    >
                      <span className={`status-chip status-${task.status}`}>{task.status}</span>
                      <div>{task.prompt_summary}</div>
                      <div className="subtle">{formatTimestamp(task.created_at)}</div>
                    </button>
                  ))}
                </div>
              )}
            </section>

            <TaskDetailPanel
              task={selectedTaskDetail}
              loading={taskDetailLoading}
              onClose={() => setSelectedTaskDetail(null)}
              onOpenThread={(threadId) => {
                setSection("chat");
                setSelectedThreadId(threadId);
                setLiveEvents([]);
                lastSeq.current = 0;
              }}
            />

            <section className="panel-card">
              <div className="card-head">
                <h3>Keyboard shortcuts</h3>
              </div>
              <div className="shortcut-list">
                <div>
                  <kbd>Ctrl/Cmd+K</kbd> Open command palette
                </div>
                <div>
                  <kbd>Ctrl/Cmd+R</kbd> Search sessions
                </div>
                <div>
                  <kbd>Ctrl/Cmd+N</kbd> New thread
                </div>
                <div>
                  <kbd>Enter</kbd> Send composer input
                </div>
                <div>
                  <kbd>Shift+Enter</kbd> New line
                </div>
                <div>
                  <kbd>Alt+Enter</kbd> New line
                </div>
                <div>
                  <kbd>Ctrl/Cmd+J</kbd> New line
                </div>
                <div>
                  <kbd>Esc</kbd> Close overlays / clear notices
                </div>
              </div>
            </section>
          </div>
        ) : null}
      </main>

      <CommandPalette
        open={paletteOpen}
        title={paletteMode === "sessions" ? "Recent Sessions" : "Command Palette"}
        items={commandItems}
        query={paletteQuery}
        onQueryChange={setPaletteQuery}
        onClose={closePalette}
      />
    </div>
  );
}
