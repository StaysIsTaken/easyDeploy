// Known failure messages → plain-language explanation and, where possible, a
// one-click fix. Shown in the run panel after a job fails.

export interface Hint {
  title: string;
  text: string;
  action?: { label: string; command: string };
}

interface Rule {
  re: RegExp;
  hint: (m: RegExpMatchArray) => Hint;
}

const RULES: Rule[] = [
  {
    re: /no devices from which to generate a provisioning profile/i,
    hint: () => ({
      title: "iPhone ist nicht im Apple-Team registriert",
      text: "Xcode konnte kein Provisioning-Profil erstellen, weil dein Gerät noch nicht registriert ist. Das aktuelle iOS-Rezept registriert das Gerät automatisch. Öffne das Profil unter „Bearbeiten“ und klicke auf „Schritte neu erzeugen“.",
    }),
  },
  {
    re: /(No Account for Team|No Accounts|requires a development team|Signing for "[^"]+" requires)/i,
    hint: () => ({
      title: "Kein Signierungs-Team eingerichtet",
      text: "Melde dich in Xcode unter Einstellungen → Accounts mit deiner Apple-ID an. Wähle danach im Projekt (Target „Runner“ → Signing & Capabilities) dein Team aus.",
      action: { label: "Xcode-Projekt öffnen", command: "open ios/Runner.xcworkspace" },
    }),
  },
  {
    re: /(profile has not been explicitly trusted|untrusted developer|invalid code signature|is not trusted|ApplicationVerificationFailed)/i,
    hint: () => ({
      title: "Entwickler auf dem iPhone vertrauen",
      text: "Auf dem iPhone: Einstellungen → Allgemein → VPN & Geräteverwaltung → dein Entwicklerzertifikat antippen → „Vertrauen“. Danach die App erneut starten.",
    }),
  },
  {
    re: /(developer ?mode (is )?(disabled|not enabled)|Developer Mode)/i,
    hint: () => ({
      title: "Entwicklermodus aktivieren",
      text: "Auf dem Gerät: Einstellungen → Datenschutz & Sicherheit → Entwicklermodus einschalten und neu starten.",
    }),
  },
  {
    re: /(device is locked|Unable to launch .* because the device was not, or could not be, unlocked)/i,
    hint: () => ({ title: "Gerät ist gesperrt", text: "Entsperre das Gerät und starte das Deploy erneut." }),
  },
  {
    re: /INSTALL_FAILED_UPDATE_INCOMPATIBLE[^\n]*?Package ([\w.]+)/,
    hint: (m) => ({
      title: "Alte App-Version mit anderer Signatur",
      text: `Auf dem Gerät ist ${m[1]} mit einer anderen Signatur installiert. Deinstalliere die alte Version (App-Daten gehen dabei verloren) und deploye erneut.`,
      action: { label: "Alte App deinstallieren", command: `adb uninstall ${m[1]}` },
    }),
  },
  {
    re: /INSTALL_FAILED_USER_RESTRICTED/,
    hint: () => ({
      title: "Installation über USB blockiert",
      text: "Auf dem Gerät muss die Installation über USB erlaubt werden (z. B. Xiaomi: Entwickleroptionen → „Über USB installieren“) bzw. die Nachfrage auf dem Display bestätigt werden.",
    }),
  },
  {
    re: /(device unauthorized|unauthorized\. This adb server)/i,
    hint: () => ({ title: "USB-Debugging nicht erlaubt", text: "Entsperre das Android-Gerät und bestätige „USB-Debugging zulassen“." }),
  },
  {
    re: /You have not accepted the license agreements|licen[cs]es? (have )?not (been )?accepted/i,
    hint: () => ({
      title: "Android-Lizenzen nicht akzeptiert",
      text: "Die Android-SDK-Lizenzen müssen einmalig akzeptiert werden. Du bestätigst jede Lizenz selbst.",
      action: { label: "Lizenzen anzeigen & akzeptieren", command: "sdkmanager --licenses" },
    }),
  },
  {
    re: /SDK location not found|ANDROID_(HOME|SDK_ROOT).*not (set|found)/i,
    hint: () => ({
      title: "Android SDK nicht gefunden",
      text: "Installiere das Android SDK (Seite „Voraussetzungen“) oder setze ANDROID_HOME auf den SDK-Ordner.",
    }),
  },
  {
    re: /Permission denied \(publickey/i,
    hint: () => ({
      title: "SSH-Key wird vom Server abgelehnt",
      text: "Der Server kennt deinen öffentlichen Schlüssel nicht. Wähle im Ziel „Passwort“ als Anmeldung, oder hinterlege deinen Key auf dem Server (ssh-copy-id).",
    }),
  },
  {
    re: /REMOTE HOST IDENTIFICATION HAS CHANGED|Host key verification failed/i,
    hint: () => ({
      title: "Server-Schlüssel hat sich geändert",
      text: "Der gespeicherte Fingerabdruck passt nicht mehr, etwa nach einer Neuinstallation des Servers. Wenn du sicher bist, dass es der richtige Server ist, entferne den alten Eintrag aus known_hosts und verbinde dich erneut.",
    }),
  },
  {
    re: /(Could not resolve hostname|Connection refused|Connection timed out|Operation timed out|No route to host)/i,
    hint: () => ({
      title: "Server nicht erreichbar",
      text: "Prüfe Host/IP, Port und ob der Rechner eingeschaltet und im selben Netzwerk ist. Bei Rechnern im Heimnetz muss SSH aktiviert sein (macOS: Freigaben → Entfernte Anmeldung).",
    }),
  },
  {
    re: /Cannot connect to the Docker daemon|docker daemon is not running|error during connect/i,
    hint: () => ({ title: "Docker läuft nicht", text: "Starte Docker Desktop bzw. den Docker-Dienst und versuche es erneut." }),
  },
  {
    re: /(EADDRINUSE|address already in use)/i,
    hint: () => ({ title: "Port ist belegt", text: "Ein anderes Programm nutzt den Port bereits. Beende es oder ändere den Port." }),
  },
  {
    re: /Cannot find module @?(@rollup\/rollup-[\w-]+|@esbuild\/[\w-]+|@swc\/core-[\w-]+|lightningcss-[\w-]+)|npm has a bug related to optional dependencies/,
    hint: () => ({
      title: "node_modules ist unvollständig",
      text: "Eine plattformspezifische Datei fehlt. Das passiert oft, wenn der Ordner node_modules von einem anderen Rechner mitkopiert wurde (z. B. per ZIP oder Teams). Lösche node_modules und installiere neu.",
      action: {
        label: "node_modules neu installieren",
        command: `node -e "require('fs').rmSync('node_modules',{recursive:true,force:true})" && npm install`,
      },
    }),
  },
  {
    re: /allow-scripts[^\n]*not yet covered by allowScripts/,
    hint: () => ({
      title: "npm hat Install-Skripte blockiert",
      text: "Neuere npm-Versionen führen Install-Skripte erst nach deiner Freigabe aus. Fehlen deshalb native Module (z. B. „Could not locate the bindings file“), prüfe die Liste und gib nur Pakete frei, denen du vertraust: npm install-scripts approve <paket>.",
      action: { label: "Blockierte Skripte anzeigen", command: "npm install-scripts ls" },
    }),
  },
  {
    re: /(?:^|\n)[^\n]*?(?:bash: |sh: |zsh: )?(?:line \d+: )?([\w.-]+): (?:command )?not found/,
    hint: (m) => ({
      title: `„${m[1]}“ ist nicht installiert`,
      text: "Das Programm wurde nicht gefunden. Auf der Seite „Voraussetzungen“ kannst du es installieren. Nach einer Installation hilft manchmal ein Neustart von easyDeploy, damit der PATH aktualisiert wird.",
    }),
  },
  {
    re: /'([\w.-]+)' is not recognized as an internal or external command/,
    hint: (m) => ({
      title: `„${m[1]}“ ist nicht installiert`,
      text: "Das Programm wurde nicht gefunden. Auf der Seite „Voraussetzungen“ kannst du es installieren und danach easyDeploy neu starten.",
    }),
  },
  {
    re: /CocoaPods not installed|pod: command not found/i,
    hint: () => ({ title: "CocoaPods fehlt", text: "CocoaPods wird für iOS-Builds benötigt.", action: { label: "CocoaPods installieren", command: "brew install cocoapods" } }),
  },
];

const ANSI = /\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g;

export function findHints(output: string): Hint[] {
  const text = output.replace(ANSI, "").slice(-200_000);
  const seen = new Set<string>();
  const out: Hint[] = [];
  for (const r of RULES) {
    const m = text.match(r.re);
    if (!m) continue;
    const h = r.hint(m);
    if (seen.has(h.title)) continue;
    seen.add(h.title);
    out.push(h);
  }
  return out;
}
