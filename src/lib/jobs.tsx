import { listen } from "@tauri-apps/api/event";
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { api, errorText } from "./api";
import type { JobPrompt, StepSpec, StepStatus } from "./types";
import { uid } from "./util";

export interface JobState {
  id: string;
  title: string;
  kind: "deploy" | "install" | "task";
  profileId?: string;
  baseDir?: string;
  stepNames: string[];
  stepStatus: StepStatus[];
  status: "running" | "success" | "failed" | "cancelled";
  startedAt: number;
  finishedAt?: number;
  prompts: JobPrompt[];
  error?: string;
}

export interface RunOptions {
  title: string;
  kind?: JobState["kind"];
  profileId?: string;
  baseDir?: string;
  steps: StepSpec[];
  env?: Record<string, string>;
  /** Open the run panel immediately (default true). */
  show?: boolean;
}

interface Jobs {
  jobs: JobState[];
  activeId: string | null;
  panelOpen: boolean;
  openPanel: (id?: string) => void;
  closePanel: () => void;
  run: (o: RunOptions) => Promise<{ id: string; success: boolean }>;
  input: (id: string, data: string) => void;
  cancel: (id: string) => void;
  answerPrompt: (p: JobPrompt, value: string | null) => void;
  subscribe: (id: string, cb: (chunk: string) => void) => () => void;
  buffer: (id: string) => string[];
  clearFinished: () => void;
}

const Ctx = createContext<Jobs | null>(null);
const MAX_BUFFER_CHUNKS = 20000;

export function JobsProvider({ children }: { children: ReactNode }) {
  const [jobs, setJobs] = useState<JobState[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [panelOpen, setPanelOpen] = useState(false);
  const buffers = useRef(new Map<string, string[]>());
  const subs = useRef(new Map<string, Set<(c: string) => void>>());
  const resolvers = useRef(new Map<string, (r: { id: string; success: boolean }) => void>());

  const patch = useCallback((id: string, fn: (j: JobState) => JobState) => {
    setJobs((js) => js.map((j) => (j.id === id ? fn(j) : j)));
  }, []);

  useEffect(() => {
    let disposed = false;
    const unlisteners: (() => void)[] = [];
    const add = <T,>(event: string, cb: (payload: T) => void) =>
      listen<T>(event, (e) => cb(e.payload)).then((u) => (disposed ? u() : unlisteners.push(u)));

    add<{ jobId: string; data: string }>("job://output", ({ jobId, data }) => {
      const buf = buffers.current.get(jobId) ?? [];
      buf.push(data);
      if (buf.length > MAX_BUFFER_CHUNKS) buf.splice(0, buf.length - MAX_BUFFER_CHUNKS);
      buffers.current.set(jobId, buf);
      subs.current.get(jobId)?.forEach((cb) => cb(data));
    });
    add<{ jobId: string; index: number; status: StepStatus }>("job://step", ({ jobId, index, status }) => {
      patch(jobId, (j) => {
        const s = [...j.stepStatus];
        s[index] = status;
        return { ...j, stepStatus: s };
      });
    });
    add<JobPrompt>("job://prompt", (p) => {
      patch(p.jobId, (j) => ({ ...j, prompts: [...j.prompts, p] }));
      setActiveId(p.jobId);
      setPanelOpen(true);
    });
    add<{ jobId: string; success: boolean; cancelled: boolean; error?: string }>("job://done", (e) => {
      patch(e.jobId, (j) => ({
        ...j,
        status: e.cancelled ? "cancelled" : e.success ? "success" : "failed",
        finishedAt: Date.now(),
        prompts: [],
        error: e.error ?? undefined,
      }));
      resolvers.current.get(e.jobId)?.({ id: e.jobId, success: e.success });
      resolvers.current.delete(e.jobId);
    });
    return () => {
      disposed = true;
      unlisteners.forEach((u) => u());
    };
  }, [patch]);

  const run = useCallback(
    (o: RunOptions) => {
      const id = uid();
      const job: JobState = {
        id,
        title: o.title,
        kind: o.kind ?? "task",
        profileId: o.profileId,
        baseDir: o.baseDir,
        stepNames: o.steps.map((s) => s.name),
        stepStatus: o.steps.map(() => "pending"),
        status: "running",
        startedAt: Date.now(),
        prompts: [],
      };
      buffers.current.set(id, []);
      setJobs((js) => [job, ...js].slice(0, 30));
      if (o.show !== false) {
        setActiveId(id);
        setPanelOpen(true);
      }
      return new Promise<{ id: string; success: boolean }>((resolve) => {
        resolvers.current.set(id, resolve);
        api
          .jobStart({ jobId: id, title: o.title, baseDir: o.baseDir, steps: o.steps, env: o.env, cols: 110, rows: 30 })
          .catch((e) => {
            patch(id, (j) => ({ ...j, status: "failed", error: errorText(e), finishedAt: Date.now() }));
            resolvers.current.delete(id);
            resolve({ id, success: false });
          });
      });
    },
    [patch],
  );

  const input = useCallback((id: string, data: string) => {
    api.jobInput(id, data).catch(() => {});
  }, []);

  const cancel = useCallback((id: string) => {
    api.jobCancel(id).catch(() => {});
  }, []);

  const answerPrompt = useCallback(
    (p: JobPrompt, value: string | null) => {
      if (value !== null) api.jobInput(p.jobId, value).catch(() => {});
      patch(p.jobId, (j) => ({ ...j, prompts: j.prompts.filter((x) => x.promptId !== p.promptId) }));
    },
    [patch],
  );

  const subscribe = useCallback((id: string, cb: (c: string) => void) => {
    const set = subs.current.get(id) ?? new Set();
    set.add(cb);
    subs.current.set(id, set);
    return () => {
      set.delete(cb);
    };
  }, []);

  const buffer = useCallback((id: string) => buffers.current.get(id) ?? [], []);

  const clearFinished = useCallback(() => {
    setJobs((js) => {
      js.filter((j) => j.status !== "running").forEach((j) => buffers.current.delete(j.id));
      return js.filter((j) => j.status === "running");
    });
  }, []);

  const value: Jobs = {
    jobs,
    activeId,
    panelOpen,
    openPanel: (id) => {
      if (id) setActiveId(id);
      setPanelOpen(true);
    },
    closePanel: () => setPanelOpen(false),
    run,
    input,
    cancel,
    answerPrompt,
    subscribe,
    buffer,
    clearFinished,
  };
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useJobs(): Jobs {
  const j = useContext(Ctx);
  if (!j) throw new Error("JobsProvider fehlt");
  return j;
}
