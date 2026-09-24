import { invoke } from "@tauri-apps/api/core";
import type {
  Device,
  GhRepo,
  GhRun,
  GhUser,
  GhWorkflow,
  ProjectInfo,
  ShareInfo,
  StepSpec,
  SystemInfo,
  ToolStatus,
  VersionConstraint,
} from "./types";

export const api = {
  configLoad: () => invoke<unknown>("config_load"),
  configSave: (data: unknown) => invoke<void>("config_save", { data }),
  pathExists: (path: string) => invoke<boolean>("path_exists", { path }),
  writeTextFile: (path: string, content: string) => invoke<void>("write_text_file", { path, content }),

  detectProject: (path: string) => invoke<ProjectInfo>("detect_project", { path }),
  listDevices: (includeNetwork: boolean) => invoke<Device[]>("list_devices", { includeNetwork }),
  checkTools: (ids: string[], constraints: VersionConstraint[]) =>
    invoke<ToolStatus[]>("check_tools", { ids, constraints }),
  systemInfo: () => invoke<SystemInfo>("system_info"),

  jobStart: (req: {
    jobId: string;
    title: string;
    baseDir?: string;
    steps: StepSpec[];
    env?: Record<string, string>;
    cols?: number;
    rows?: number;
  }) => invoke<void>("job_start", { req }),
  jobInput: (jobId: string, data: string) => invoke<void>("job_input", { jobId, data }),
  jobResize: (jobId: string, cols: number, rows: number) => invoke<void>("job_resize", { jobId, cols, rows }),
  jobCancel: (jobId: string) => invoke<void>("job_cancel", { jobId }),

  shareStart: (path: string, title?: string) => invoke<ShareInfo>("share_start", { path, title }),
  shareStop: (id: string) => invoke<void>("share_stop", { id }),
  shareList: () => invoke<ShareInfo[]>("share_list"),

  secretSet: (key: string, value: string) => invoke<void>("secret_set", { key, value }),
  secretHas: (key: string) => invoke<boolean>("secret_has", { key }),
  secretDelete: (key: string) => invoke<void>("secret_delete", { key }),

  ghConnectToken: (token: string) => invoke<GhUser>("gh_connect_token", { token }),
  ghImportCli: () => invoke<GhUser>("gh_import_cli"),
  ghStatus: () => invoke<GhUser | null>("gh_status"),
  ghLogout: () => invoke<void>("gh_logout"),
  ghRepos: () => invoke<GhRepo[]>("gh_repos"),
  ghWorkflows: (owner: string, repo: string) => invoke<GhWorkflow[]>("gh_workflows", { owner, repo }),
  ghRuns: (owner: string, repo: string) => invoke<GhRun[]>("gh_runs", { owner, repo }),
  ghDispatch: (owner: string, repo: string, workflow: string, gitRef: string) =>
    invoke<void>("gh_dispatch", { owner, repo, workflow, gitRef }),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}

export const sshSecretKey = (targetId: string) => `ssh-pw:${targetId}`;
