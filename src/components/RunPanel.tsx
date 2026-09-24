import {
  AlertTriangle,
  CheckCircle2,
  Circle,
  FileText,
  KeyRound,
  Lightbulb,
  Loader2,
  Minimize2,
  MinusCircle,
  ShieldCheck,
  Square,
  Trash2,
  XCircle,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { findHints } from "../lib/hints";
import { useJobs, type JobState } from "../lib/jobs";
import type { JobPrompt, StepStatus } from "../lib/types";
import { JobTerminal } from "./Terminal";
import { Badge, Button } from "./ui";

export function StepIcon({ status }: { status: StepStatus }) {
  switch (status) {
    case "running":
      return <Loader2 size={16} className="spin c-accent" />;
    case "success":
      return <CheckCircle2 size={16} className="c-ok" />;
    case "failed":
      return <XCircle size={16} className="c-err" />;
    case "skipped":
      return <MinusCircle size={16} className="c-mute" />;
    default:
      return <Circle size={16} className="c-mute" />;
  }
}

export function JobStatusBadge({ job }: { job: JobState }) {
  if (job.status === "running") return <Badge tone="accent">läuft</Badge>;
  if (job.status === "success") return <Badge tone="ok">erfolgreich</Badge>;
  if (job.status === "cancelled") return <Badge tone="warn">abgebrochen</Badge>;
  return <Badge tone="err">fehlgeschlagen</Badge>;
}

function duration(job: JobState) {
  const s = Math.round(((job.finishedAt ?? Date.now()) - job.startedAt) / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

function PromptCard({ prompt }: { prompt: JobPrompt }) {
  const { answerPrompt } = useJobs();
  const [secret, setSecret] = useState("");
  const [showLicense, setShowLicense] = useState(prompt.kind === "license");

  const meta = {
    secret: { icon: <KeyRound size={20} />, title: "Passwort / Eingabe benötigt" },
    hostkey: { icon: <ShieldCheck size={20} />, title: "Unbekannter Server – vertrauen?" },
    license: { icon: <FileText size={20} />, title: "Lizenz bestätigen" },
    confirm: { icon: <AlertTriangle size={20} />, title: "Rückfrage" },
  }[prompt.kind];

  const contextLines = prompt.context.split("\n");
  const shortContext = contextLines.slice(-8).join("\n");

  return (
    <div className={`prompt prompt-${prompt.kind}`}>
      <div className="prompt-head">
        {meta.icon}
        <div>
          <strong>{meta.title}</strong>
          <p className="mono small">{prompt.message}</p>
        </div>
      </div>

      {prompt.kind === "hostkey" && (
        <>
          <pre className="prompt-context">{shortContext}</pre>
          <p className="small muted">
            Das ist die erste Verbindung zu diesem Server. Vergleiche den Fingerabdruck, wenn du sicher gehen willst.
          </p>
        </>
      )}
      {prompt.kind === "license" && (
        <>
          {showLicense ? (
            <pre className="prompt-context tall">{prompt.context}</pre>
          ) : (
            <button className="link small" onClick={() => setShowLicense(true)}>
              Lizenztext anzeigen
            </button>
          )}
          <p className="small muted">Lies die Lizenz und entscheide selbst. easyDeploy akzeptiert Lizenzen nie automatisch.</p>
        </>
      )}
      {prompt.kind === "confirm" && <pre className="prompt-context">{shortContext}</pre>}

      {prompt.kind === "secret" ? (
        <form
          className="row gap"
          onSubmit={(e) => {
            e.preventDefault();
            answerPrompt(prompt, secret + "\r");
            setSecret("");
          }}
        >
          <input
            type="password"
            autoFocus
            className="input grow"
            placeholder="Eingabe (wird nicht gespeichert)"
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
          />
          <Button variant="primary" type="submit">
            Senden
          </Button>
        </form>
      ) : (
        <div className="row gap wrap">
          {prompt.options.map((o) => (
            <Button key={o.value} variant={o.primary ? "primary" : "secondary"} onClick={() => answerPrompt(prompt, o.value)}>
              {o.label}
            </Button>
          ))}
        </div>
      )}
      <button className="link small" onClick={() => answerPrompt(prompt, null)}>
        Selbst im Terminal beantworten
      </button>
    </div>
  );
}

function FailureHints({ job }: { job: JobState }) {
  const { buffer, run } = useJobs();
  const hints = useMemo(() => (job.status === "failed" ? findHints(buffer(job.id).join("")) : []), [job.status, job.id, buffer]);
  if (hints.length === 0) return null;
  return (
    <div className="hints">
      {hints.map((h) => (
        <div key={h.title} className="hint">
          <div className="row gap">
            <Lightbulb size={16} className="c-warn" />
            <strong>{h.title}</strong>
          </div>
          <p className="small">{h.text}</p>
          {h.action && (
            <Button
              size="sm"
              onClick={() =>
                run({ title: h.action!.label, baseDir: job.baseDir, steps: [{ kind: "shell", name: h.action!.label, command: h.action!.command }] })
              }
            >
              {h.action.label}
            </Button>
          )}
        </div>
      ))}
    </div>
  );
}

export function RunPanel() {
  const { jobs, activeId, panelOpen, closePanel, openPanel, cancel, clearFinished } = useJobs();
  const job = jobs.find((j) => j.id === activeId) ?? jobs[0];
  const [, tick] = useState(0);

  useEffect(() => {
    if (!job || job.status !== "running") return;
    const t = window.setInterval(() => tick((x) => x + 1), 1000);
    return () => window.clearInterval(t);
  }, [job]);

  if (!panelOpen || !job) return null;
  const prompt = job.prompts[0];
  const done = job.stepStatus.filter((s) => s === "success").length;

  return (
    <div className="run-overlay">
      <section className="run-panel">
        <header className="run-head">
          <div className="row gap">
            <h2>{job.title}</h2>
            <JobStatusBadge job={job} />
            <span className="muted small">
              {done}/{job.stepNames.length} Schritte · {duration(job)}
            </span>
          </div>
          <div className="row gap">
            {job.status === "running" && (
              <Button variant="danger" size="sm" icon={<Square size={14} />} onClick={() => cancel(job.id)}>
                Abbrechen
              </Button>
            )}
            <Button variant="ghost" size="sm" icon={<Minimize2 size={14} />} onClick={closePanel}>
              Minimieren
            </Button>
          </div>
        </header>
        <div className="run-body">
          <aside className="run-steps">
            <ol>
              {job.stepNames.map((n, i) => (
                <li key={i} className={`step-${job.stepStatus[i]}`}>
                  <StepIcon status={job.stepStatus[i]} />
                  <span>{n}</span>
                </li>
              ))}
            </ol>
            {job.error && <div className="note note-err small">{job.error}</div>}
            <FailureHints job={job} />
            {jobs.length > 1 && (
              <div className="run-history">
                <div className="row between">
                  <span className="eyebrow">Letzte Jobs</span>
                  <button className="icon-btn" title="Beendete Jobs entfernen" onClick={clearFinished}>
                    <Trash2 size={14} />
                  </button>
                </div>
                {jobs.map((j) => (
                  <button key={j.id} className={`history-item ${j.id === job.id ? "active" : ""}`} onClick={() => openPanel(j.id)}>
                    <StepIcon
                      status={j.status === "running" ? "running" : j.status === "success" ? "success" : j.status === "failed" ? "failed" : "skipped"}
                    />
                    <span className="ellipsis">{j.title}</span>
                  </button>
                ))}
              </div>
            )}
          </aside>
          <div className="run-term">
            {prompt && <PromptCard key={prompt.promptId} prompt={prompt} />}
            <JobTerminal key={job.id} jobId={job.id} interactive={job.status === "running"} />
          </div>
        </div>
      </section>
    </div>
  );
}

export function JobPill() {
  const { jobs, panelOpen, openPanel } = useJobs();
  const running = jobs.filter((j) => j.status === "running");
  if (panelOpen || running.length === 0) return null;
  const j = running[0];
  const idx = j.stepStatus.findIndex((s) => s === "running");
  const needsInput = running.some((r) => r.prompts.length > 0);
  return (
    <button className={`job-pill ${needsInput ? "attention" : ""}`} onClick={() => openPanel(j.id)}>
      <Loader2 size={16} className="spin" />
      <span>
        {j.title}
        {idx >= 0 && ` · ${j.stepNames[idx]}`}
      </span>
      {running.length > 1 && <Badge tone="accent">+{running.length - 1}</Badge>}
    </button>
  );
}
