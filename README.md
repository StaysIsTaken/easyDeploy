# easyDeploy

Desktop-App (Tauri + React), die beliebige Projekte per Klick deployt – auf Server (SSH), Android-Geräte, iPhones/iPads, diesen PC, USB-Laufwerke, als Download per QR-Code im Netzwerk oder über GitHub Actions.

## Entwicklung

```bash
npm install
npm run tauri dev
```

Voraussetzungen: Node.js, Rust (rustup). Unter Linux zusätzlich die [Tauri-Systempakete](https://tauri.app/start/prerequisites/#linux) sowie `libdbus-1-dev` (für den Schlüsselbund).

## Aufbau

- `src-tauri/src/jobs.rs` – Pipeline-Runner in einem echten PTY, erkennt Rückfragen (Passwort, Host-Key, Lizenzen, Ja/Nein) und reicht sie an die UI weiter
- `src-tauri/src/detect.rs` – Projekterkennung (Flutter, React Native, Tauri, Node, Python, Docker) inkl. verlangter Versionen
- `src-tauri/src/requirements.rs` – Tool-/SDK-Checks mit Ein-Klick-Installation (brew / winget / apt / dnf / pacman)
- `src-tauri/src/devices.rs` – Geräteerkennung (adb, devicectl, mDNS, ~/.ssh/config)
- `src-tauri/src/share.rs` – LAN-Download mit QR-Code
- `src-tauri/src/github.rs` – GitHub-API (Repos, Workflows, Runs, Dispatch)
- `src-tauri/src/secrets.rs` – Passwörter & Tokens im System-Schlüsselbund
- `src/lib/recipes.ts` – Deploy-Rezepte je Projekttyp × Ziel
- `src/lib/workflow.ts` – Generator für GitHub-Actions-Workflows
