import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api } from "./api";
import type { AppData, Profile, Project, ProjectInfo, SystemInfo, Target } from "./types";
import { uid } from "./util";

export const BUILTIN_TARGETS: Target[] = [
  { id: "local", name: "Dieser PC", kind: "local", builtin: true },
  { id: "share", name: "Download im Netzwerk (QR)", kind: "share", builtin: true },
];

const EMPTY: AppData = { version: 1, projects: [], targets: [], profiles: [] };

export type Route =
  | { page: "projects" }
  | { page: "project"; id: string }
  | { page: "targets" }
  | { page: "requirements" }
  | { page: "github" }
  | { page: "shares" };

interface Store {
  data: AppData;
  loaded: boolean;
  sys: SystemInfo | null;
  route: Route;
  navigate: (r: Route) => void;
  targets: Target[];
  addProject: (info: ProjectInfo) => Project;
  updateProject: (id: string, info: ProjectInfo) => void;
  removeProject: (id: string) => void;
  upsertTarget: (t: Target) => void;
  removeTarget: (id: string) => void;
  upsertProfile: (p: Profile) => void;
  removeProfile: (id: string) => void;
  markRun: (profileId: string, success: boolean) => void;
}

const Ctx = createContext<Store | null>(null);

export function StoreProvider({ children }: { children: ReactNode }) {
  const [data, setData] = useState<AppData>(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const [route, setRoute] = useState<Route>({ page: "projects" });
  const saveTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    api.systemInfo().then(setSys).catch(() => {});
    api
      .configLoad()
      .then((raw) => {
        const d = raw as Partial<AppData> | null;
        if (d && Array.isArray(d.projects)) setData({ ...EMPTY, ...d, version: 1 } as AppData);
      })
      .catch((e) => console.error(e))
      .finally(() => setLoaded(true));
  }, []);

  useEffect(() => {
    if (!loaded) return;
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      api.configSave(data).catch((e) => console.error("Speichern fehlgeschlagen", e));
    }, 300);
  }, [data, loaded]);

  const addProject = useCallback((info: ProjectInfo) => {
    const p: Project = { ...info, id: uid(), addedAt: Date.now() };
    setData((d) => ({ ...d, projects: [p, ...d.projects] }));
    return p;
  }, []);

  const updateProject = useCallback((id: string, info: ProjectInfo) => {
    setData((d) => ({ ...d, projects: d.projects.map((p) => (p.id === id ? { ...p, ...info } : p)) }));
  }, []);

  const removeProject = useCallback((id: string) => {
    setData((d) => ({
      ...d,
      projects: d.projects.filter((p) => p.id !== id),
      profiles: d.profiles.filter((p) => p.projectId !== id),
    }));
  }, []);

  const upsertTarget = useCallback((t: Target) => {
    setData((d) => {
      const exists = d.targets.some((x) => x.id === t.id);
      return { ...d, targets: exists ? d.targets.map((x) => (x.id === t.id ? t : x)) : [...d.targets, t] };
    });
  }, []);

  const removeTarget = useCallback((id: string) => {
    setData((d) => ({
      ...d,
      targets: d.targets.filter((t) => t.id !== id),
      profiles: d.profiles.filter((p) => p.targetId !== id),
    }));
  }, []);

  const upsertProfile = useCallback((p: Profile) => {
    setData((d) => {
      const exists = d.profiles.some((x) => x.id === p.id);
      return { ...d, profiles: exists ? d.profiles.map((x) => (x.id === p.id ? p : x)) : [...d.profiles, p] };
    });
  }, []);

  const removeProfile = useCallback((id: string) => {
    setData((d) => ({ ...d, profiles: d.profiles.filter((p) => p.id !== id) }));
  }, []);

  const markRun = useCallback((profileId: string, success: boolean) => {
    setData((d) => ({
      ...d,
      profiles: d.profiles.map((p) => (p.id === profileId ? { ...p, lastRun: { at: Date.now(), success } } : p)),
    }));
  }, []);

  const targets = useMemo(() => [...BUILTIN_TARGETS, ...data.targets], [data.targets]);

  const value: Store = {
    data,
    loaded,
    sys,
    route,
    navigate: setRoute,
    targets,
    addProject,
    updateProject,
    removeProject,
    upsertTarget,
    removeTarget,
    upsertProfile,
    removeProfile,
    markRun,
  };
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useStore(): Store {
  const s = useContext(Ctx);
  if (!s) throw new Error("StoreProvider fehlt");
  return s;
}
