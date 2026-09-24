// Generates a GitHub Actions workflow that builds the project in CI, so the
// same project can be deployed locally with easyDeploy and in GitHub Actions.

import type { DetectedType, Project } from "./types";
import { safeName } from "./util";

/** JSON strings are valid YAML scalars, so this safely quotes any text. */
const q = (s: string) => JSON.stringify(s);

const header = (name: string) => `# Erzeugt von easyDeploy
name: ${q(name)}

on:
  workflow_dispatch:
  # push:
  #   branches: [main]

jobs:
`;

function nodeVersion(project: Project): string {
  const c = project.constraints.find((x) => x.tool === "node");
  const m = c?.constraint.match(/\d+/);
  return m ? m[0] : "20";
}

/** FVM versions look like "3.22.2" or "stable"; anything else is dropped. */
function flutterVersionLine(v?: string): string {
  return v && /^[\w.+-]+$/.test(v) ? `\n          flutter-version: "${v}"` : "";
}

function pmSetup(pm: string): { install: string; run: (s: string) => string; setup: string } {
  if (pm === "pnpm") return { install: "pnpm install --frozen-lockfile", run: (s) => `pnpm run ${s}`, setup: "      - uses: pnpm/action-setup@v4\n" };
  if (pm === "yarn") return { install: "yarn install --frozen-lockfile", run: (s) => `yarn ${s}`, setup: "" };
  return { install: "npm ci", run: (s) => `npm run ${s}`, setup: "" };
}

export function workflowFileName(type: DetectedType): string {
  return `easydeploy-${type.kind}.yml`;
}

export function generateWorkflow(input: Project, type: DetectedType): string {
  // Only a sanitized name ever reaches the YAML (project files are untrusted).
  const project = { ...input, name: safeName(input.name) };
  const pm = pmSetup(type.details.packageManager ?? "npm");
  const node = nodeVersion(project);
  switch (type.kind) {
    case "flutter": {
      const platforms = (type.details.platforms ?? "").split(",");
      const flutterVersion = flutterVersionLine(type.details.fvm);
      const android = platforms.includes("android");
      return (
        header(`Build ${project.name} (Flutter)`) +
        `  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
${android ? `      - uses: actions/setup-java@v4\n        with:\n          distribution: temurin\n          java-version: "17"\n` : ""}      - uses: subosito/flutter-action@v2
        with:
          channel: stable${flutterVersion}
          cache: true
      - run: flutter pub get
${android ? "      - run: flutter build apk --release\n" : ""}${platforms.includes("web") ? "      - run: flutter build web --release\n" : ""}      - uses: actions/upload-artifact@v4
        with:
          name: ${project.name}-build
          path: |
${android ? "            build/app/outputs/flutter-apk/app-release.apk\n" : ""}${platforms.includes("web") ? "            build/web\n" : ""}`
      );
    }
    case "react-native":
      return (
        header(`Build ${project.name} (Android)`) +
        `  android:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
${pm.setup}      - uses: actions/setup-node@v4
        with:
          node-version: "${node}"
      - uses: actions/setup-java@v4
        with:
          distribution: temurin
          java-version: "17"
      - run: ${pm.install}
${type.details.expo === "true" && type.details.android !== "true" ? "      - run: npx expo prebuild --platform android\n" : ""}      - run: ./gradlew assembleRelease
        working-directory: android
      - uses: actions/upload-artifact@v4
        with:
          name: ${project.name}-apk
          path: android/app/build/outputs/apk/release/*.apk
`
      );
    case "tauri":
      return (
        header(`Build ${project.name} (Tauri)`) +
        `  build:
    strategy:
      fail-fast: false
      matrix:
        platform: [macos-latest, ubuntu-22.04, windows-latest]
    runs-on: \${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4
      - name: Linux-Abhängigkeiten
        if: matrix.platform == 'ubuntu-22.04'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
${pm.setup}      - uses: actions/setup-node@v4
        with:
          node-version: "${node}"
      - uses: dtolnay/rust-toolchain@stable
      - uses: swatinem/rust-cache@v2
        with:
          workspaces: "./src-tauri -> target"
      - run: ${pm.install}
      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: \${{ secrets.GITHUB_TOKEN }}
      - uses: actions/upload-artifact@v4
        with:
          name: ${project.name}-\${{ matrix.platform }}
          path: src-tauri/target/release/bundle
`
      );
    case "node": {
      const scripts = (type.details.scripts ?? "").split(",");
      const out = type.details.outputDir ?? "dist";
      return (
        header(`Build & Deploy ${project.name}`) +
        `  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
${pm.setup}      - uses: actions/setup-node@v4
        with:
          node-version: "${node}"
      - run: ${pm.install}
${scripts.includes("test") ? `      - run: ${pm.run("test")} --if-present\n` : ""}${scripts.includes("build") ? `      - run: ${pm.run("build")}\n` : ""}      - uses: actions/upload-artifact@v4
        with:
          name: ${project.name}-build
          path: ${out}
      # Deploy per SSH: Secrets SSH_HOST, SSH_USER, SSH_KEY im Repository anlegen
      # - uses: appleboy/scp-action@v0.1.7
      #   with:
      #     host: \${{ secrets.SSH_HOST }}
      #     username: \${{ secrets.SSH_USER }}
      #     key: \${{ secrets.SSH_KEY }}
      #     source: "${out}/*"
      #     target: "~/apps/${project.name}"
      #     strip_components: 1
`
      );
    }
    case "python": {
      const c = project.constraints.find((x) => x.tool === "python");
      const v = c?.constraint.match(/\d+\.\d+/)?.[0] ?? "3.12";
      return (
        header(`Build ${project.name} (Python)`) +
        `  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: "${v}"
      - run: pip install -U pip
${type.details.requirements ? "      - run: pip install -r requirements.txt\n" : "      - run: pip install .\n"}      - run: python -m compileall -q .
`
      );
    }
    case "docker":
      return (
        header(`Docker-Image ${project.name}`) +
        `  image:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write
    steps:
      - uses: actions/checkout@v4
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: \${{ github.actor }}
          password: \${{ secrets.GITHUB_TOKEN }}
      - uses: docker/build-push-action@v6
        with:
          context: .
          push: true
          tags: ghcr.io/\${{ github.repository }}:latest
`
      );
    default:
      return (
        header(`Build ${project.name}`) +
        `  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: echo "Build-Schritte hier ergänzen"
`
      );
  }
}
