import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { Copy, FilePlus, FolderPlus, QrCode, Square } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Empty, ErrorNote, PageHeader } from "../components/ui";
import { api, errorText } from "../lib/api";
import type { ShareInfo } from "../lib/types";
import { humanSize } from "../lib/util";

function ShareCard({ share }: { share: ShareInfo }) {
  const [urlIndex, setUrlIndex] = useState(0);
  const [copied, setCopied] = useState(false);
  const url = share.urls[urlIndex];
  const total = share.files.reduce((a, f) => a + f.size, 0);

  return (
    <article className="card share-card">
      <div className="row between">
        <div className="min0">
          <h3 className="ellipsis">{share.title}</h3>
          <p className="small muted mono ellipsis">{share.path}</p>
        </div>
        <Button size="sm" variant="danger" icon={<Square size={14} />} onClick={() => api.shareStop(share.id)}>
          Beenden
        </Button>
      </div>
      <div className="share-body">
        {url ? (
          <div className="qr" dangerouslySetInnerHTML={{ __html: url.qrSvg }} />
        ) : (
          <div className="note note-warn small">Keine Netzwerkverbindung gefunden.</div>
        )}
        <div className="stack grow min0">
          <p className="small">Auf dem Handy den QR-Code scannen oder auf dem anderen Laptop den Link öffnen:</p>
          {url && (
            <div className="row gap">
              <code className="url ellipsis">{url.url}</code>
              <button
                className="icon-btn"
                title="Kopieren"
                onClick={() => {
                  navigator.clipboard.writeText(url.url);
                  setCopied(true);
                  window.setTimeout(() => setCopied(false), 1500);
                }}
              >
                <Copy size={14} />
              </button>
              {copied && <span className="small c-ok">kopiert</span>}
            </div>
          )}
          {share.urls.length > 1 && (
            <div className="row gap wrap">
              {share.urls.map((u, i) => (
                <button key={u.url} className={`chip ${i === urlIndex ? "active" : ""}`} onClick={() => setUrlIndex(i)}>
                  {u.interface}
                  {u.url.includes("//169.254.") && " (Direktkabel)"}
                </button>
              ))}
            </div>
          )}
          <div className="row gap wrap">
            <Badge>
              {share.files.length} Datei{share.files.length === 1 ? "" : "en"}
            </Badge>
            <Badge>{humanSize(total)}</Badge>
          </div>
          <p className="small muted">Beide Geräte müssen im selben Netzwerk sein (oder per Kabel direkt verbunden).</p>
        </div>
      </div>
    </article>
  );
}

export function SharesPage() {
  const [shares, setShares] = useState<ShareInfo[]>([]);
  const [error, setError] = useState("");

  const load = useCallback(() => {
    api.shareList().then(setShares).catch((e) => setError(errorText(e)));
  }, []);

  useEffect(() => {
    load();
    const un = listen("share://changed", load);
    return () => {
      un.then((u) => u());
    };
  }, [load]);

  const start = async (directory: boolean) => {
    setError("");
    const p = await open({ directory, title: directory ? "Ordner freigeben" : "Datei freigeben" });
    if (typeof p !== "string") return;
    try {
      await api.shareStart(p);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="page">
      <PageHeader
        title="Freigaben"
        subtitle="Builds, Installer oder APKs per Link und QR-Code an andere Geräte im Netzwerk weitergeben."
        actions={
          <>
            <Button icon={<FilePlus size={16} />} onClick={() => start(false)}>
              Datei freigeben
            </Button>
            <Button icon={<FolderPlus size={16} />} onClick={() => start(true)}>
              Ordner freigeben
            </Button>
          </>
        }
      />
      <ErrorNote>{error}</ErrorNote>
      {shares.length === 0 ? (
        <Empty icon={<QrCode size={32} />} title="Keine aktiven Freigaben">
          <p className="muted">Deploy-Ziele vom Typ „Download im Netzwerk“ erscheinen hier automatisch.</p>
        </Empty>
      ) : (
        <div className="card-grid wide">
          {shares.map((s) => (
            <ShareCard key={s.id} share={s} />
          ))}
        </div>
      )}
    </div>
  );
}
