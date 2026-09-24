import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Download, ExternalLink, FolderGit2, Lock, LogOut, Play, RefreshCw, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { GithubIcon } from "../components/icons";
import { Badge, Button, ErrorNote, Field, PageHeader, Spinner } from "../components/ui";
import { api, errorText } from "../lib/api";
import { useJobs } from "../lib/jobs";
import { useStore } from "../lib/store";
import type { GhRepo, GhRun, GhUser, GhWorkflow } from "../lib/types";
import { joinPath, timeAgo } from "../lib/util";

const TOKEN_URL = "https://github.com/settings/tokens/new?scopes=repo,workflow&description=easyDeploy";

function Connect({ onConnected }: { onConnected: (u: GhUser) => void }) {
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState<"token" | "cli" | null>(null);
  const [error, setError] = useState("");

  const connect = async (mode: "token" | "cli") => {
    setBusy(mode);
    setError("");
    try {
      onConnected(mode === "cli" ? await api.ghImportCli() : await api.ghConnectToken(token));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="card narrow">
      <h3 className="row gap">
        <GithubIcon /> Mit GitHub verbinden
      </h3>
      <p className="muted">Damit kannst du Repositories klonen, Workflows starten und den Status von GitHub Actions sehen.</p>
      <Button icon={<GithubIcon size={16} />} loading={busy === "cli"} onClick={() => connect("cli")}>
        Anmeldung der GitHub CLI übernehmen
      </Button>
      <div className="divider">oder</div>
      <Field
        label="Personal Access Token"
        hint={
          <>
            Benötigte Rechte: <span className="mono">repo</span>, <span className="mono">workflow</span>.{" "}
            <button className="link" onClick={() => openUrl(TOKEN_URL)}>
              Token auf GitHub erstellen
            </button>
            . Der Token wird im Schlüsselbund des Systems gespeichert.
          </>
        }
      >
        <input className="input mono" type="password" value={token} onChange={(e) => setToken(e.target.value)} placeholder="ghp_… oder github_pat_…" />
      </Field>
      <Button variant="primary" loading={busy === "token"} disabled={!token.trim()} onClick={() => connect("token")}>
        Verbinden
      </Button>
      <ErrorNote>{error}</ErrorNote>
    </div>
  );
}

function RunBadge({ run }: { run: GhRun }) {
  if (run.status !== "completed") return <Badge tone="accent">{run.status === "queued" ? "wartet" : "läuft"}</Badge>;
  if (run.conclusion === "success") return <Badge tone="ok">erfolgreich</Badge>;
  if (run.conclusion === "cancelled") return <Badge tone="warn">abgebrochen</Badge>;
  return <Badge tone="err">{run.conclusion ?? "fehlgeschlagen"}</Badge>;
}

function RepoDetail({ repo }: { repo: GhRepo }) {
  const { data, addProject, navigate } = useStore();
  const { run } = useJobs();
  const [workflows, setWorkflows] = useState<GhWorkflow[] | null>(null);
  const [runs, setRuns] = useState<GhRun[] | null>(null);
  const [ref, setRef] = useState(repo.defaultBranch);
  const [error, setError] = useState("");
  const [info, setInfo] = useState("");
  const local = data.projects.find((p) => p.git?.github?.toLowerCase() === repo.fullName.toLowerCase());

  const load = useCallback(() => {
    setError("");
    api.ghWorkflows(repo.owner, repo.name).then(setWorkflows).catch((e) => setError(errorText(e)));
    api.ghRuns(repo.owner, repo.name).then(setRuns).catch((e) => setError(errorText(e)));
  }, [repo]);

  useEffect(() => {
    setWorkflows(null);
    setRuns(null);
    setRef(repo.defaultBranch);
    setInfo("");
    load();
  }, [repo, load]);

  useEffect(() => {
    if (!runs?.some((r) => r.status !== "completed")) return;
    const t = window.setInterval(() => api.ghRuns(repo.owner, repo.name).then(setRuns).catch(() => {}), 8000);
    return () => window.clearInterval(t);
  }, [runs, repo]);

  const clone = async () => {
    const parent = await open({ directory: true, title: "Wohin soll das Repository geklont werden?" });
    if (typeof parent !== "string") return;
    const dest = joinPath(parent, repo.name);
    if (await api.pathExists(dest)) {
      setError(`${dest} existiert bereits.`);
      return;
    }
    const r = await run({ title: `${repo.fullName} klonen`, steps: [{ kind: "gitClone", name: "Repository klonen", url: repo.cloneUrl, dest }] });
    if (r.success) {
      const p = addProject(await api.detectProject(dest));
      navigate({ page: "project", id: p.id });
    }
  };

  const dispatch = async (w: GhWorkflow) => {
    setError("");
    setInfo("");
    try {
      await api.ghDispatch(repo.owner, repo.name, w.file, ref);
      setInfo(`„${w.name}“ wurde auf ${ref} gestartet.`);
      window.setTimeout(load, 2500);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="stack">
      <section className="card">
        <div className="row between wrap">
          <div className="min0">
            <h2 className="row gap">
              {repo.private && <Lock size={16} />} {repo.fullName}
            </h2>
            {repo.description && <p className="muted">{repo.description}</p>}
          </div>
          <div className="row gap">
            <Button size="sm" variant="ghost" icon={<ExternalLink size={14} />} onClick={() => openUrl(repo.htmlUrl)}>
              Öffnen
            </Button>
            {local ? (
              <Button size="sm" variant="primary" icon={<FolderGit2 size={14} />} onClick={() => navigate({ page: "project", id: local.id })}>
                Projekt öffnen
              </Button>
            ) : (
              <Button size="sm" variant="primary" icon={<Download size={14} />} onClick={clone}>
                Klonen & hinzufügen
              </Button>
            )}
          </div>
        </div>
      </section>
      <ErrorNote>{error}</ErrorNote>
      {info && <div className="note note-ok small">{info}</div>}

      <section className="card">
        <div className="row between">
          <h3>Workflows</h3>
          <div className="row gap">
            <span className="small muted">Branch</span>
            <input className="input small mono" style={{ width: 140 }} value={ref} onChange={(e) => setRef(e.target.value)} />
          </div>
        </div>
        {!workflows && <Spinner />}
        {workflows?.length === 0 && (
          <p className="small muted">Keine Workflows. Auf der Projektseite kann easyDeploy eine passende Workflow-Datei erzeugen.</p>
        )}
        <div className="list">
          {workflows?.map((w) => (
            <div key={w.id} className="list-item">
              <div className="grow min0">
                <strong>{w.name}</strong>
                <p className="small muted mono">{w.path}</p>
              </div>
              {w.state !== "active" && <Badge tone="warn">{w.state}</Badge>}
              <Button size="sm" icon={<Play size={14} />} onClick={() => dispatch(w)}>
                Starten
              </Button>
            </div>
          ))}
        </div>
      </section>

      <section className="card">
        <div className="row between">
          <h3>Letzte Läufe</h3>
          <button className="icon-btn" onClick={load} title="Aktualisieren">
            <RefreshCw size={16} />
          </button>
        </div>
        {!runs && <Spinner />}
        {runs?.length === 0 && <p className="small muted">Noch keine Läufe.</p>}
        <div className="list">
          {runs?.map((r) => (
            <button key={r.id} className="list-item clickable" onClick={() => openUrl(r.htmlUrl)}>
              <RunBadge run={r} />
              <div className="grow min0 left">
                <strong className="ellipsis">{r.title || r.name}</strong>
                <p className="small muted">
                  {r.name} · {r.branch} · {r.event} · {timeAgo(new Date(r.createdAt).getTime())}
                </p>
              </div>
              <ExternalLink size={14} className="c-mute" />
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}

export function GitHubPage() {
  const [user, setUser] = useState<GhUser | null | undefined>(undefined);
  const [repos, setRepos] = useState<GhRepo[] | null>(null);
  const [selected, setSelected] = useState<GhRepo | null>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    api
      .ghStatus()
      .then(setUser)
      .catch((e) => {
        setUser(null);
        setError(errorText(e));
      });
  }, []);

  useEffect(() => {
    if (!user) return;
    api
      .ghRepos()
      .then(setRepos)
      .catch((e) => setError(errorText(e)));
  }, [user]);

  const filtered = useMemo(
    () => (repos ?? []).filter((r) => r.fullName.toLowerCase().includes(query.toLowerCase())),
    [repos, query],
  );

  const logout = async () => {
    await api.ghLogout();
    setUser(null);
    setRepos(null);
    setSelected(null);
  };

  return (
    <div className="page">
      <PageHeader
        title="GitHub"
        subtitle="Repositories klonen, Actions-Workflows starten und Läufe verfolgen."
        actions={
          user && (
            <div className="row gap">
              <img src={user.avatarUrl} alt="" className="avatar" />
              <span>{user.name ?? user.login}</span>
              <Button size="sm" variant="ghost" icon={<LogOut size={14} />} onClick={logout}>
                Abmelden
              </Button>
            </div>
          )
        }
      />
      <ErrorNote>{error}</ErrorNote>
      {user === undefined && <Spinner />}
      {user === null && <Connect onConnected={setUser} />}
      {user && (
        <div className="split">
          <aside className="repo-list">
            <div className="search">
              <Search size={14} />
              <input placeholder="Repository suchen" value={query} onChange={(e) => setQuery(e.target.value)} />
            </div>
            {!repos && <Spinner label="Lade Repositories …" />}
            {filtered.map((r) => (
              <button key={r.fullName} className={`repo-item ${selected?.fullName === r.fullName ? "active" : ""}`} onClick={() => setSelected(r)}>
                <div className="row gap">
                  <strong className="ellipsis">{r.name}</strong>
                  {r.private && <Lock size={12} className="c-mute" />}
                </div>
                <span className="small muted ellipsis">
                  {r.owner}
                  {r.language && ` · ${r.language}`} · {timeAgo(new Date(r.updatedAt).getTime())}
                </span>
              </button>
            ))}
          </aside>
          <div className="grow min0">
            {selected ? <RepoDetail repo={selected} /> : <p className="muted pad">Wähle links ein Repository.</p>}
          </div>
        </div>
      )}
    </div>
  );
}
