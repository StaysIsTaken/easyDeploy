import { confirm } from "@tauri-apps/plugin-dialog";
import { Cable, Pencil, PlugZap, Plus, RefreshCw, Trash2, Usb, Wifi } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { TargetIcon } from "../components/icons";
import { TargetForm } from "../components/TargetForm";
import { Badge, Button, ErrorNote, PageHeader, Spinner } from "../components/ui";
import { api, errorText, sshSecretKey } from "../lib/api";
import { sshConn } from "../lib/deploy";
import { useJobs } from "../lib/jobs";
import { TARGET_LABEL } from "../lib/recipes";
import { useStore } from "../lib/store";
import type { Device, Target } from "../lib/types";
import { uid } from "../lib/util";

function ConnectionIcon({ c }: { c: string }) {
  if (c === "usb") return <Usb size={14} />;
  if (c === "wlan" || c === "lan") return <Wifi size={14} />;
  if (c === "kabel") return <Cable size={14} />;
  return null;
}

const CONNECTION_LABEL: Record<string, string> = {
  usb: "USB",
  wlan: "WLAN",
  lan: "Netzwerk",
  kabel: "Direktkabel",
  emulator: "Emulator",
  ssh: "ssh-config",
  offline: "offline",
};

export function TargetsPage() {
  const { data, upsertTarget, removeTarget, sys } = useStore();
  const { run } = useJobs();
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [editing, setEditing] = useState<Partial<Target> | null>(null);
  const [error, setError] = useState("");

  const scan = useCallback(() => {
    setScanning(true);
    setError("");
    api
      .listDevices(true)
      .then(setDevices)
      .catch((e) => setError(errorText(e)))
      .finally(() => setScanning(false));
  }, []);

  useEffect(scan, [scan]);

  const testSsh = (t: Target) =>
    run({
      title: `Verbindung testen: ${t.name}`,
      steps: [{ kind: "remote", name: "SSH-Verbindung", command: 'echo "Verbindung OK: $(hostname) – $(uname -sr)"', ssh: sshConn(t) }],
    });

  const known = (d: Device) =>
    data.targets.some((t) => t.device?.id === d.id || (t.ssh && (t.ssh.host === d.id || t.ssh.host === d.details.address || t.ssh.host === d.details.hostname)));

  const adopt = (d: Device) => {
    if (d.kind === "android" || d.kind === "ios") {
      upsertTarget({ id: uid(), name: d.name, kind: d.kind, device: { id: d.id, model: d.name } });
    } else if (d.kind === "network") {
      setEditing({
        kind: "ssh",
        name: d.name,
        ssh: { host: d.details.address ?? d.id, port: Number(d.details.port) || 22, user: "", auth: "agent" },
      });
    } else {
      setEditing({
        kind: "ssh",
        name: d.name,
        ssh: {
          host: d.id,
          port: Number(d.details.port) || 22,
          user: d.details.user ?? "",
          auth: d.details.identityfile ? "key" : "agent",
          keyPath: d.details.identityfile,
        },
      });
    }
  };

  const remove = async (t: Target) => {
    const used = data.profiles.filter((p) => p.targetId === t.id).length;
    const ok = await confirm(
      used ? `„${t.name}“ wird von ${used} Deploy-Profil(en) genutzt, die ebenfalls gelöscht werden. Fortfahren?` : `„${t.name}“ löschen?`,
      { title: "Ziel löschen", kind: "warning" },
    );
    if (!ok) return;
    removeTarget(t.id);
    api.secretDelete(sshSecretKey(t.id)).catch(() => {});
  };

  const groups: { title: string; hint: string; items: Device[] }[] = devices
    ? [
        {
          title: "Handys & Tablets",
          hint: sys?.os === "macos" ? "Android per adb, iPhone/iPad per Xcode" : "Android per adb (iOS nur auf dem Mac)",
          items: devices.filter((d) => d.kind === "android" || d.kind === "ios"),
        },
        {
          title: "Computer im Netzwerk",
          hint: "Rechner mit aktivem SSH – auch Laptops, die direkt per USB-C/Thunderbolt-Kabel verbunden sind",
          items: devices.filter((d) => d.kind === "network"),
        },
        { title: "Aus ~/.ssh/config", hint: "Bereits eingerichtete SSH-Hosts", items: devices.filter((d) => d.kind === "ssh-config") },
      ]
    : [];

  return (
    <div className="page">
      <PageHeader
        title="Geräte & Ziele"
        subtitle="Wohin deployt werden kann: Server, Handys, Tablets, andere PCs, Laufwerke oder GitHub."
        actions={
          <Button variant="primary" icon={<Plus size={16} />} onClick={() => setEditing({})}>
            Ziel hinzufügen
          </Button>
        }
      />
      <ErrorNote>{error}</ErrorNote>

      <section>
        <h2>Gespeicherte Ziele</h2>
        <div className="list">
          <div className="list-item">
            <TargetIcon kind="local" />
            <div className="grow">
              <strong>Dieser PC</strong>
              <p className="small muted">
                {sys?.hostname} · {sys?.os} {sys?.arch}
              </p>
            </div>
            <Badge>immer verfügbar</Badge>
          </div>
          <div className="list-item">
            <TargetIcon kind="share" />
            <div className="grow">
              <strong>Download im Netzwerk (QR)</strong>
              <p className="small muted">Stellt den Build per Link & QR-Code bereit – ideal für Laptops, Handys und Tablets ohne Kabel.</p>
            </div>
            <Badge>immer verfügbar</Badge>
          </div>
          {data.targets.map((t) => (
            <div key={t.id} className="list-item">
              <TargetIcon kind={t.kind} />
              <div className="grow min0">
                <strong>{t.name}</strong>
                <p className="small muted mono ellipsis">
                  {TARGET_LABEL[t.kind]}
                  {t.ssh && ` · ${t.ssh.user ? t.ssh.user + "@" : ""}${t.ssh.host}:${t.ssh.port}`}
                  {t.device && ` · ${t.device.id}`}
                  {t.folder && ` · ${t.folder.path}`}
                  {t.github && ` · ${t.github.owner}/${t.github.repo} · ${t.github.workflow}`}
                </p>
              </div>
              {t.kind === "ssh" && (
                <Button size="sm" variant="ghost" icon={<PlugZap size={14} />} onClick={() => testSsh(t)}>
                  Testen
                </Button>
              )}
              <button className="icon-btn" title="Bearbeiten" onClick={() => setEditing(t)}>
                <Pencil size={16} />
              </button>
              <button className="icon-btn danger" title="Löschen" onClick={() => remove(t)}>
                <Trash2 size={16} />
              </button>
            </div>
          ))}
        </div>
      </section>

      <section>
        <div className="section-head">
          <h2>Erkannte Geräte</h2>
          <Button size="sm" variant="ghost" icon={<RefreshCw size={14} />} loading={scanning} onClick={scan}>
            Neu suchen
          </Button>
        </div>
        {!devices && scanning && <Spinner label="Suche nach Geräten (USB, WLAN, Netzwerk) …" />}
        {groups.map((g) => (
          <div key={g.title} className="device-group">
            <div className="row gap">
              <h3>{g.title}</h3>
              <span className="small muted">{g.hint}</span>
            </div>
            {g.items.length === 0 ? (
              <p className="small muted">Nichts gefunden.</p>
            ) : (
              <div className="card-grid">
                {g.items.map((d) => (
                  <div key={d.kind + d.id} className="card device-card">
                    <div className="row gap">
                      <div className="kind-icon">
                        <TargetIcon kind={d.kind === "ssh-config" ? "ssh" : d.kind} />
                      </div>
                      <div className="grow min0">
                        <strong className="ellipsis">{d.name}</strong>
                        <p className="small muted mono ellipsis">{d.details.address ?? d.details.hostname ?? d.id}</p>
                      </div>
                      <Badge tone={d.state === "device" ? "ok" : "warn"}>
                        <ConnectionIcon c={d.connection} /> {CONNECTION_LABEL[d.connection] ?? d.connection}
                      </Badge>
                    </div>
                    {d.details.osVersion && <p className="small muted">iOS {d.details.osVersion}</p>}
                    {d.hint && <div className="note note-warn small">{d.hint}</div>}
                    <div className="row end">
                      {known(d) ? (
                        <Badge tone="ok">gespeichert</Badge>
                      ) : (
                        <Button size="sm" icon={<Plus size={14} />} onClick={() => adopt(d)}>
                          Als Ziel speichern
                        </Button>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </section>

      {editing && <TargetForm initial={editing} onClose={() => setEditing(null)} />}
    </div>
  );
}
