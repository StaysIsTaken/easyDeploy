// Deploy recipes: turn (project type × target) into an editable list of steps,
// the tools that must be installed, and hints for the user.

import type { DetectedType, Project, ProjectKind, Step, Target, TargetKind } from "./types";
import { safeName, uid } from "./util";

export interface Recipe {
  name: string;
  steps: Step[];
  vars: Record<string, string>;
  tools: string[];
  notes: string[];
}

export const TARGET_LABEL: Record<TargetKind, string> = {
  ssh: "Server (SSH)",
  android: "Android-Gerät",
  ios: "iPhone / iPad",
  local: "Dieser PC",
  folder: "Ordner / USB-Laufwerk",
  share: "Download im Netzwerk (QR)",
  github: "GitHub Actions",
};

export const VAR_LABEL: Record<string, string> = {
  remotePath: "Pfad auf dem Server",
  appName: "App-Name",
  dest: "Ziel-Ordner",
  image: "Docker-Image / Container-Name",
  ports: "Ports (Host:Container)",
  bundleId: "Bundle-ID",
};

export const TARGETS_FOR: Record<ProjectKind, TargetKind[]> = {
  flutter: ["android", "ios", "local", "share", "folder", "ssh", "github"],
  "react-native": ["android", "ios", "share", "folder", "ssh", "github"],
  tauri: ["local", "share", "folder", "android", "ios", "ssh", "github"],
  node: ["ssh", "local", "share", "folder", "github"],
  python: ["ssh", "local", "github"],
  docker: ["local", "ssh", "github"],
  dart: ["ssh", "local", "github"],
};

const step = (kind: Step["kind"], name: string, extra: Partial<Step> = {}): Step => ({
  id: uid(),
  kind,
  name,
  ...extra,
});
const sh = (name: string, command: string, extra: Partial<Step> = {}) => step("shell", name, { command, ...extra });

function pmCommands(pm: string) {
  switch (pm) {
    case "pnpm":
      return { install: "pnpm install", run: (s: string) => `pnpm run ${s}`, exec: "pnpm exec", tool: "pnpm" };
    case "yarn":
      return { install: "yarn install", run: (s: string) => `yarn ${s}`, exec: "yarn", tool: "yarn" };
    case "bun":
      return { install: "bun install", run: (s: string) => `bun run ${s}`, exec: "bunx", tool: "" };
    default:
      return { install: "npm install", run: (s: string) => `npm run ${s}`, exec: "npx", tool: "npm" };
  }
}

const desktopPlatform = (os: string) => (os === "macos" ? "macos" : os === "windows" ? "windows" : "linux");

const ANDROID_BUILD_TOOLS = ["java", "android-licenses", "android-platform"];
const APPLE_TOOLS = ["xcode", "xcode-license"];

export function buildRecipe(project: Project, type: DetectedType, target: Target, os: string): Recipe {
  const r: Recipe = { name: `${type.label} → ${target.name}`, steps: [], vars: {}, tools: ["git"], notes: [] };
  const appSlug = safeName(project.name);
  const t = target.kind;
  const add = (...s: Step[]) => r.steps.push(...s);
  const tools = (...ids: string[]) => ids.filter(Boolean).forEach((id) => !r.tools.includes(id) && r.tools.push(id));
  if (t === "ssh") {
    tools("ssh", "tar");
    r.vars.remotePath = `~/apps/${appSlug}`;
  }
  if (t === "folder") r.vars.dest = target.folder?.path ?? "";

  if (t === "github") {
    add(step("ghDispatch", "GitHub-Workflow starten"));
    r.notes.push("Startet den gewählten Workflow über „workflow_dispatch“. Unter „Bearbeiten“ kannst du eine passende Workflow-Datei erzeugen lassen.");
    return r;
  }

  switch (type.kind) {
    // ------------------------------------------------------------------ Flutter
    case "flutter": {
      const f = type.details.fvm ? "fvm flutter" : "flutter";
      tools(type.details.fvm ? "fvm" : "flutter");
      if (project.constraints.some((c) => c.tool === "dart")) tools("dart");
      const platforms = (type.details.platforms ?? "").split(",");
      add(sh("Abhängigkeiten laden", `${f} pub get`));
      const apk = "build/app/outputs/flutter-apk/app-release.apk";
      if (t === "android") {
        tools("adb", ...ANDROID_BUILD_TOOLS);
        add(
          sh("APK bauen (Release)", `${f} build apk --release`),
          sh("Auf Gerät installieren", `adb -s {{device}} install -r "${apk}"`),
        );
      } else if (t === "ios") {
        tools(...APPLE_TOOLS, "cocoapods");
        const app = "build/ios/easydeploy/Build/Products/Release-iphoneos/Runner.app";
        add(
          sh("iOS-Projekt vorbereiten", `${f} build ios --release --config-only`),
          sh(
            "Bauen & signieren (Xcode)",
            `xcodebuild -quiet -workspace ios/Runner.xcworkspace -scheme Runner -configuration Release -destination "id={{device}}" -derivedDataPath build/ios/easydeploy -allowProvisioningUpdates -allowProvisioningDeviceRegistration build`,
          ),
          sh("Auf Gerät installieren", `xcrun devicectl device install app --device {{device}} "${app}"`),
        );
        if (type.details.iosBundleId) {
          r.vars.bundleId = type.details.iosBundleId;
          add(sh("App starten", "xcrun devicectl device process launch --device {{device}} {{bundleId}}", { allowFailure: true }));
        }
        r.notes.push(
          "Xcode registriert dein Gerät beim ersten Build automatisch im Apple-Team. Voraussetzung: In Xcode → Einstellungen → Accounts ist deine Apple-ID angemeldet und im Runner-Target ist ein Team gewählt.",
        );
      } else if (t === "local") {
        const p = desktopPlatform(os);
        if (!platforms.includes(p)) add(sh(`${p}-Plattform hinzufügen`, `${f} create --platforms=${p} .`));
        if (os === "macos") tools(...APPLE_TOOLS, "cocoapods");
        add(sh(`Desktop-App bauen (${p})`, `${f} build ${p} --release`));
        const artifact =
          p === "macos"
            ? "build/macos/Build/Products/Release/{{appName}}.app"
            : p === "windows"
              ? "build/windows/x64/runner/Release/{{appName}}.exe"
              : "build/linux/x64/release/bundle/{{appName}}";
        add(step("open", "App starten", { target: artifact }));
        r.vars.appName = appSlug;
      } else if (t === "ssh") {
        add(
          sh("Web-Version bauen", `${f} build web --release`),
          step("upload", "Auf Server hochladen", { source: "build/web", remotePath: "{{remotePath}}" }),
        );
        r.notes.push("Lädt die Flutter-Web-Version hoch. Richte deinen Webserver (nginx, Caddy …) auf den Remote-Pfad aus.");
      } else if (t === "share" || t === "folder") {
        if (platforms.includes("android")) {
          tools(...ANDROID_BUILD_TOOLS);
          add(sh("APK bauen (Release)", `${f} build apk --release`));
          add(t === "share" ? step("share", "Im Netzwerk bereitstellen", { source: apk }) : step("copy", "In Ordner kopieren", { source: apk, dest: "{{dest}}" }));
        } else {
          const p = desktopPlatform(os);
          add(sh(`Desktop-App bauen (${p})`, `${f} build ${p} --release`));
          const out = p === "macos" ? "build/macos/Build/Products/Release" : p === "windows" ? "build/windows/x64/runner/Release" : "build/linux/x64/release/bundle";
          add(t === "share" ? step("share", "Im Netzwerk bereitstellen", { source: out }) : step("copy", "In Ordner kopieren", { source: out, dest: "{{dest}}" }));
        }
      }
      break;
    }

    // ------------------------------------------------------------- React Native
    case "react-native": {
      const pm = pmCommands(type.details.packageManager ?? "npm");
      const expo = type.details.expo === "true";
      tools("node", pm.tool);
      add(sh("Abhängigkeiten installieren", pm.install));
      const gradle = os === "windows" ? "gradlew.bat" : "./gradlew";
      const apk = "android/app/build/outputs/apk/release/app-release.apk";
      const buildApk = () => {
        tools(...ANDROID_BUILD_TOOLS);
        if (expo && type.details.android !== "true") add(sh("Android-Projekt erzeugen (Expo)", `${pm.exec} expo prebuild --platform android`));
        add(sh("APK bauen (Release)", `${gradle} assembleRelease`, { cwd: "android" }));
      };
      if (t === "android") {
        tools("adb");
        buildApk();
        add(sh("Auf Gerät installieren", `adb -s {{device}} install -r "${apk}"`));
      } else if (t === "ios") {
        tools(...APPLE_TOOLS, "cocoapods");
        add(
          expo
            ? sh("Bauen & auf Gerät installieren", `${pm.exec} expo run:ios --configuration Release --device {{device}}`)
            : sh("Bauen & auf Gerät installieren", `${pm.exec} react-native run-ios --mode Release --udid {{device}}`),
        );
        r.notes.push("Für echte iPhones muss in Xcode ein Signierungs-Team eingestellt sein.");
      } else if (t === "share" || t === "folder" || t === "ssh") {
        buildApk();
        if (t === "share") add(step("share", "Im Netzwerk bereitstellen", { source: apk }));
        if (t === "folder") add(step("copy", "In Ordner kopieren", { source: apk, dest: "{{dest}}" }));
        if (t === "ssh") add(step("upload", "APK auf Server hochladen", { source: apk, remotePath: "{{remotePath}}" }));
      }
      if (t !== "ios") r.notes.push("Release-APKs müssen signiert sein. Ist in android/app/build.gradle kein Release-Keystore eingerichtet, nutzt React Native den Debug-Key.");
      break;
    }

    // -------------------------------------------------------------------- Tauri
    case "tauri": {
      const pm = pmCommands(type.details.packageManager ?? "npm");
      tools("node", pm.tool, "rust");
      add(sh("Abhängigkeiten installieren", pm.install));
      const tauri = pm.tool === "npm" ? "npm run tauri --" : `${pm.exec} tauri`;
      const bundle = "src-tauri/target/release/bundle";
      if (t === "local" || t === "share" || t === "folder" || t === "ssh") {
        if (os === "macos") tools("xcode");
        add(sh("App bauen & Installer erzeugen", `${tauri} build`));
        if (t === "local") add(step("open", "Installer-Ordner öffnen", { target: bundle }));
        if (t === "share") add(step("share", "Installer im Netzwerk bereitstellen", { source: bundle }));
        if (t === "folder") add(step("copy", "In Ordner kopieren", { source: bundle, dest: "{{dest}}" }));
        if (t === "ssh") add(step("upload", "Installer hochladen", { source: bundle, remotePath: "{{remotePath}}" }));
      } else if (t === "android") {
        tools("adb", ...ANDROID_BUILD_TOOLS, "sdkmanager");
        if (type.details.android !== "true") add(sh("Android-Projekt initialisieren", `${tauri} android init`));
        add(
          sh("APK bauen (Debug-signiert)", `${tauri} android build --apk --debug`),
          sh("Auf Gerät installieren", `adb -s {{device}} install -r "src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk"`),
        );
        r.notes.push("Für Release-Builds (Play Store) muss ein Keystore konfiguriert werden – siehe tauri.app/distribute/sign/android.");
      } else if (t === "ios") {
        tools(...APPLE_TOOLS, "cocoapods");
        if (type.details.ios !== "true") add(sh("iOS-Projekt initialisieren", `${tauri} ios init`));
        add(
          sh("iOS-App bauen", `${tauri} ios build --export-method debugging`),
          sh("Auf Gerät installieren", `xcrun devicectl device install app --device {{device}} "src-tauri/gen/apple/build/arm64/{{appName}}.ipa"`),
        );
        r.vars.appName = safeName(type.details.productName ?? project.name);
        r.notes.push("Benötigt ein Signierungs-Team in Xcode (src-tauri/gen/apple).");
      }
      break;
    }

    // --------------------------------------------------------------------- Node
    case "node": {
      const pm = pmCommands(type.details.packageManager ?? "npm");
      const scripts = (type.details.scripts ?? "").split(",");
      const isServer = type.details.server === "true";
      const out = type.details.outputDir ?? "dist";
      tools("node", pm.tool);
      add(sh("Abhängigkeiten installieren", pm.install));
      if (scripts.includes("build")) add(sh("Bauen", pm.run("build")));
      if (t === "ssh") {
        if (isServer) {
          add(
            step("upload", "Projekt hochladen", {
              source: ".",
              remotePath: "{{remotePath}}",
              excludes: ["node_modules", ".git", ".env", ".DS_Store"],
            }),
            step("remote", "Auf dem Server installieren & (neu)starten", {
              command:
                "cd {{remotePath}} && npm install --omit=dev && (npx pm2 restart {{appName}} || npx pm2 start npm --name {{appName}} -- start)",
            }),
          );
          r.vars.appName = appSlug;
          r.notes.push("Der Server braucht Node.js. Die App wird mit pm2 gestartet und bei jedem Deploy neu gestartet.");
        } else {
          add(step("upload", "Build hochladen", { source: out, remotePath: "{{remotePath}}", clean: true }));
          r.notes.push(`Lädt den Inhalt von „${out}“ hoch und ersetzt den alten Stand. Richte deinen Webserver auf den Remote-Pfad aus.`);
        }
      } else if (t === "local") {
        const start = scripts.includes("start") ? "start" : scripts.includes("preview") ? "preview" : scripts.includes("dev") ? "dev" : "start";
        add(sh("Starten", pm.run(start)));
        r.notes.push("Die App läuft, bis du den Job stoppst.");
      } else if (t === "share") {
        add(step("share", "Build im Netzwerk bereitstellen", { source: out }));
      } else if (t === "folder") {
        add(step("copy", "Build in Ordner kopieren", { source: out, dest: "{{dest}}" }));
      }
      break;
    }

    // ------------------------------------------------------------------- Python
    case "python": {
      const uv = type.details.tool === "uv";
      const entry = type.details.entry ?? "main.py";
      const hasReq = !!type.details.requirements;
      tools(uv ? "uv" : "python");
      const django = type.details.framework === "Django";
      if (t === "ssh") {
        add(
          step("upload", "Projekt hochladen", {
            source: ".",
            remotePath: "{{remotePath}}",
            excludes: [".venv", "venv", "__pycache__", ".git", ".env", "*.pyc"],
          }),
          step("remote", "Umgebung einrichten", {
            command: uv
              ? "cd {{remotePath}} && uv sync"
              : `cd {{remotePath}} && python3 -m venv .venv && .venv/bin/pip install -U pip${hasReq ? " && .venv/bin/pip install -r requirements.txt" : " && .venv/bin/pip install ."}`,
          }),
        );
        if (django) add(step("remote", "Migrationen", { command: "cd {{remotePath}} && .venv/bin/python manage.py migrate --noinput" }));
        r.notes.push("Füge bei Bedarf einen Schritt zum Neustarten deines Dienstes hinzu, z. B. „sudo systemctl restart meine-app“.");
      } else if (t === "local") {
        const py = os === "windows" ? ".venv\\Scripts\\python" : ".venv/bin/python";
        if (uv) {
          add(sh("Abhängigkeiten", "uv sync"), sh("Starten", django ? "uv run manage.py runserver" : `uv run ${entry}`));
        } else {
          const create = os === "windows" ? "python -m venv .venv" : "python3 -m venv .venv";
          add(sh("Virtuelle Umgebung", create));
          add(sh("Abhängigkeiten", hasReq ? `${py} -m pip install -r requirements.txt` : `${py} -m pip install .`));
          add(sh("Starten", django ? `${py} manage.py runserver` : `${py} ${entry}`));
        }
      }
      break;
    }

    // ------------------------------------------------------------------- Docker
    case "docker": {
      tools("docker", "docker-daemon");
      const compose = type.details.compose;
      if (t === "local") {
        if (compose) add(sh("Container bauen & starten", "docker compose up -d --build"));
        else {
          r.vars.image = appSlug.toLowerCase();
          r.vars.ports = "8080:8080";
          add(
            sh("Image bauen", "docker build -t {{image}} ."),
            sh("Alten Container entfernen", "docker rm -f {{image}}", { allowFailure: true }),
            sh("Container starten", "docker run -d --name {{image}} -p {{ports}} {{image}}"),
          );
        }
      } else if (t === "ssh") {
        add(
          step("upload", "Projekt hochladen", { source: ".", remotePath: "{{remotePath}}", excludes: [".git", "node_modules", ".venv", ".env"] }),
          step("remote", compose ? "docker compose up" : "Image bauen & Container starten", {
            command: compose
              ? "cd {{remotePath}} && docker compose up -d --build"
              : "cd {{remotePath}} && docker build -t {{image}} . && (docker rm -f {{image}} || true) && docker run -d --name {{image}} -p {{ports}} {{image}}",
          }),
        );
        if (!compose) {
          r.vars.image = appSlug.toLowerCase();
          r.vars.ports = "8080:8080";
        }
        r.notes.push("Auf dem Server muss Docker installiert sein und dein Benutzer Docker ausführen dürfen.");
      }
      break;
    }

    case "dart": {
      tools("dart");
      add(sh("Abhängigkeiten laden", "dart pub get"), sh("Kompilieren", "dart compile exe bin/main.dart -o build/app"));
      if (t === "ssh") add(step("upload", "Hochladen", { source: "build", remotePath: "{{remotePath}}" }));
      if (t === "local") add(sh("Starten", os === "windows" ? "build\\app" : "./build/app"));
      break;
    }
  }

  if (t === "ssh" && target.ssh?.auth === "password") {
    r.notes.push("Tipp: Mit einem SSH-Key statt Passwort geht das Deployen ohne Rückfragen.");
  }
  return r;
}

/** Tool ids to check when no profile exists yet. */
export function baseTools(project: Project): string[] {
  const ids = new Set<string>(["git"]);
  for (const t of project.types) {
    const pmTool = pmCommands(t.details.packageManager ?? "npm").tool;
    if (t.kind === "flutter") ids.add(t.details.fvm ? "fvm" : "flutter");
    if (t.kind === "react-native") ["node", pmTool].forEach((x) => x && ids.add(x));
    if (t.kind === "tauri") ["node", pmTool, "rust"].forEach((x) => x && ids.add(x));
    if (t.kind === "node") ["node", pmTool].forEach((x) => x && ids.add(x));
    if (t.kind === "python") ids.add(t.details.tool === "uv" ? "uv" : "python");
    if (t.kind === "docker") ["docker", "docker-daemon"].forEach((x) => ids.add(x));
    if (t.kind === "dart") ids.add("dart");
  }
  for (const c of project.constraints) {
    if (c.tool === "dart" || c.tool === "rust" || c.tool === "node" || c.tool === "python") ids.add(c.tool);
  }
  return [...ids];
}
