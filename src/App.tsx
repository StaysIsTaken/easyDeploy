import { listen } from "@tauri-apps/api/event";
import { FolderKanban, MonitorSmartphone, QrCode, Rocket, Wrench, X } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { DeployProvider } from "./components/DeployFlow";
import { GithubIcon } from "./components/icons";
import { JobPill, RunPanel } from "./components/RunPanel";
import { Spinner } from "./components/ui";
import { api } from "./lib/api";
import { JobsProvider } from "./lib/jobs";
import { StoreProvider, useStore, type Route } from "./lib/store";
import { GitHubPage } from "./pages/GitHubPage";
import { ProjectDetail } from "./pages/ProjectDetail";
import { ProjectsPage } from "./pages/ProjectsPage";
import { RequirementsPage } from "./pages/RequirementsPage";
import { SharesPage } from "./pages/SharesPage";
import { TargetsPage } from "./pages/TargetsPage";
import "./styles.css";

const NAV: { page: Route["page"]; label: string; icon: ReactNode }[] = [
  { page: "projects", label: "Projekte", icon: <FolderKanban size={18} /> },
  { page: "targets", label: "Geräte & Ziele", icon: <MonitorSmartphone size={18} /> },
  { page: "requirements", label: "Voraussetzungen", icon: <Wrench size={18} /> },
  { page: "github", label: "GitHub", icon: <GithubIcon size={18} /> },
  { page: "shares", label: "Freigaben", icon: <QrCode size={18} /> },
];

function Sidebar() {
  const { route, navigate, data, sys } = useStore();
  const [shareCount, setShareCount] = useState(0);

  useEffect(() => {
    const refresh = () => api.shareList().then((s) => setShareCount(s.length)).catch(() => {});
    refresh();
    const un = listen("share://changed", refresh);
    return () => {
      un.then((u) => u());
    };
  }, []);

  const active = route.page === "project" ? "projects" : route.page;
  return (
    <nav className="sidebar">
      <div className="brand">
        <div className="logo">
          <Rocket size={18} />
        </div>
        <span>easyDeploy</span>
      </div>
      {NAV.map((n) => (
        <button key={n.page} className={`nav-item ${active === n.page ? "active" : ""}`} onClick={() => navigate({ page: n.page } as Route)}>
          {n.icon}
          <span>{n.label}</span>
          {n.page === "shares" && shareCount > 0 && <span className="nav-count">{shareCount}</span>}
        </button>
      ))}
      {data.projects.length > 0 && (
        <div className="nav-projects">
          <span className="eyebrow">Projekte</span>
          {data.projects.slice(0, 8).map((p) => (
            <button
              key={p.id}
              className={`nav-sub ${route.page === "project" && route.id === p.id ? "active" : ""}`}
              onClick={() => navigate({ page: "project", id: p.id })}
            >
              <span className="ellipsis">{p.name}</span>
            </button>
          ))}
        </div>
      )}
      <div className="sidebar-foot small muted">
        {sys ? `${sys.hostname} · ${sys.os === "macos" ? "macOS" : sys.os === "windows" ? "Windows" : "Linux"}` : ""}
      </div>
    </nav>
  );
}

function ShareToast() {
  const { route, navigate } = useStore();
  const [show, setShow] = useState(false);
  useEffect(() => {
    let known = -1;
    const un = listen("share://changed", async () => {
      const n = (await api.shareList()).length;
      if (known >= 0 && n > known) setShow(true);
      known = n;
    });
    api.shareList().then((s) => (known = s.length)).catch(() => {});
    return () => {
      un.then((u) => u());
    };
  }, []);
  if (!show || route.page === "shares") return null;
  return (
    <div className="toast">
      <QrCode size={18} />
      <span>Download im Netzwerk ist bereit.</span>
      <button
        className="link"
        onClick={() => {
          setShow(false);
          navigate({ page: "shares" });
        }}
      >
        QR-Code anzeigen
      </button>
      <button className="icon-btn" onClick={() => setShow(false)}>
        <X size={14} />
      </button>
    </div>
  );
}

function Main() {
  const { route, loaded } = useStore();
  if (!loaded) return <Spinner label="Lade …" />;
  switch (route.page) {
    case "projects":
      return <ProjectsPage />;
    case "project":
      return <ProjectDetail id={route.id} />;
    case "targets":
      return <TargetsPage />;
    case "requirements":
      return <RequirementsPage />;
    case "github":
      return <GitHubPage />;
    case "shares":
      return <SharesPage />;
  }
}

export default function App() {
  return (
    <StoreProvider>
      <JobsProvider>
        <DeployProvider>
          <div className="app">
            <Sidebar />
            <main className="main">
              <Main />
            </main>
          </div>
          <RunPanel />
          <JobPill />
          <ShareToast />
        </DeployProvider>
      </JobsProvider>
    </StoreProvider>
  );
}
