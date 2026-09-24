import { Info, Plus } from "lucide-react";
import { useMemo, useState } from "react";
import { buildRecipe, TARGET_LABEL, TARGETS_FOR, VAR_LABEL, type Recipe } from "../lib/recipes";
import { useStore } from "../lib/store";
import type { DetectedType, Profile, Project, Target, TargetKind } from "../lib/types";
import { uid } from "../lib/util";
import { KindIcon, TargetIcon } from "./icons";
import { StepSummary } from "./ProfileEditor";
import { TargetForm } from "./TargetForm";
import { Badge, Button, Field, Modal } from "./ui";

export function ProfileWizard({ project, onClose, onCreated }: { project: Project; onClose: () => void; onCreated?: (p: Profile) => void }) {
  const { targets, sys, upsertProfile } = useStore();
  const [type, setType] = useState<DetectedType | null>(project.types.length === 1 ? project.types[0] : null);
  const [target, setTarget] = useState<Target | null>(null);
  const [newKind, setNewKind] = useState<TargetKind | null>(null);
  const [recipe, setRecipe] = useState<Recipe | null>(null);

  const allowed = useMemo(() => (type ? TARGETS_FOR[type.kind] ?? [] : []), [type]);
  const usable = targets.filter((t) => allowed.includes(t.kind) && (t.kind !== "ios" || sys?.os === "macos"));
  const creatable = allowed.filter((k) => k !== "local" && k !== "share" && (k !== "ios" || sys?.os === "macos"));

  const choose = (t: Target) => {
    setTarget(t);
    if (type) setRecipe(buildRecipe(project, type, t, sys?.os ?? "macos"));
  };

  const save = () => {
    if (!recipe || !target || !type) return;
    const p: Profile = {
      id: uid(),
      projectId: project.id,
      targetId: target.id,
      name: recipe.name,
      kind: type.kind,
      steps: recipe.steps,
      vars: recipe.vars,
      tools: recipe.tools,
      notes: recipe.notes,
    };
    upsertProfile(p);
    onCreated?.(p);
    onClose();
  };

  const stage = !type ? 1 : !recipe ? 2 : 3;

  return (
    <Modal
      title="Neues Deploy-Ziel"
      subtitle={project.name}
      width={720}
      onClose={onClose}
      footer={
        stage === 3 ? (
          <>
            <Button
              onClick={() => {
                setRecipe(null);
                setTarget(null);
              }}
            >
              Zurück
            </Button>
            <Button variant="primary" onClick={save}>
              Speichern
            </Button>
          </>
        ) : (
          <Button onClick={onClose}>Abbrechen</Button>
        )
      }
    >
      <ol className="wizard-steps">
        <li className={stage >= 1 ? "active" : ""}>Projekttyp</li>
        <li className={stage >= 2 ? "active" : ""}>Ziel</li>
        <li className={stage >= 3 ? "active" : ""}>Überprüfen</li>
      </ol>

      {stage === 1 && (
        <div className="choice-grid">
          {project.types.map((t) => (
            <button key={t.kind} className="choice" onClick={() => setType(t)}>
              <KindIcon kind={t.kind} size={22} />
              <strong>{t.label}</strong>
            </button>
          ))}
        </div>
      )}

      {stage === 2 && type && (
        <>
          <p className="muted">
            Wohin soll <strong>{type.label}</strong> deployt werden?
          </p>
          <div className="choice-grid">
            {usable.map((t) => (
              <button key={t.id} className="choice" onClick={() => choose(t)}>
                <TargetIcon kind={t.kind} size={22} />
                <strong>{t.name}</strong>
                <span className="small muted">{TARGET_LABEL[t.kind]}</span>
              </button>
            ))}
            {creatable.map((k) => (
              <button key={k} className="choice dashed" onClick={() => setNewKind(k)}>
                <Plus size={22} />
                <strong>{TARGET_LABEL[k]}</strong>
                <span className="small muted">neu anlegen</span>
              </button>
            ))}
          </div>
          {project.types.length > 1 && (
            <button className="link small" onClick={() => setType(null)}>
              ← anderen Projekttyp wählen
            </button>
          )}
        </>
      )}

      {stage === 3 && recipe && target && (
        <div className="form">
          <Field label="Name">
            <input className="input" value={recipe.name} onChange={(e) => setRecipe({ ...recipe, name: e.target.value })} />
          </Field>
          {Object.keys(recipe.vars).length > 0 && (
            <div className="grid-2">
              {Object.entries(recipe.vars).map(([k, v]) => (
                <Field key={k} label={VAR_LABEL[k] ?? k}>
                  <input
                    className="input mono"
                    value={v}
                    onChange={(e) => setRecipe({ ...recipe, vars: { ...recipe.vars, [k]: e.target.value } })}
                  />
                </Field>
              ))}
            </div>
          )}
          <div>
            <span className="eyebrow">Schritte</span>
            <StepSummary steps={recipe.steps} />
            <p className="small muted">Alle Schritte kannst du später unter „Bearbeiten“ anpassen.</p>
          </div>
          <div className="row gap wrap">
            <span className="eyebrow">Benötigt</span>
            {recipe.tools.map((t) => (
              <Badge key={t}>{t}</Badge>
            ))}
          </div>
          {recipe.notes.map((n, i) => (
            <div key={i} className="note small">
              <Info size={14} /> {n}
            </div>
          ))}
        </div>
      )}

      {newKind && (
        <TargetForm
          initial={{ kind: newKind }}
          allowedKinds={[newKind]}
          onClose={() => setNewKind(null)}
          onSaved={(t) => choose(t)}
        />
      )}
    </Modal>
  );
}
