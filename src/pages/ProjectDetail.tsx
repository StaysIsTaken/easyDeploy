import { confirm } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { ArrowLeft, FileCode2, FolderOpen, Pencil, Plus, RefreshCw, Rocket, Trash2, Upload } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useDeploy } from "../components/DeployFlow";
import { GithubIcon, KindIcon, TargetIcon } from "../components/icons";
import { ProfileEditor } from "../components/ProfileEditor";
import { ProfileWizard } from "../components/ProfileWizard";
import { RequirementsList } from "../components/RequirementsList";
import { Badge, Button, Empty, ErrorNote, Modal } from "../components/ui";
import { api, errorText } from "../lib/api";
import { useJobs } from "../lib/jobs";
import { baseTools, TARGET_LABEL } from "../lib/recipes";
import { useStore } from "../lib/store";
import type { Profile, Project, ToolStatus } from "../lib/types";
import { joinPath, timeAgo } from "../lib/util";
import { generateWorkflow, workflowFileName } from "../lib/workflow";

function WorkflowCard({ project }: { project: Project }) {
  const { run } = useJobs();
  const [written, setWritten] = useState<string | null>(null);
  const [error, setError] = useState("");
  const repo = project.git?.github;

  const write = async (kind: string) => {
    const type = project.types.find((t) => t.kind === kind);
    if (!type) return;
    setError("");
    const rel = `.github/workflows/${workflowFileName(type)}`;
    const path = joinPath(project.path, rel);
    try {
      if (await api.pathExists(path)) {
        if (!(await confirm(`${rel} existiert bereits. Überschreiben?`, { title: "Workflow", kind: "warning" }))) return;
      }
      await api.writeTextFile(path, generateWorkflow(project, type));
      setWritten(rel);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const push = () => {
    if (!written) return;
    run({
      title: "Workflow committen & pushen",
      baseDir: project.path,
      steps: [
        { kind: "shell", name: "Committen", command: `git add "${written}" && git commit -m "Add easyDeploy workflow"` },
        { kind: "shell", name: "Pushen", command: "git push" },
      ],
    });
  };

  return (
    <section className="card">
      <div className="row between">
        <h3 className="row gap">
          <GithubIcon size={18} /> GitHub Actions
        </h3>
        {repo && (
          <Button size="sm" variant="ghost" onClick={() => openUrl(`https://github.com/${repo}/actions`)}>
            {repo}
          </Button>
        )}
      </div>
      <p className="small muted">
        Erzeugt eine Workflow-Datei, die das Projekt in GitHub Actions baut (Trigger: manuell per „workflow_dispatch“). Danach kannst du ein
        GitHub-Ziel anlegen und Builds per Klick starten.
      </p>
      <div className="row gap wrap">
        {project.types.map((t) => (
          <Button key={t.kind} size="sm" icon={<FileCode2 size={14} />} onClick={() => write(t.kind)}>
            Workflow für {t.label}
          </Button>
        ))}
      </div>
      {written && (
        <div className="note note-ok small">
          <span>
            <span className="mono">{written}</span> wurde erstellt.
          </span>
          {repo && (
            <Button size="sm" icon={<Upload size={14} />} onClick={push}>
              Committen & pushen
            </Button>
          )}
        </div>
      )}
      {!repo && <p className="small muted">Kein GitHub-Remote erkannt (origin).</p>}
      <ErrorNote>{error}</ErrorNote>
    </section>
  );
}

function ProfileCard({ profile, onEdit }: { profile: Profile; onEdit: () => void }) {
  const { targets, removeProfile } = useStore();
  const { deploy } = useDeploy();
  const { jobs } = useJobs();
  const target = targets.find((t) => t.id === profile.targetId);
  const running = jobs.some((j) => j.status === "running" && j.profileId === profile.id);

  return (
    <article className="card profile-card">
      <div className="row gap">
        <div className="kind-icon">
          <TargetIcon kind={target?.kind ?? "local"} size={20} />
        </div>
        <div className="grow min0">
          <h3 className="ellipsis">{profile.name}</h3>
          <p className="small muted">
            {target ? `${target.name} · ${TARGET_LABEL[target.kind]}` : "Ziel wurde gelöscht"} · {profile.steps.length} Schritte
          </p>
        </div>
      </div>
      <div className="row between">
        <span className="small muted">
          {profile.lastRun ? (
            <>
              <span className={`dot ${profile.lastRun.success ? "ok" : "err"}`} /> {profile.lastRun.success ? "Erfolgreich" : "Fehlgeschlagen"}{" "}
              {timeAgo(profile.lastRun.at)}
            </>
          ) : (
            "Noch nie deployt"
          )}
        </span>
        <div className="row gap">
          <button className="icon-btn" title="Bearbeiten" onClick={onEdit}>
            <Pencil size={16} />
          </button>
          <button
            className="icon-btn danger"
            title="Löschen"
            onClick={async () => (await confirm(`„${profile.name}“ löschen?`, { title: "Deploy-Profil", kind: "warning" })) && removeProfile(profile.id)}
          >
            <Trash2 size={16} />
          </button>
          <Button variant="primary" icon={<Rocket size={16} />} loading={running} disabled={!target} onClick={() => deploy(profile.id)}>
            Deploy
          </Button>
        </div>
      </div>
    </article>
  );
}

export function ProjectDetail({ id }: { id: string }) {
  const { data, navigate, updateProject, removeProject } = useStore();
  const project = data.projects.find((p) => p.id === id);
  const profiles = data.profiles.filter((p) => p.projectId === id);
  const [wizard, setWizard] = useState(false);
  const [editing, setEditing] = useState<Profile | null>(null);
  const [tools, setTools] = useState<ToolStatus[] | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState("");
  const [confirmRemove, setConfirmRemove] = useState(false);

  const toolIds = useMemo(() => {
    if (!project) return [];
    const ids = new Set(baseTools(project));
    profiles.forEach((p) => p.tools.forEach((t) => ids.add(t)));
    return [...ids];
  }, [project, profiles]);

  const check = useCallback(() => {
    if (!project) return;
    setChecking(true);
    api
      .checkTools(toolIds, project.constraints)
      .then(setTools)
      .catch((e) => setError(errorText(e)))
      .finally(() => setChecking(false));
  }, [project, toolIds]);

  useEffect(() => {
    check();
    // Only re-check when the set of tools changes, not on every render.
  }, [toolIds.join(",")]);

  if (!project) return <Empty icon={<FolderOpen size={32} />} title="Projekt nicht gefunden" />;

  const redetect = async () => {
    try {
      updateProject(project.id, await api.detectProject(project.path));
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="page">
      <button className="link back" onClick={() => navigate({ page: "projects" })}>
        <ArrowLeft size={14} /> Projekte
      </button>
      <header className="page-head">
        <div className="row gap">
          <div className="kind-icon lg">
            <KindIcon kind={project.types[0]?.kind ?? "other"} size={26} />
          </div>
          <div className="min0">
            <h1 className="ellipsis">{project.name}</h1>
            <p className="muted mono small ellipsis">{project.path}</p>
            <div className="row gap wrap">
              {project.types.map((t) => (
                <Badge key={t.kind} tone="accent">
                  {t.label}
                </Badge>
              ))}
              {project.types.length === 0 && <Badge tone="warn">Kein bekannter Projekttyp</Badge>}
              {project.git?.branch && <Badge>⎇ {project.git.branch}</Badge>}
            </div>
          </div>
        </div>
        <div className="row gap">
          <Button size="sm" variant="ghost" icon={<FolderOpen size={14} />} onClick={() => revealItemInDir(project.path)}>
            Ordner
          </Button>
          <Button size="sm" variant="ghost" icon={<RefreshCw size={14} />} onClick={redetect}>
            Neu erkennen
          </Button>
          <Button size="sm" variant="ghost" icon={<Trash2 size={14} />} onClick={() => setConfirmRemove(true)}>
            Entfernen
          </Button>
        </div>
      </header>
      <ErrorNote>{error}</ErrorNote>

      <section>
        <div className="section-head">
          <h2>Deploy-Ziele</h2>
          <Button variant="primary" icon={<Plus size={16} />} disabled={project.types.length === 0} onClick={() => setWizard(true)}>
            Deploy-Ziel hinzufügen
          </Button>
        </div>
        {profiles.length === 0 ? (
          <div className="card dashed center-text">
            <p className="muted">
              Lege fest, wohin das Projekt soll – Handy, Tablet, Server, dieser PC, ein USB-Laufwerk oder als Download per QR-Code.
            </p>
            <Button variant="primary" icon={<Plus size={16} />} disabled={project.types.length === 0} onClick={() => setWizard(true)}>
              Erstes Deploy-Ziel anlegen
            </Button>
          </div>
        ) : (
          <div className="card-grid">
            {profiles.map((p) => (
              <ProfileCard key={p.id} profile={p} onEdit={() => setEditing(p)} />
            ))}
          </div>
        )}
      </section>

      <div className="two-col">
        <section className="card">
          <h3>Voraussetzungen</h3>
          <RequirementsList statuses={tools} loading={checking} projectDir={project.path} onRecheck={check} compact />
        </section>
        <div className="stack">
          {project.constraints.length > 0 && (
            <section className="card">
              <h3>Vom Projekt verlangte Versionen</h3>
              <table className="table">
                <tbody>
                  {project.constraints.map((c, i) => (
                    <tr key={i}>
                      <td>{c.tool}</td>
                      <td className="mono">{c.constraint}</td>
                      <td className="muted small">{c.source}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
          )}
          <WorkflowCard project={project} />
        </div>
      </div>

      {wizard && <ProfileWizard project={project} onClose={() => setWizard(false)} />}
      {editing && <ProfileEditor profile={editing} onClose={() => setEditing(null)} />}
      {confirmRemove && (
        <Modal
          title="Projekt entfernen?"
          onClose={() => setConfirmRemove(false)}
          footer={
            <>
              <Button onClick={() => setConfirmRemove(false)}>Abbrechen</Button>
              <Button
                variant="danger"
                onClick={() => {
                  removeProject(project.id);
                  navigate({ page: "projects" });
                }}
              >
                Entfernen
              </Button>
            </>
          }
        >
          <p>
            Das Projekt und seine Deploy-Profile werden aus easyDeploy entfernt. <strong>Deine Dateien bleiben unverändert.</strong>
          </p>
        </Modal>
      )}
    </div>
  );
}
