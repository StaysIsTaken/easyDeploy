import { ChevronDown, ChevronUp, Plus, RotateCcw, Trash2 } from "lucide-react";
import { useState } from "react";
import { buildRecipe, TARGETS_FOR, VAR_LABEL } from "../lib/recipes";
import { useStore } from "../lib/store";
import type { Profile, Step, StepKind } from "../lib/types";
import { uid } from "../lib/util";
import { Button, Field, Modal } from "./ui";

export const STEP_KIND_LABEL: Record<StepKind, string> = {
  shell: "Befehl",
  upload: "Hochladen (SSH)",
  remote: "Befehl auf Server",
  copy: "Kopieren",
  share: "Im Netzwerk teilen",
  open: "Öffnen",
  ghDispatch: "GitHub-Workflow",
};

function stepDetail(s: Step): string {
  switch (s.kind) {
    case "shell":
      return (s.cwd ? `[${s.cwd}] ` : "") + (s.command ?? "");
    case "remote":
      return s.command ?? "";
    case "upload":
      return `${s.source} → ${s.remotePath}${s.excludes?.length ? `  (ohne ${s.excludes.join(", ")})` : ""}`;
    case "copy":
      return `${s.source} → ${s.dest}`;
    case "share":
      return s.source ?? "";
    case "open":
      return s.target ?? "";
    case "ghDispatch":
      return "workflow_dispatch";
  }
}

export function StepSummary({ steps }: { steps: Step[] }) {
  return (
    <ol className="step-summary">
      {steps.map((s) => (
        <li key={s.id}>
          <div className="row gap">
            <strong>{s.name}</strong>
            <span className="badge badge-neutral">{STEP_KIND_LABEL[s.kind]}</span>
          </div>
          <code>{stepDetail(s)}</code>
        </li>
      ))}
    </ol>
  );
}

function StepEditor({
  step,
  onChange,
  onRemove,
  onMove,
}: {
  step: Step;
  onChange: (s: Step) => void;
  onRemove: () => void;
  onMove: (d: -1 | 1) => void;
}) {
  const set = (p: Partial<Step>) => onChange({ ...step, ...p });
  return (
    <div className="step-edit">
      <div className="row gap">
        <input className="input grow" value={step.name} onChange={(e) => set({ name: e.target.value })} />
        <span className="badge badge-neutral">{STEP_KIND_LABEL[step.kind]}</span>
        <button className="icon-btn" onClick={() => onMove(-1)} title="Nach oben">
          <ChevronUp size={16} />
        </button>
        <button className="icon-btn" onClick={() => onMove(1)} title="Nach unten">
          <ChevronDown size={16} />
        </button>
        <button className="icon-btn danger" onClick={onRemove} title="Entfernen">
          <Trash2 size={16} />
        </button>
      </div>
      {(step.kind === "shell" || step.kind === "remote") && (
        <textarea className="input mono" rows={2} value={step.command ?? ""} onChange={(e) => set({ command: e.target.value })} />
      )}
      {step.kind === "shell" && (
        <div className="row gap">
          <input className="input mono small grow" placeholder="Unterordner (optional)" value={step.cwd ?? ""} onChange={(e) => set({ cwd: e.target.value })} />
          <label className="check small">
            <input type="checkbox" checked={!!step.allowFailure} onChange={(e) => set({ allowFailure: e.target.checked })} />
            Fehler ignorieren
          </label>
        </div>
      )}
      {step.kind === "upload" && (
        <>
          <div className="grid-2">
            <input className="input mono small" placeholder="Quelle (relativ zum Projekt)" value={step.source ?? ""} onChange={(e) => set({ source: e.target.value })} />
            <input className="input mono small" placeholder="Remote-Pfad" value={step.remotePath ?? ""} onChange={(e) => set({ remotePath: e.target.value })} />
          </div>
          <div className="row gap">
            <input
              className="input mono small grow"
              placeholder="Ausschließen (Komma-getrennt)"
              value={(step.excludes ?? []).join(", ")}
              onChange={(e) => set({ excludes: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })}
            />
            <label className="check small">
              <input type="checkbox" checked={!!step.clean} onChange={(e) => set({ clean: e.target.checked })} />
              Ziel vorher leeren
            </label>
          </div>
        </>
      )}
      {step.kind === "copy" && (
        <div className="grid-2">
          <input className="input mono small" placeholder="Quelle" value={step.source ?? ""} onChange={(e) => set({ source: e.target.value })} />
          <input className="input mono small" placeholder="Ziel-Ordner" value={step.dest ?? ""} onChange={(e) => set({ dest: e.target.value })} />
        </div>
      )}
      {step.kind === "share" && (
        <input className="input mono small" placeholder="Datei oder Ordner" value={step.source ?? ""} onChange={(e) => set({ source: e.target.value })} />
      )}
      {step.kind === "open" && (
        <input className="input mono small" placeholder="Datei, Ordner, App oder URL" value={step.target ?? ""} onChange={(e) => set({ target: e.target.value })} />
      )}
    </div>
  );
}

export function ProfileEditor({ profile, onClose }: { profile: Profile; onClose: () => void }) {
  const { data, targets, upsertProfile, sys } = useStore();
  const [p, setP] = useState<Profile>(structuredClone(profile));
  const [newVar, setNewVar] = useState("");
  const target = targets.find((t) => t.id === p.targetId);
  const compatible = targets.filter(
    (t) => TARGETS_FOR[p.kind]?.includes(t.kind) && t.kind === target?.kind && (t.kind !== "ios" || sys?.os === "macos"),
  );

  const updateStep = (i: number, s: Step) => setP({ ...p, steps: p.steps.map((x, j) => (j === i ? s : x)) });
  const move = (i: number, d: -1 | 1) => {
    const j = i + d;
    if (j < 0 || j >= p.steps.length) return;
    const steps = [...p.steps];
    [steps[i], steps[j]] = [steps[j], steps[i]];
    setP({ ...p, steps });
  };
  const addStep = (kind: StepKind) => {
    const s: Step = { id: uid(), kind, name: STEP_KIND_LABEL[kind] };
    if (kind === "upload") Object.assign(s, { source: ".", remotePath: "{{remotePath}}" });
    setP({ ...p, steps: [...p.steps, s] });
  };

  const project = data.projects.find((x) => x.id === p.projectId);
  const regenerate = () => {
    const type = project?.types.find((t) => t.kind === p.kind);
    if (!project || !type || !target) return;
    const r = buildRecipe(project, type, target, sys?.os ?? "macos");
    setP({ ...p, steps: r.steps, tools: r.tools, notes: r.notes, vars: { ...r.vars, ...p.vars } });
  };

  const addable: StepKind[] = ["shell", "copy", "share", "open"];
  if (target?.kind === "ssh") addable.push("upload", "remote");

  return (
    <Modal
      title="Deploy-Profil bearbeiten"
      width={760}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Abbrechen</Button>
          <Button
            variant="primary"
            onClick={() => {
              upsertProfile(p);
              onClose();
            }}
          >
            Speichern
          </Button>
        </>
      }
    >
      <div className="form">
        <div className="grid-2">
          <Field label="Name">
            <input className="input" value={p.name} onChange={(e) => setP({ ...p, name: e.target.value })} />
          </Field>
          <Field label="Ziel">
            <select className="input" value={p.targetId} onChange={(e) => setP({ ...p, targetId: e.target.value })}>
              {compatible.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
          </Field>
        </div>

        <div>
          <span className="eyebrow">Variablen</span>
          <p className="small muted">
            In Schritten als <code>{"{{name}}"}</code> nutzbar. Automatisch verfügbar: <code>{"{{device}}"}</code>, <code>{"{{host}}"}</code>,{" "}
            <code>{"{{user}}"}</code>, <code>{"{{projectDir}}"}</code>, <code>{"{{dest}}"}</code>.
          </p>
          <div className="grid-2">
            {Object.entries(p.vars).map(([k, v]) => (
              <Field key={k} label={VAR_LABEL[k] ? `${VAR_LABEL[k]} (${k})` : k}>
                <div className="row gap">
                  <input className="input mono grow" value={v} onChange={(e) => setP({ ...p, vars: { ...p.vars, [k]: e.target.value } })} />
                  <button
                    className="icon-btn danger"
                    onClick={() => {
                      const vars = { ...p.vars };
                      delete vars[k];
                      setP({ ...p, vars });
                    }}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </Field>
            ))}
          </div>
          <form
            className="row gap"
            onSubmit={(e) => {
              e.preventDefault();
              const k = newVar.trim().replace(/[^\w.-]/g, "");
              if (k) setP({ ...p, vars: { ...p.vars, [k]: "" } });
              setNewVar("");
            }}
          >
            <input className="input small mono" placeholder="neue Variable" value={newVar} onChange={(e) => setNewVar(e.target.value)} />
            <Button size="sm" type="submit" icon={<Plus size={14} />}>
              Hinzufügen
            </Button>
          </form>
        </div>

        <div>
          <div className="row between">
            <span className="eyebrow">Schritte</span>
            <Button size="sm" variant="ghost" icon={<RotateCcw size={14} />} onClick={regenerate} title="Schritte aus dem aktuellen Rezept neu erzeugen">
              Schritte neu erzeugen
            </Button>
          </div>
          <div className="step-list">
            {p.steps.map((s, i) => (
              <StepEditor
                key={s.id}
                step={s}
                onChange={(n) => updateStep(i, n)}
                onRemove={() => setP({ ...p, steps: p.steps.filter((_, j) => j !== i) })}
                onMove={(d) => move(i, d)}
              />
            ))}
          </div>
          <div className="row gap wrap">
            {addable.map((k) => (
              <Button key={k} size="sm" variant="ghost" icon={<Plus size={14} />} onClick={() => addStep(k)}>
                {STEP_KIND_LABEL[k]}
              </Button>
            ))}
          </div>
        </div>
      </div>
    </Modal>
  );
}
