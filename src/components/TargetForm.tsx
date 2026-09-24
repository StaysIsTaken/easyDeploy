import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { api, errorText, sshSecretKey } from "../lib/api";
import { TARGET_LABEL } from "../lib/recipes";
import { useStore } from "../lib/store";
import type { Device, GhRepo, GhWorkflow, Target, TargetKind } from "../lib/types";
import { uid } from "../lib/util";
import { TargetIcon } from "./icons";
import { Button, ErrorNote, Field, Modal } from "./ui";

const EDITABLE_KINDS: TargetKind[] = ["ssh", "android", "ios", "folder", "github"];

export function TargetForm({
  initial,
  allowedKinds,
  onClose,
  onSaved,
}: {
  initial?: Partial<Target>;
  allowedKinds?: TargetKind[];
  onClose: () => void;
  onSaved?: (t: Target) => void;
}) {
  const { upsertTarget, sys } = useStore();
  const kinds = EDITABLE_KINDS.filter((k) => (!allowedKinds || allowedKinds.includes(k)) && (k !== "ios" || sys?.os === "macos"));
  const [t, setT] = useState<Target>(() => ({
    id: initial?.id ?? uid(),
    name: initial?.name ?? "",
    kind: initial?.kind ?? kinds[0] ?? "ssh",
    ssh: initial?.ssh ?? { host: "", port: 22, user: "", auth: "agent" },
    device: initial?.device,
    folder: initial?.folder,
    github: initial?.github,
  }));
  const [password, setPassword] = useState("");
  const [hasPassword, setHasPassword] = useState(false);
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [loadingDevices, setLoadingDevices] = useState(false);
  const [repos, setRepos] = useState<GhRepo[] | null>(null);
  const [workflows, setWorkflows] = useState<GhWorkflow[] | null>(null);
  const [error, setError] = useState("");
  const isNew = !initial?.id;

  useEffect(() => {
    if (initial?.id) api.secretHas(sshSecretKey(initial.id)).then(setHasPassword).catch(() => {});
  }, [initial?.id]);

  const loadDevices = () => {
    setLoadingDevices(true);
    api
      .listDevices(false)
      .then(setDevices)
      .catch((e) => setError(errorText(e)))
      .finally(() => setLoadingDevices(false));
  };

  useEffect(() => {
    if ((t.kind === "android" || t.kind === "ios") && devices === null) loadDevices();
    if (t.kind === "github" && repos === null) api.ghRepos().then(setRepos).catch(() => setRepos([]));
  }, [t.kind]);

  useEffect(() => {
    const g = t.github;
    if (t.kind !== "github" || !g?.owner || !g.repo) return;
    setWorkflows(null);
    api.ghWorkflows(g.owner, g.repo).then(setWorkflows).catch(() => setWorkflows([]));
  }, [t.kind, t.github?.owner, t.github?.repo]);

  const set = (patch: Partial<Target>) => setT((x) => ({ ...x, ...patch }));
  const setSsh = (patch: Partial<NonNullable<Target["ssh"]>>) => setT((x) => ({ ...x, ssh: { ...x.ssh!, ...patch } }));

  const save = async () => {
    setError("");
    const out: Target = { id: t.id, name: t.name.trim(), kind: t.kind };
    if (t.kind === "ssh") {
      if (!t.ssh?.host) return setError("Bitte Host bzw. IP-Adresse angeben.");
      out.ssh = { ...t.ssh, port: Number(t.ssh.port) || 22 };
      out.name ||= `${t.ssh.user ? t.ssh.user + "@" : ""}${t.ssh.host}`;
      if (t.ssh.auth === "password" && password) {
        try {
          await api.secretSet(sshSecretKey(t.id), password);
        } catch (e) {
          return setError(`Passwort konnte nicht im Schlüsselbund gespeichert werden: ${errorText(e)}`);
        }
      }
      if (t.ssh.auth !== "password") api.secretDelete(sshSecretKey(t.id)).catch(() => {});
    }
    if (t.kind === "android" || t.kind === "ios") {
      if (!t.device?.id) return setError("Bitte ein Gerät auswählen.");
      out.device = t.device;
      out.name ||= t.device.model ?? t.device.id;
    }
    if (t.kind === "folder") {
      if (!t.folder?.path) return setError("Bitte einen Ordner wählen.");
      out.folder = t.folder;
      out.name ||= t.folder.path.split(/[\\/]/).filter(Boolean).pop() ?? "Ordner";
    }
    if (t.kind === "github") {
      if (!t.github?.repo || !t.github.workflow) return setError("Bitte Repository und Workflow wählen.");
      out.github = t.github;
      out.name ||= `${t.github.repo} · ${t.github.workflow}`;
    }
    upsertTarget(out);
    onSaved?.(out);
    onClose();
  };

  const pickFolder = async () => {
    const p = await open({ directory: true, title: "Ziel-Ordner wählen (z. B. USB-Stick)" });
    if (typeof p === "string") set({ folder: { path: p } });
  };

  const pickKey = async () => {
    const p = await open({ title: "SSH-Key wählen", defaultPath: sys ? `${sys.home}/.ssh` : undefined });
    if (typeof p === "string") setSsh({ keyPath: p, auth: "key" });
  };

  const deviceList = (devices ?? []).filter((d) => d.kind === t.kind);

  return (
    <Modal
      title={isNew ? "Neues Ziel" : "Ziel bearbeiten"}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Abbrechen</Button>
          <Button variant="primary" onClick={save}>
            Speichern
          </Button>
        </>
      }
    >
      {isNew && kinds.length > 1 && (
        <div className="kind-picker">
          {kinds.map((k) => (
            <button key={k} className={`kind-option ${t.kind === k ? "active" : ""}`} onClick={() => set({ kind: k })}>
              <TargetIcon kind={k} size={20} />
              <span>{TARGET_LABEL[k]}</span>
            </button>
          ))}
        </div>
      )}

      <div className="form">
        <Field label="Name" hint="Optional – wird sonst automatisch vergeben.">
          <input className="input" value={t.name} onChange={(e) => set({ name: e.target.value })} placeholder="z. B. Produktiv-Server" />
        </Field>

        {t.kind === "ssh" && t.ssh && (
          <>
            <div className="grid-2">
              <Field label="Host / IP-Adresse">
                <input className="input" value={t.ssh.host} onChange={(e) => setSsh({ host: e.target.value.trim() })} placeholder="192.168.1.20 oder server.de" />
              </Field>
              <div className="grid-2">
                <Field label="Port">
                  <input className="input" type="number" value={t.ssh.port} onChange={(e) => setSsh({ port: Number(e.target.value) })} />
                </Field>
                <Field label="Benutzer">
                  <input className="input" value={t.ssh.user} onChange={(e) => setSsh({ user: e.target.value.trim() })} placeholder="root" />
                </Field>
              </div>
            </div>
            <Field label="Anmeldung">
              <div className="segmented">
                {(
                  [
                    ["agent", "Standard-Key / Agent"],
                    ["key", "Bestimmter Key"],
                    ["password", "Passwort"],
                  ] as const
                ).map(([k, l]) => (
                  <button key={k} className={t.ssh!.auth === k ? "active" : ""} onClick={() => setSsh({ auth: k })}>
                    {l}
                  </button>
                ))}
              </div>
            </Field>
            {t.ssh.auth === "key" && (
              <Field label="Key-Datei">
                <div className="row gap">
                  <input className="input grow mono" value={t.ssh.keyPath ?? ""} onChange={(e) => setSsh({ keyPath: e.target.value })} placeholder="~/.ssh/id_ed25519" />
                  <Button icon={<FolderOpen size={16} />} onClick={pickKey}>
                    Auswählen
                  </Button>
                </div>
              </Field>
            )}
            {t.ssh.auth === "password" && (
              <Field
                label="Passwort"
                hint={
                  hasPassword
                    ? "Ein Passwort ist im Schlüsselbund gespeichert. Leer lassen, um es zu behalten."
                    : "Wird sicher im Schlüsselbund des Systems gespeichert. Leer lassen, um bei jedem Deploy gefragt zu werden."
                }
              >
                <input className="input" type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder={hasPassword ? "••••••••" : ""} />
              </Field>
            )}
          </>
        )}

        {(t.kind === "android" || t.kind === "ios") && (
          <Field
            label="Gerät"
            hint={t.kind === "android" ? "Per USB anschließen und USB-Debugging aktivieren – oder per WLAN-Debugging koppeln." : "Per Kabel anschließen, entsperren und dem Mac vertrauen. Entwicklermodus muss aktiv sein."}
          >
            <div className="device-pick">
              {deviceList.map((d) => (
                <button
                  key={d.id}
                  className={`device-option ${t.device?.id === d.id ? "active" : ""}`}
                  onClick={() => set({ device: { id: d.id, model: d.name } })}
                >
                  <TargetIcon kind={d.kind} />
                  <div>
                    <strong>{d.name}</strong>
                    <span className="small muted mono">{d.id}</span>
                    {d.hint && <span className="small c-warn">{d.hint}</span>}
                  </div>
                </button>
              ))}
              {devices && deviceList.length === 0 && <p className="muted small">Kein Gerät gefunden.</p>}
              <div className="row gap">
                <Button size="sm" variant="ghost" icon={<RefreshCw size={14} />} loading={loadingDevices} onClick={loadDevices}>
                  Suchen
                </Button>
                <input
                  className="input grow mono small"
                  placeholder={t.kind === "android" ? "oder Seriennummer / IP:Port eingeben" : "oder UDID eingeben"}
                  value={t.device?.id ?? ""}
                  onChange={(e) => set({ device: { id: e.target.value.trim(), model: t.device?.model } })}
                />
              </div>
            </div>
          </Field>
        )}

        {t.kind === "folder" && (
          <Field label="Ordner" hint="USB-Stick, externe Festplatte, Netzlaufwerk oder freigegebener Ordner eines anderen PCs.">
            <div className="row gap">
              <input className="input grow mono" value={t.folder?.path ?? ""} onChange={(e) => set({ folder: { path: e.target.value } })} />
              <Button icon={<FolderOpen size={16} />} onClick={pickFolder}>
                Wählen
              </Button>
            </div>
          </Field>
        )}

        {t.kind === "github" && (
          <>
            {repos && repos.length === 0 && <p className="note small">Verbinde zuerst GitHub (Seite „GitHub“), um Repositories auszuwählen.</p>}
            <div className="grid-2">
              <Field label="Repository">
                <select
                  className="input"
                  value={t.github ? `${t.github.owner}/${t.github.repo}` : ""}
                  onChange={(e) => {
                    const r = repos?.find((x) => x.fullName === e.target.value);
                    if (r) set({ github: { owner: r.owner, repo: r.name, workflow: "", ref: r.defaultBranch } });
                  }}
                >
                  <option value="">– wählen –</option>
                  {repos?.map((r) => (
                    <option key={r.fullName} value={r.fullName}>
                      {r.fullName}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="Branch / Ref">
                <input
                  className="input"
                  value={t.github?.ref ?? ""}
                  onChange={(e) => t.github && set({ github: { ...t.github, ref: e.target.value } })}
                />
              </Field>
            </div>
            <Field label="Workflow" hint="Der Workflow braucht den Trigger „workflow_dispatch“.">
              <select
                className="input"
                value={t.github?.workflow ?? ""}
                onChange={(e) => t.github && set({ github: { ...t.github, workflow: e.target.value } })}
              >
                <option value="">– wählen –</option>
                {workflows?.map((w) => (
                  <option key={w.id} value={w.file}>
                    {w.name} ({w.file})
                  </option>
                ))}
              </select>
            </Field>
          </>
        )}
        <ErrorNote>{error}</ErrorNote>
      </div>
    </Modal>
  );
}
