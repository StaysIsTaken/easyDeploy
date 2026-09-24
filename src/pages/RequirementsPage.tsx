import { useCallback, useEffect, useState } from "react";
import { RequirementsList } from "../components/RequirementsList";
import { ErrorNote, PageHeader } from "../components/ui";
import { api, errorText } from "../lib/api";
import { useStore } from "../lib/store";
import type { ToolStatus } from "../lib/types";

const GROUPS: { title: string; ids: string[]; os?: string }[] = [
  { title: "Allgemein", ids: ["git", "ssh", "tar", "gh"] },
  { title: "Node / Web / React Native", ids: ["node", "npm", "pnpm", "yarn"] },
  { title: "Flutter & Dart", ids: ["flutter", "dart", "fvm"] },
  { title: "Android", ids: ["java", "adb", "sdkmanager", "android-licenses"] },
  { title: "Apple (iOS / macOS)", ids: ["xcode", "xcode-license", "cocoapods"], os: "macos" },
  { title: "Rust / Tauri", ids: ["rust"] },
  { title: "Python", ids: ["python", "uv"] },
  { title: "Docker", ids: ["docker", "docker-daemon"] },
];

function Group({ title, ids }: { title: string; ids: string[] }) {
  const [statuses, setStatuses] = useState<ToolStatus[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const check = useCallback(() => {
    setLoading(true);
    api
      .checkTools(ids, [])
      .then((s) => setStatuses(s.map((x) => ({ ...x, optional: true }))))
      .catch((e) => setError(errorText(e)))
      .finally(() => setLoading(false));
  }, [ids]);
  useEffect(check, [check]);
  const ok = statuses?.filter((s) => s.ok).length ?? 0;
  return (
    <section className="card">
      <div className="row between">
        <h3>{title}</h3>
        {statuses && (
          <span className={`small ${ok === statuses.length ? "c-ok" : "muted"}`}>
            {ok}/{statuses.length} bereit
          </span>
        )}
      </div>
      <ErrorNote>{error}</ErrorNote>
      <RequirementsList statuses={statuses} loading={loading} onRecheck={check} compact />
    </section>
  );
}

export function RequirementsPage() {
  const { sys } = useStore();
  return (
    <div className="page">
      <PageHeader
        title="Voraussetzungen"
        subtitle={
          <>
            Welche SDKs und Tools auf diesem Rechner installiert sind. Fehlendes kannst du per Klick installieren
            {sys?.os === "macos" && " (über Homebrew)"}
            {sys?.os === "windows" && " (über winget)"}
            {sys?.os === "linux" && sys.linuxPackageManager && ` (über ${sys.linuxPackageManager})`}.
          </>
        }
      />
      <div className="card-grid wide">
        {GROUPS.filter((g) => !g.os || g.os === sys?.os).map((g) => (
          <Group key={g.title} title={g.title} ids={g.ids} />
        ))}
      </div>
    </div>
  );
}
