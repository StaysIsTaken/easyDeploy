//! LAN download shares: serves a build artifact (APK, installer, zip, folder)
//! over HTTP so any device in the same network – or a laptop plugged in via
//! USB-C/Thunderbolt network bridge – can download it by opening a link or
//! scanning a QR code.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tiny_http::{Header, Response, Server};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareUrl {
    pub url: String,
    pub interface: String,
    pub qr_svg: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareFile {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareInfo {
    pub id: String,
    pub title: String,
    pub path: String,
    pub port: u16,
    pub urls: Vec<ShareUrl>,
    pub files: Vec<ShareFile>,
    pub started_at: u64,
}

struct ActiveShare {
    info: ShareInfo,
    server: Arc<Server>,
}

static SHARES: LazyLock<Mutex<HashMap<String, ActiveShare>>> = LazyLock::new(Default::default);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn random_token() -> String {
    const ALPHA: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut buf = [0u8; 12];
    getrandom::fill(&mut buf).expect("OS random source unavailable");
    buf.iter().map(|b| ALPHA[(*b as usize) % ALPHA.len()] as char).collect()
}

fn collect_files(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![];
    if root.is_file() {
        out.push((root.file_name().unwrap_or_default().to_string_lossy().to_string(), root.to_path_buf()));
        return out;
    }
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>, depth: usize) {
        if depth > 6 || out.len() > 500 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            // Skip hidden files and symlinks (a link could point outside the shared folder).
            if name.starts_with('.') || e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            if p.is_dir() {
                walk(base, &p, out, depth + 1);
            } else {
                let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                out.push((rel, p));
            }
        }
    }
    walk(root, root, &mut out, 0);
    out
}

fn local_ips() -> Vec<(String, IpAddr)> {
    let mut v: Vec<(String, IpAddr)> = local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, ip)| ip.is_ipv4() && !ip.is_loopback())
        .collect();
    // Regular LAN addresses first, link-local (direct cable) afterwards.
    v.sort_by_key(|(_, ip)| match ip {
        IpAddr::V4(v4) if v4.is_link_local() => 1,
        _ => 0,
    });
    v
}

fn qr_svg(data: &str) -> String {
    use qrcode::render::svg;
    qrcode::QrCode::new(data.as_bytes())
        .map(|c| {
            c.render::<svg::Color>()
                .min_dimensions(180, 180)
                .quiet_zone(true)
                .dark_color(svg::Color("#000000"))
                .light_color(svg::Color("#ffffff"))
                .build()
        })
        .unwrap_or_default()
}

fn mime_for(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.ends_with(".apk") {
        "application/vnd.android.package-archive"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".txt") || lower.ends_with(".md") {
        "text/plain; charset=utf-8"
    } else {
        "application/octet-stream"
    }
}

fn human_size(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.1} {}", v, units[i])
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn index_page(title: &str, token: &str, files: &[(String, PathBuf)]) -> String {
    let mut rows = String::new();
    for (i, (name, path)) in files.iter().enumerate() {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        rows.push_str(&format!(
            r#"<a class="f" href="/{token}/f/{i}" download><span class="n">{}</span><span class="s">{}</span><span class="d">Herunterladen</span></a>"#,
            html_escape(name),
            human_size(size)
        ));
    }
    format!(
        r#"<!doctype html><html lang="de"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{t} · easyDeploy</title><style>
:root{{color-scheme:light dark;--bg:#f6f7fb;--card:#fff;--fg:#14161f;--mute:#667;--acc:#5b5bf0}}
@media(prefers-color-scheme:dark){{:root{{--bg:#0f1117;--card:#181b24;--fg:#e8e9f0;--mute:#99a;--acc:#8b8bff}}}}
body{{margin:0;font:16px/1.5 system-ui,-apple-system,Segoe UI,sans-serif;background:var(--bg);color:var(--fg)}}
main{{max-width:640px;margin:0 auto;padding:32px 16px}}h1{{font-size:22px;margin:0 0 4px}}p{{color:var(--mute);margin:0 0 24px}}
.f{{display:flex;gap:12px;align-items:center;background:var(--card);border-radius:12px;padding:14px 16px;margin-bottom:10px;text-decoration:none;color:inherit;box-shadow:0 1px 3px #0001}}
.n{{flex:1;word-break:break-all;font-weight:500}}.s{{color:var(--mute);font-size:14px;white-space:nowrap}}
.d{{background:var(--acc);color:#fff;border-radius:8px;padding:6px 12px;font-size:14px;white-space:nowrap}}
</style></head><body><main><h1>{t}</h1><p>Bereitgestellt von easyDeploy · Tippe auf eine Datei zum Herunterladen.</p>{rows}</main></body></html>"#,
        t = html_escape(title),
    )
}

pub fn start_share(app: &AppHandle, path: &Path, title: &str) -> Result<ShareInfo, String> {
    if !path.exists() {
        return Err(format!("Pfad nicht gefunden: {}", path.display()));
    }
    let files = collect_files(path);
    if files.is_empty() {
        return Err("Keine Dateien zum Freigeben gefunden".into());
    }
    let server = Server::http("0.0.0.0:0").map_err(|e| format!("Server-Start fehlgeschlagen: {e}"))?;
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
    let server = Arc::new(server);
    let token = random_token();
    let id = token.clone();

    let urls: Vec<ShareUrl> = local_ips()
        .into_iter()
        .map(|(iface, ip)| {
            let url = format!("http://{ip}:{port}/{token}/");
            ShareUrl { qr_svg: qr_svg(&url), url, interface: iface }
        })
        .collect();

    let info = ShareInfo {
        id: id.clone(),
        title: title.to_string(),
        path: path.to_string_lossy().to_string(),
        port,
        urls,
        files: files
            .iter()
            .map(|(n, p)| ShareFile { name: n.clone(), size: std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) })
            .collect(),
        started_at: now_secs(),
    };

    let srv = server.clone();
    let title_owned = title.to_string();
    std::thread::spawn(move || {
        let html_header = Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap();
        let nosniff = Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap();
        let csp = Header::from_bytes("Content-Security-Policy", "default-src 'none'; style-src 'unsafe-inline'").unwrap();
        let noref = Header::from_bytes("Referrer-Policy", "no-referrer").unwrap();
        for req in srv.incoming_requests() {
            if *req.method() != tiny_http::Method::Get && *req.method() != tiny_http::Method::Head {
                let _ = req.respond(Response::from_string("Methode nicht erlaubt").with_status_code(405));
                continue;
            }
            let url = req.url().to_string();
            let prefix = format!("/{token}/");
            if url == format!("/{token}") || url == prefix {
                let _ = req.respond(
                    Response::from_string(index_page(&title_owned, &token, &files))
                        .with_header(html_header.clone())
                        .with_header(csp.clone())
                        .with_header(noref.clone())
                        .with_header(nosniff.clone()),
                );
            } else if let Some(idx) = url.strip_prefix(&format!("{prefix}f/")).and_then(|s| s.parse::<usize>().ok()) {
                match files.get(idx).and_then(|(n, p)| std::fs::File::open(p).ok().map(|f| (n, f))) {
                    Some((name, file)) => {
                        let fname: String = name
                            .rsplit('/')
                            .next()
                            .unwrap_or(name)
                            .chars()
                            .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
                            .collect();
                        let mut resp = Response::from_file(file)
                            .with_header(Header::from_bytes("Content-Type", mime_for(&fname)).unwrap())
                            .with_header(nosniff.clone());
                        if let Ok(h) = Header::from_bytes("Content-Disposition", format!("attachment; filename=\"{fname}\"")) {
                            resp = resp.with_header(h);
                        }
                        let _ = req.respond(resp);
                    }
                    None => {
                        let _ = req.respond(Response::from_string("Nicht gefunden").with_status_code(404));
                    }
                }
            } else {
                let _ = req.respond(Response::from_string("Nicht gefunden").with_status_code(404));
            }
        }
    });

    SHARES.lock().unwrap().insert(id, ActiveShare { info: info.clone(), server });
    let _ = app.emit("share://changed", ());
    Ok(info)
}

#[tauri::command]
pub fn share_start(app: AppHandle, path: String, title: Option<String>) -> Result<ShareInfo, String> {
    let p = PathBuf::from(&path);
    let t = title.unwrap_or_else(|| p.file_name().unwrap_or_default().to_string_lossy().to_string());
    start_share(&app, &p, &t)
}

#[tauri::command]
pub fn share_stop(app: AppHandle, id: String) {
    if let Some(s) = SHARES.lock().unwrap().remove(&id) {
        s.server.unblock();
    }
    let _ = app.emit("share://changed", ());
}

#[tauri::command]
pub fn share_list() -> Vec<ShareInfo> {
    let mut v: Vec<ShareInfo> = SHARES.lock().unwrap().values().map(|s| s.info.clone()).collect();
    v.sort_by_key(|s| std::cmp::Reverse(s.started_at));
    v
}
