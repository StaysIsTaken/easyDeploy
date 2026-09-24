import { open } from "@tauri-apps/plugin-dialog";
import { FolderPlus, Rocket } from "lucide-react";
import { useState } from "react";
import { useDeploy } from "../components/DeployFlow";
import { GithubIcon, KindIcon, TargetIcon } from "../components/icons";
import { Badge, Button, Empty, ErrorNote, PageHeader } from "../components/ui";
import { api, errorText } from "../lib/api";
import { useStore } from "../lib/store";
import { timeAgo } from "../lib/util";

export function useAddProject() {
  const { data, addProject, navigate } = useStore();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const addPath = async (path: string) => {
    const existing = data.projects.find((p) => p.path === path);
    if (existing) return navigate({ page: "project", id: existing.id });
    setBusy(true);
    setError("");
    try {
      const info = await api.detectProject(path);
      const p = addProject(info);
      navigate({ page: "project", id: p.id });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const pick = async () => {
    const path = await open({ directory: true, title: "Projektordner wählen" });
    if (typeof path === "string") await addPath(path);
  };
  return { pick, addPath, error, busy };
}

export function ProjectsPage() {
  const { data, targets, navigate } = useStore();
  const { deploy } = useDeploy();
  const { pick, error, busy } = useAddProject();

  return (
    <div className="page">
      <PageHeader
        title="Projekte"
        subtitle="Füge einen Projektordner hinzu. easyDeploy erkennt den Typ und schlägt passende Deploy-Ziele vor."
        actions={
          <Button variant="primary" icon={<FolderPlus size={16} />} loading={busy} onClick={pick}>
            Projekt hinzufügen
          </Button>
        }
      />
      <ErrorNote>{error}</ErrorNote>

      {data.projects.length === 0 ? (
        <Empty icon={<Rocket size={32} />} title="Noch keine Projekte">
          <p className="muted">Flutter, React Native, Tauri, Node, Python oder Docker – wähle einfach den Ordner aus.</p>
          <div className="row gap center">
            <Button variant="primary" icon={<FolderPlus size={16} />} onClick={pick}>
              Ordner wählen
            </Button>
            <Button icon={<GithubIcon size={16} />} onClick={() => navigate({ page: "github" })}>
              Von GitHub klonen
            </Button>
          </div>
        </Empty>
      ) : (
        <div className="card-grid">
          {data.projects.map((p) => {
            const profiles = data.profiles.filter((x) => x.projectId === p.id);
            return (
              <article key={p.id} className="card project-card" onClick={() => navigate({ page: "project", id: p.id })}>
                <div className="row gap">
                  <div className="kind-icon">
                    <KindIcon kind={p.types[0]?.kind ?? "other"} size={20} />
                  </div>
                  <div className="grow min0">
                    <h3 className="ellipsis">{p.name}</h3>
                    <p className="small muted mono ellipsis">{p.path}</p>
                  </div>
                </div>
                <div className="row gap wrap">
                  {p.types.length === 0 && <Badge tone="warn">Typ unbekannt</Badge>}
                  {p.types.map((t) => (
                    <Badge key={t.kind}>{t.label}</Badge>
                  ))}
                  {p.git?.branch && <Badge tone="neutral">⎇ {p.git.branch}</Badge>}
                </div>
                <div className="quick-deploys" onClick={(e) => e.stopPropagation()}>
                  {profiles.slice(0, 3).map((pr) => {
                    const t = targets.find((x) => x.id === pr.targetId);
                    return (
                      <button key={pr.id} className="quick-deploy" onClick={() => deploy(pr.id)} title={pr.name}>
                        <TargetIcon kind={t?.kind ?? "local"} size={15} />
                        <span className="ellipsis">{t?.name ?? pr.name}</span>
                        {pr.lastRun && <span className={`dot ${pr.lastRun.success ? "ok" : "err"}`} title={timeAgo(pr.lastRun.at)} />}
                        <Rocket size={14} className="c-accent" />
                      </button>
                    );
                  })}
                  {profiles.length === 0 && <span className="small muted">Noch kein Deploy-Ziel</span>}
                </div>
              </article>
            );
          })}
        </div>
      )}
    </div>
  );
}
