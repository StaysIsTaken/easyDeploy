export type ProjectKind = "flutter" | "react-native" | "tauri" | "node" | "python" | "docker" | "dart";

export interface DetectedType {
  kind: ProjectKind;
  label: string;
  details: Record<string, string>;
}

export interface VersionConstraint {
  tool: string;
  constraint: string;
  mode: "exact" | "range";
  source: string;
}

export interface GitInfo {
  branch?: string | null;
  remote?: string | null;
  github?: string | null;
}

export interface ProjectInfo {
  path: string;
  name: string;
  types: DetectedType[];
  constraints: VersionConstraint[];
  git?: GitInfo | null;
}

export interface Project extends ProjectInfo {
  id: string;
  addedAt: number;
}

export type TargetKind = "ssh" | "android" | "ios" | "local" | "folder" | "share" | "github";

export interface SshConfig {
  host: string;
  port: number;
  user: string;
  auth: "key" | "password" | "agent";
  keyPath?: string;
}

export interface Target {
  id: string;
  name: string;
  kind: TargetKind;
  builtin?: boolean;
  ssh?: SshConfig;
  device?: { id: string; model?: string };
  folder?: { path: string };
  github?: { owner: string; repo: string; workflow: string; ref: string };
}

export type StepKind = "shell" | "upload" | "remote" | "copy" | "share" | "open" | "ghDispatch";

export interface Step {
  id: string;
  kind: StepKind;
  name: string;
  command?: string;
  cwd?: string;
  /** Shell steps only: continue the pipeline even if this step fails. */
  allowFailure?: boolean;
  source?: string;
  remotePath?: string;
  excludes?: string[];
  clean?: boolean;
  dest?: string;
  target?: string;
}

export interface Profile {
  id: string;
  projectId: string;
  targetId: string;
  name: string;
  kind: ProjectKind;
  steps: Step[];
  vars: Record<string, string>;
  tools: string[];
  notes?: string[];
  lastRun?: { at: number; success: boolean };
}

export interface AppData {
  version: 1;
  projects: Project[];
  targets: Target[];
  profiles: Profile[];
}

// ---- Backend responses ------------------------------------------------------

export interface Device {
  id: string;
  name: string;
  kind: "android" | "ios" | "network" | "ssh-config";
  connection: string;
  state: string;
  hint?: string | null;
  details: Record<string, string>;
}

export interface ToolAction {
  label: string;
  command?: string | null;
  url?: string | null;
  confirm?: string | null;
  inProject: boolean;
}

export interface ToolStatus {
  id: string;
  name: string;
  installed: boolean;
  version?: string | null;
  /** Where the executable was found. */
  path?: string | null;
  ok: boolean;
  required?: string | null;
  requiredSource?: string | null;
  message?: string | null;
  actions: ToolAction[];
  docsUrl?: string | null;
  optional: boolean;
}

export interface SystemInfo {
  os: "macos" | "windows" | "linux" | string;
  arch: string;
  hostname: string;
  home: string;
  linuxPackageManager?: string | null;
  androidHome?: string | null;
}

export interface ShareInfo {
  id: string;
  title: string;
  path: string;
  port: number;
  urls: { url: string; interface: string; qrSvg: string }[];
  files: { name: string; size: number }[];
  startedAt: number;
}

export interface GhUser {
  login: string;
  name?: string | null;
  avatarUrl: string;
}

export interface GhRepo {
  fullName: string;
  owner: string;
  name: string;
  private: boolean;
  description?: string | null;
  cloneUrl: string;
  sshUrl: string;
  htmlUrl: string;
  defaultBranch: string;
  language?: string | null;
  updatedAt: string;
}

export interface GhWorkflow {
  id: number;
  name: string;
  path: string;
  state: string;
  file: string;
}

export interface GhRun {
  id: number;
  name: string;
  title: string;
  status: string;
  conclusion?: string | null;
  branch: string;
  event: string;
  htmlUrl: string;
  createdAt: string;
}

// ---- Jobs -------------------------------------------------------------------

/** Step payload sent to the Rust job runner. */
export type StepSpec =
  | { kind: "shell"; name: string; command: string; cwd?: string; env?: Record<string, string>; allowFailure?: boolean }
  | { kind: "upload"; name: string; source: string; remotePath: string; ssh: SshConn; excludes?: string[]; clean?: boolean }
  | { kind: "remote"; name: string; command: string; ssh: SshConn }
  | { kind: "copy"; name: string; source: string; dest: string }
  | { kind: "share"; name: string; source: string; title?: string }
  | { kind: "open"; name: string; target: string }
  | { kind: "gitClone"; name: string; url: string; dest: string }
  | { kind: "ghDispatch"; name: string; owner: string; repo: string; workflow: string; gitRef: string; inputs?: Record<string, string> };

export interface SshConn {
  host: string;
  port?: number;
  user?: string;
  keyPath?: string;
  passwordSecret?: string;
}

export interface PromptOption {
  label: string;
  value: string;
  primary: boolean;
}

export interface JobPrompt {
  jobId: string;
  promptId: number;
  kind: "secret" | "hostkey" | "license" | "confirm";
  message: string;
  context: string;
  options: PromptOption[];
}

export type StepStatus = "pending" | "running" | "success" | "failed" | "skipped";
