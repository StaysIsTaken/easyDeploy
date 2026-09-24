import { Rocket, Smartphone } from "lucide-react";
import { createContext, useCallback, useContext, useState, type ReactNode } from "react";
import { api, errorText } from "../lib/api";
import { missingVars, resolveSteps } from "../lib/deploy";
import { useJobs } from "../lib/jobs";
import { useStore } from "../lib/store";
import type { Profile, Project, Target, ToolStatus } from "../lib/types";
import { RequirementsList } from "./RequirementsList";
import { Button, ErrorNote, Modal, Spinner } from "./ui";

interface Preflight {
  profile: Profile;
  project: Project;
  target: Target;
  checking: boolean;
  tools: ToolStatus[] | null;
  deviceIssue: string | null;
  missing: string[];
  error: string;
}

const Ctx = createContext<{ deploy: (profileId: string) => void } | null>(null);

async function checkDevice(target: Target): Promise<string | null> {
  if (target.kind !== "android" && target.kind !== "ios") return null;
  const devices = await api.listDevices(false);
  const d = devices.find((x) => x.id === target.device?.id);
  if (!d) {
    return target.kind === "android"
      ? `„${target.name}“ ist nicht verbunden. Schließe das Gerät per USB an (USB-Debugging an) oder verbinde es per WLAN-Debugging.`
      : `„${target.name}“ ist nicht verbunden. Schließe das Gerät per Kabel an und entsperre es.`;
  }
  if (d.state !== "device") return d.hint ?? `Gerät meldet Status „${d.state}“.`;
  return null;
}

export function DeployProvider({ children }: { children: ReactNode }) {
  const { data, targets, markRun } = useStore();
  const { run } = useJobs();
  const [pf, setPf] = useState<Preflight | null>(null);

  const start = useCallback(
    async (profile: Profile, project: Project, target: Target) => {
      setPf(null);
      let steps;
      try {
        steps = resolveSteps(profile, project, target);
      } catch (e) {
        setPf({ profile, project, target, checking: false, tools: null, deviceIssue: null, missing: [], error: errorText(e) });
        return;
      }
      const r = await run({ title: `${project.name} → ${target.name}`, kind: "deploy", profileId: profile.id, baseDir: project.path, steps });
      markRun(profile.id, r.success);
    },
    [run, markRun],
  );

  const check = useCallback(
    async (profile: Profile, project: Project, target: Target, autoStart: boolean) => {
      const missing = missingVars(profile, project, target);
      setPf({ profile, project, target, checking: true, tools: null, deviceIssue: null, missing, error: "" });
      try {
        const [tools, deviceIssue] = await Promise.all([api.checkTools(profile.tools, project.constraints), checkDevice(target)]);
        const blocking = tools.some((t) => !t.ok && !t.optional) || !!deviceIssue || missing.length > 0;
        if (!blocking && autoStart) {
          start(profile, project, target);
          return;
        }
        setPf({ profile, project, target, checking: false, tools, deviceIssue, missing, error: "" });
      } catch (e) {
        setPf({ profile, project, target, checking: false, tools: null, deviceIssue: null, missing, error: errorText(e) });
      }
    },
    [start],
  );

  const deploy = useCallback(
    (profileId: string) => {
      const profile = data.profiles.find((p) => p.id === profileId);
      const project = profile && data.projects.find((p) => p.id === profile.projectId);
      const target = profile && targets.find((t) => t.id === profile.targetId);
      if (!profile || !project || !target) return;
      check(profile, project, target, true);
    },
    [data, targets, check],
  );

  const problems = pf?.tools?.filter((t) => !t.ok) ?? [];

  return (
    <Ctx.Provider value={{ deploy }}>
      {children}
      {pf && (
        <Modal
          title={pf.checking ? "Prüfe Voraussetzungen …" : "Vor dem Deploy"}
          subtitle={`${pf.project.name} → ${pf.target.name}`}
          onClose={() => setPf(null)}
          footer={
            !pf.checking && (
              <>
                <Button onClick={() => setPf(null)}>Abbrechen</Button>
                {!pf.error && (
                  <Button variant="primary" icon={<Rocket size={16} />} onClick={() => start(pf.profile, pf.project, pf.target)}>
                    Trotzdem deployen
                  </Button>
                )}
              </>
            )
          }
        >
          {pf.checking && <Spinner label="Tools, Versionen und Geräte werden geprüft …" />}
          <ErrorNote>{pf.error}</ErrorNote>
          {!pf.checking && (
            <div className="form">
              {pf.deviceIssue && (
                <div className="note note-warn">
                  <Smartphone size={16} />
                  <div>
                    {pf.deviceIssue}
                    <div>
                      <Button size="sm" variant="ghost" onClick={() => check(pf.profile, pf.project, pf.target, false)}>
                        Erneut suchen
                      </Button>
                    </div>
                  </div>
                </div>
              )}
              {pf.missing.length > 0 && (
                <div className="note note-warn">
                  Nicht gesetzte Variablen: {pf.missing.map((m) => `{{${m}}}`).join(", ")} – bitte im Profil unter „Bearbeiten“ ergänzen.
                </div>
              )}
              {problems.length > 0 && (
                <>
                  <p>Für dieses Deploy fehlt noch etwas. Du kannst es direkt hier installieren:</p>
                  <RequirementsList
                    statuses={problems}
                    projectDir={pf.project.path}
                    onRecheck={() => check(pf.profile, pf.project, pf.target, false)}
                  />
                </>
              )}
              {problems.length === 0 && !pf.deviceIssue && pf.missing.length === 0 && !pf.error && <p>Alles bereit.</p>}
            </div>
          )}
        </Modal>
      )}
    </Ctx.Provider>
  );
}

export function useDeploy() {
  const c = useContext(Ctx);
  if (!c) throw new Error("DeployProvider fehlt");
  return c;
}
