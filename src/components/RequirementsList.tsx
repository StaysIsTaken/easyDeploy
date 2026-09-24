import { openUrl } from "@tauri-apps/plugin-opener";
import { AlertTriangle, CheckCircle2, Download, ExternalLink, RefreshCw, XCircle } from "lucide-react";
import { useState } from "react";
import { useJobs } from "../lib/jobs";
import type { ToolAction, ToolStatus } from "../lib/types";
import { Button, Modal, Spinner } from "./ui";

/** Shows tool checks with one-click install/fix buttons. */
export function RequirementsList({
  statuses,
  loading,
  projectDir,
  onRecheck,
  compact,
}: {
  statuses: ToolStatus[] | null;
  loading?: boolean;
  projectDir?: string;
  onRecheck: () => void;
  compact?: boolean;
}) {
  const { run } = useJobs();
  const [confirm, setConfirm] = useState<ToolAction | null>(null);

  const execute = async (a: ToolAction) => {
    if (a.url) {
      await openUrl(a.url);
      return;
    }
    if (!a.command) return;
    await run({
      title: a.label,
      kind: "install",
      baseDir: a.inProject ? projectDir : undefined,
      steps: [{ kind: "shell", name: a.label, command: a.command }],
    });
    onRecheck();
  };

  const trigger = (a: ToolAction) => (a.confirm ? setConfirm(a) : execute(a));

  if (!statuses && loading) return <Spinner label="Prüfe installierte Tools …" />;
  if (!statuses) return null;

  const sorted = [...statuses].sort((a, b) => Number(a.ok) - Number(b.ok));
  return (
    <div className={`req-list ${compact ? "compact" : ""}`}>
      {sorted.map((s) => (
        <div key={s.id} className={`req ${s.ok ? "ok" : s.installed ? "warn" : "missing"}`}>
          <div className="req-icon">
            {s.ok ? (
              <CheckCircle2 size={18} className="c-ok" />
            ) : s.installed ? (
              <AlertTriangle size={18} className="c-warn" />
            ) : (
              <XCircle size={18} className="c-err" />
            )}
          </div>
          <div className="req-main">
            <div className="row gap wrap">
              <strong>{s.name}</strong>
              {s.version && <span className="mono small muted">{s.version}</span>}
              {s.required && (
                <span className="small muted">
                  benötigt <span className="mono">{s.required}</span>
                  {s.requiredSource && ` (${s.requiredSource})`}
                </span>
              )}
            </div>
            {!s.ok && s.message && <p className="small">{s.message}</p>}
          </div>
          {!s.ok && (
            <div className="req-actions">
              {s.actions.map((a, i) => (
                <Button
                  key={i}
                  size="sm"
                  variant={i === 0 && !a.url ? "primary" : "secondary"}
                  icon={a.url ? <ExternalLink size={14} /> : <Download size={14} />}
                  onClick={() => trigger(a)}
                  title={a.command ?? a.url ?? undefined}
                >
                  {a.label}
                </Button>
              ))}
            </div>
          )}
        </div>
      ))}
      <div className="row gap">
        <Button size="sm" variant="ghost" icon={<RefreshCw size={14} />} loading={loading} onClick={onRecheck}>
          Erneut prüfen
        </Button>
      </div>
      {confirm && (
        <Modal
          title="Bitte bestätigen"
          onClose={() => setConfirm(null)}
          footer={
            <>
              <Button onClick={() => setConfirm(null)}>Ablehnen</Button>
              <Button
                variant="primary"
                onClick={() => {
                  const a = confirm;
                  setConfirm(null);
                  execute(a);
                }}
              >
                Akzeptieren & ausführen
              </Button>
            </>
          }
        >
          <p>{confirm.confirm}</p>
          <p className="small muted">
            Befehl: <span className="mono">{confirm.command}</span>
          </p>
        </Modal>
      )}
    </div>
  );
}
