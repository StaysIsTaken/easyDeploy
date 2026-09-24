import { sshSecretKey } from "./api";
import type { Profile, Project, SshConn, StepSpec, Target } from "./types";
import { fill, safeName } from "./util";

export function sshConn(target: Target): SshConn {
  const s = target.ssh;
  if (!s) throw new Error(`„${target.name}“ ist kein SSH-Ziel`);
  return {
    host: s.host,
    port: s.port || 22,
    user: s.user || undefined,
    keyPath: s.auth === "key" && s.keyPath ? s.keyPath : undefined,
    passwordSecret: s.auth === "password" ? sshSecretKey(target.id) : undefined,
  };
}

const SAFE_VALUE = /^[A-Za-z0-9._:@%\[\]-]*$/;

/** Device ids and host names are substituted into shell commands. */
function checked(name: string, value: string): string {
  if (!SAFE_VALUE.test(value) || value.startsWith("-")) {
    throw new Error(`Ungültiger Wert für ${name}: „${value}“`);
  }
  return value;
}

export function profileVars(profile: Profile, project: Project, target: Target): Record<string, string> {
  const v: Record<string, string> = {
    projectDir: project.path,
    projectName: safeName(project.name),
  };
  if (target.device) v.device = checked("Gerät", target.device.id);
  if (target.ssh) {
    v.host = checked("Host", target.ssh.host);
    v.user = checked("Benutzer", target.ssh.user);
    v.port = String(Number(target.ssh.port) || 22);
  }
  if (target.folder) v.dest = target.folder.path;
  return { ...v, ...profile.vars };
}

/** Converts a saved profile into concrete steps for the Rust job runner. */
export function resolveSteps(profile: Profile, project: Project, target: Target): StepSpec[] {
  const vars = profileVars(profile, project, target);
  const f = (s?: string) => fill(s ?? "", vars);
  return profile.steps.map((s): StepSpec => {
    switch (s.kind) {
      case "shell":
        return { kind: "shell", name: s.name, command: f(s.command), cwd: s.cwd ? f(s.cwd) : undefined, allowFailure: s.allowFailure };
      case "upload":
        return {
          kind: "upload",
          name: s.name,
          source: f(s.source) || ".",
          remotePath: f(s.remotePath),
          ssh: sshConn(target),
          excludes: s.excludes,
          clean: s.clean,
        };
      case "remote":
        return { kind: "remote", name: s.name, command: f(s.command), ssh: sshConn(target) };
      case "copy":
        return { kind: "copy", name: s.name, source: f(s.source), dest: f(s.dest) };
      case "share":
        return { kind: "share", name: s.name, source: f(s.source), title: `${project.name} · ${profile.name}` };
      case "open":
        return { kind: "open", name: s.name, target: f(s.target) };
      case "ghDispatch": {
        const g = target.github;
        if (!g?.repo || !g.workflow) throw new Error("Für das GitHub-Ziel sind Repository und Workflow nicht gesetzt.");
        return { kind: "ghDispatch", name: s.name, owner: g.owner, repo: g.repo, workflow: g.workflow, gitRef: g.ref || "main" };
      }
    }
  });
}

/** Placeholders that are still unresolved, e.g. {{device}} without a device. */
export function missingVars(profile: Profile, project: Project, target: Target): string[] {
  let vars: Record<string, string>;
  try {
    vars = profileVars(profile, project, target);
  } catch {
    return [];
  }
  const missing = new Set<string>();
  const scan = (s?: string) => {
    for (const m of (s ?? "").matchAll(/\{\{\s*([\w.-]+)\s*\}\}/g)) {
      if (!(m[1] in vars) || vars[m[1]] === "") missing.add(m[1]);
    }
  };
  profile.steps.forEach((s) => [s.command, s.cwd, s.source, s.remotePath, s.dest, s.target].forEach(scan));
  return [...missing];
}
