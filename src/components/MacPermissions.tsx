import { ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, errorText } from "../lib/api";
import type { MacPermissionReport, Project } from "../lib/types";
import { Button, ErrorNote } from "./ui";

/** Electron apps packaged with electron-builder (where the signing defaults apply). */
export function usesElectronBuilder(project: Project): boolean {
  return project.types.some((t) => t.details.packager === "electron-builder");
}

/**
 * Warns when a signed macOS build would lack microphone/camera access:
 * electron-builder's default entitlements don't include the device
 * entitlements and Info.plist needs usage descriptions.
 */
export function MacPermissionsNote({
  project,
  initial,
  onChange,
}: {
  project: Project;
  initial?: MacPermissionReport | null;
  onChange?: (r: MacPermissionReport) => void;
}) {
  const [report, setReport] = useState<MacPermissionReport | null>(initial ?? null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [done, setDone] = useState<{ changes: string[]; ids: string[] } | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await api.checkMacPermissions(project.path);
      setReport(r);
      onChange?.(r);
    } catch (e) {
      setError(errorText(e));
    }
  }, [project.path, onChange]);

  useEffect(() => {
    if (!initial) load();
  }, [initial, load]);

  const fix = async () => {
    if (!report) return;
    setBusy(true);
    setError("");
    try {
      const ids = report.issues.map((i) => i.id);
      setDone({ changes: await api.fixMacPermissions(project.path, ids), ids });
      await load();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  if (done && report && report.issues.length === 0) {
    return (
      <div className="note note-ok small">
        <div>
          <strong>macOS-Berechtigungen ergänzt</strong>
          <ul className="small">
            {done.changes.map((c) => (
              <li key={c} className="mono">
                {c}
              </li>
            ))}
          </ul>
          Baue die App neu. Hat macOS den Zugriff früher schon verweigert, setze ihn einmal zurück:{" "}
          {done.ids.map((id) => (
            <div key={id} className="mono">
              tccutil reset {id === "camera" ? "Camera" : "Microphone"}
              {report.appId ? ` ${report.appId}` : ""}
            </div>
          ))}
        </div>
      </div>
    );
  }
  if (!report?.applies || report.issues.length === 0) return <ErrorNote>{error}</ErrorNote>;

  const labels = report.issues.map((i) => i.label).join(" und ");
  return (
    <div className="note note-warn small">
      <ShieldAlert size={16} />
      <div className="stack">
        <strong>{labels} wird im signierten Mac-Build fehlen</strong>
        <ul>
          {report.issues.map((i) => (
            <li key={i.id}>
              {i.label} wird in <span className="mono">{i.usedIn}</span> verwendet
              {report.verified
                ? `, aber ${[
                    i.missingEntitlement && "das Entitlement fehlt (Hardened Runtime)",
                    i.missingUsageDescription && "die Info.plist-Beschreibung fehlt",
                  ]
                    .filter(Boolean)
                    .join(" und ")}.`
                : ". Die Konfiguration ist JavaScript und konnte nicht geprüft werden."}
            </li>
          ))}
        </ul>
        <span>
          electron-builder signiert mit Hardened Runtime und nutzt ohne eigene Vorgabe Standard-Entitlements ohne
          Geräte­zugriff – macOS blockiert dann {labels}.
        </span>
        {report.fixable ? (
          <div>
            <Button size="sm" variant="primary" loading={busy} onClick={fix}>
              In {report.configFile ?? "package.json"} ergänzen
            </Button>
          </div>
        ) : (
          report.manual && (
            <>
              <span>
                Bitte in <span className="mono">{report.configFile}</span> ergänzen:
              </span>
              <pre className="mono small">{report.manual}</pre>
            </>
          )
        )}
        <ErrorNote>{error}</ErrorNote>
      </div>
    </div>
  );
}
