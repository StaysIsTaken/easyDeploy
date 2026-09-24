//! Requirement checks: is a tool installed, in the right version, and how can
//! the user install or fix it with one click on the current OS.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::env_path::{android_sdk_candidates, which};
use crate::proc;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub label: String,
    /// Shell command to run in an interactive terminal.
    pub command: Option<String>,
    /// URL/app link to open instead of running a command.
    pub url: Option<String>,
    /// Text the user must explicitly accept before the command runs (licenses).
    pub confirm: Option<String>,
    /// Run the command inside the project folder.
    pub in_project: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub ok: bool,
    pub required: Option<String>,
    pub required_source: Option<String>,
    pub message: Option<String>,
    pub actions: Vec<Action>,
    pub docs_url: Option<String>,
    pub optional: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintIn {
    pub tool: String,
    pub constraint: String,
    pub mode: String,
    pub source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub hostname: String,
    pub home: String,
    pub linux_package_manager: Option<String>,
    pub android_home: Option<String>,
}

#[tauri::command]
pub fn system_info() -> SystemInfo {
    SystemInfo {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        hostname: hostname(),
        home: dirs::home_dir().unwrap_or_default().to_string_lossy().into(),
        linux_package_manager: linux_pm().map(String::from),
        android_home: std::env::var("ANDROID_HOME").ok(),
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| proc::run("hostname", &[], Duration::from_secs(3)).map(|o| o.text.trim().to_string()))
        .unwrap_or_else(|| "Dieser PC".into())
}

fn linux_pm() -> Option<&'static str> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    ["apt-get", "dnf", "pacman", "zypper"].into_iter().find(|pm| which(pm).is_some())
}

// ---------------------------------------------------------------------------
// Install recipes
// ---------------------------------------------------------------------------

struct Pkg {
    brew: Option<&'static str>,
    winget: Option<&'static str>,
    apt: Option<&'static str>,
    dnf: Option<&'static str>,
    pacman: Option<&'static str>,
    /// Fallback shell command (all unix platforms).
    unix: Option<&'static str>,
}

const NONE: Pkg = Pkg { brew: None, winget: None, apt: None, dnf: None, pacman: None, unix: None };

/// Commands that download and execute a script need an explicit OK first.
fn script_confirm(cmd: &str) -> Option<String> {
    let piped = cmd.contains("| sh") || cmd.contains("| bash") || cmd.contains("$(curl");
    piped.then(|| {
        let url = cmd.split_whitespace().find(|w| w.starts_with("https://")).unwrap_or("dem Internet");
        format!(
            "Dieser Schritt lädt ein Installationsskript von {} herunter und führt es auf deinem Rechner aus. Fortfahren?",
            url.trim_matches(|c| c == '"' || c == ')' || c == '\'')
        )
    })
}

fn install_action(name: &str, pkg: &Pkg) -> Option<Action> {
    let os = std::env::consts::OS;
    let cmd = match os {
        "macos" => pkg.brew.map(|b| format!("brew install {b}")).or(pkg.unix.map(String::from)),
        "windows" => pkg.winget.map(|w| format!("winget install --id {w} -e")),
        _ => match linux_pm() {
            Some("apt-get") => pkg.apt.map(|p| format!("sudo apt-get install {p}")),
            Some("dnf") => pkg.dnf.map(|p| format!("sudo dnf install {p}")),
            Some("pacman") => pkg.pacman.map(|p| format!("sudo pacman -S {p}")),
            _ => None,
        }
        .or(pkg.unix.map(String::from)),
    }?;
    Some(Action {
        label: format!("{name} installieren"),
        confirm: script_confirm(&cmd),
        command: Some(cmd),
        url: None,
        in_project: false,
    })
}

fn cmd_action(label: &str, cmd: String, in_project: bool) -> Action {
    Action { label: label.into(), confirm: script_confirm(&cmd), command: Some(cmd), url: None, in_project }
}

fn url_action(label: &str, url: &str) -> Action {
    Action { label: label.into(), command: None, url: Some(url.into()), confirm: None, in_project: false }
}

// ---------------------------------------------------------------------------
// Tool catalog
// ---------------------------------------------------------------------------

struct Tool {
    id: &'static str,
    name: &'static str,
    program: &'static str,
    args: &'static [&'static str],
    version_re: &'static str,
    pkg: Pkg,
    docs: &'static str,
    only_os: Option<&'static str>,
}

fn catalog() -> Vec<Tool> {
    let python_prog = if cfg!(windows) { "python" } else { "python3" };
    vec![
        Tool {
            id: "git", name: "Git", program: "git", args: &["--version"], version_re: r"git version (\S+)",
            pkg: Pkg { brew: Some("git"), winget: Some("Git.Git"), apt: Some("git"), dnf: Some("git"), pacman: Some("git"), ..NONE },
            docs: "https://git-scm.com/downloads", only_os: None,
        },
        Tool {
            id: "node", name: "Node.js", program: "node", args: &["--version"], version_re: r"v?(\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("node"), winget: Some("OpenJS.NodeJS.LTS"), apt: Some("nodejs npm"), dnf: Some("nodejs"), pacman: Some("nodejs npm"), ..NONE },
            docs: "https://nodejs.org/", only_os: None,
        },
        Tool {
            id: "npm", name: "npm", program: "npm", args: &["--version"], version_re: r"(\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("node"), winget: Some("OpenJS.NodeJS.LTS"), apt: Some("npm"), dnf: Some("npm"), pacman: Some("npm"), ..NONE },
            docs: "https://nodejs.org/", only_os: None,
        },
        Tool {
            id: "pnpm", name: "pnpm", program: "pnpm", args: &["--version"], version_re: r"(\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("pnpm"), winget: Some("pnpm.pnpm"), unix: Some("npm install -g pnpm"), ..NONE },
            docs: "https://pnpm.io/installation", only_os: None,
        },
        Tool {
            id: "yarn", name: "Yarn", program: "yarn", args: &["--version"], version_re: r"(\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("yarn"), winget: Some("Yarn.Yarn"), unix: Some("npm install -g yarn"), ..NONE },
            docs: "https://yarnpkg.com/getting-started/install", only_os: None,
        },
        Tool {
            id: "flutter", name: "Flutter SDK", program: "flutter", args: &["--version"], version_re: r"Flutter (\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("--cask flutter"), unix: Some("sudo snap install flutter --classic"), ..NONE },
            docs: "https://docs.flutter.dev/get-started/install", only_os: None,
        },
        Tool {
            id: "dart", name: "Dart SDK", program: "dart", args: &["--version"], version_re: r"Dart SDK version: (\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("--cask flutter"), ..NONE },
            docs: "https://dart.dev/get-dart", only_os: None,
        },
        Tool {
            id: "fvm", name: "FVM (Flutter Version Manager)", program: "fvm", args: &["--version"], version_re: r"(\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("leoafarias/fvm/fvm"), unix: Some("dart pub global activate fvm"), winget: None, ..NONE },
            docs: "https://fvm.app/documentation/getting-started/installation", only_os: None,
        },
        Tool {
            id: "java", name: "Java JDK", program: "java", args: &["-version"], version_re: r#"version "(\d+(?:\.\d+)*)"#,
            pkg: Pkg { brew: Some("--cask temurin@17"), winget: Some("EclipseAdoptium.Temurin.17.JDK"), apt: Some("openjdk-17-jdk"), dnf: Some("java-17-openjdk-devel"), pacman: Some("jdk17-openjdk"), ..NONE },
            docs: "https://adoptium.net/", only_os: None,
        },
        Tool {
            id: "adb", name: "Android Platform-Tools (adb)", program: "adb", args: &["version"], version_re: r"Android Debug Bridge version (\S+)",
            pkg: Pkg { brew: Some("--cask android-platform-tools"), winget: Some("Google.PlatformTools"), apt: Some("adb"), dnf: Some("android-tools"), pacman: Some("android-tools"), ..NONE },
            docs: "https://developer.android.com/tools/releases/platform-tools", only_os: None,
        },
        Tool {
            id: "sdkmanager", name: "Android SDK Command-line Tools", program: "sdkmanager", args: &["--version"], version_re: r"(\d+\.\d+(?:\.\d+)?)",
            pkg: Pkg { brew: Some("--cask android-commandlinetools"), winget: Some("Google.AndroidStudio"), ..NONE },
            docs: "https://developer.android.com/studio#command-line-tools-only", only_os: None,
        },
        Tool {
            id: "xcode", name: "Xcode", program: "xcodebuild", args: &["-version"], version_re: r"Xcode (\S+)",
            pkg: NONE, docs: "https://developer.apple.com/xcode/", only_os: Some("macos"),
        },
        Tool {
            id: "cocoapods", name: "CocoaPods", program: "pod", args: &["--version"], version_re: r"(\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("cocoapods"), ..NONE },
            docs: "https://guides.cocoapods.org/using/getting-started.html", only_os: Some("macos"),
        },
        Tool {
            id: "rust", name: "Rust (rustc + cargo)", program: "rustc", args: &["--version"], version_re: r"rustc (\d+\.\d+\.\d+)",
            pkg: Pkg { winget: Some("Rustlang.Rustup"), unix: Some("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"), ..NONE },
            docs: "https://rustup.rs/", only_os: None,
        },
        Tool {
            id: "python", name: "Python 3", program: python_prog, args: &["--version"], version_re: r"Python (\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("python"), winget: Some("Python.Python.3.12"), apt: Some("python3 python3-venv python3-pip"), dnf: Some("python3 python3-pip"), pacman: Some("python python-pip"), ..NONE },
            docs: "https://www.python.org/downloads/", only_os: None,
        },
        Tool {
            id: "uv", name: "uv (Python)", program: "uv", args: &["--version"], version_re: r"uv (\d+\.\d+\.\d+)",
            pkg: Pkg { brew: Some("uv"), winget: Some("astral-sh.uv"), unix: Some("curl -LsSf https://astral.sh/uv/install.sh | sh"), ..NONE },
            docs: "https://docs.astral.sh/uv/", only_os: None,
        },
        Tool {
            id: "docker", name: "Docker", program: "docker", args: &["--version"], version_re: r"Docker version ([\d.]+)",
            pkg: Pkg { brew: Some("--cask docker"), winget: Some("Docker.DockerDesktop"), apt: Some("docker.io docker-compose-v2"), dnf: Some("docker"), pacman: Some("docker docker-compose"), ..NONE },
            docs: "https://docs.docker.com/get-docker/", only_os: None,
        },
        Tool {
            id: "ssh", name: "OpenSSH-Client", program: "ssh", args: &["-V"], version_re: r"OpenSSH_([\w.]+)",
            pkg: Pkg { apt: Some("openssh-client"), dnf: Some("openssh-clients"), pacman: Some("openssh"), ..NONE },
            docs: "https://learn.microsoft.com/windows-server/administration/openssh/openssh_install_firstuse", only_os: None,
        },
        Tool {
            id: "tar", name: "tar", program: "tar", args: &["--version"], version_re: r"(\d+\.\d+(?:\.\d+)?)",
            pkg: Pkg { apt: Some("tar"), dnf: Some("tar"), pacman: Some("tar"), ..NONE },
            docs: "https://www.gnu.org/software/tar/", only_os: None,
        },
        Tool {
            id: "gh", name: "GitHub CLI", program: "gh", args: &["--version"], version_re: r"gh version (\S+)",
            pkg: Pkg { brew: Some("gh"), winget: Some("GitHub.cli"), apt: Some("gh"), dnf: Some("gh"), pacman: Some("github-cli"), ..NONE },
            docs: "https://cli.github.com/", only_os: None,
        },
    ]
}

// ---------------------------------------------------------------------------
// Version helpers
// ---------------------------------------------------------------------------

fn numeric_parts(v: &str) -> Vec<u64> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+(?:\.\d+){0,3}").unwrap());
    RE.find(v)
        .map(|m| m.as_str().split('.').filter_map(|p| p.parse().ok()).collect())
        .unwrap_or_default()
}

fn to_semver(v: &str) -> Option<semver::Version> {
    let p = numeric_parts(v);
    if p.is_empty() {
        return None;
    }
    Some(semver::Version::new(p[0], *p.get(1).unwrap_or(&0), *p.get(2).unwrap_or(&0)))
}

/// `None` if the constraint cannot be interpreted (e.g. "stable", "lts/*").
pub fn satisfies(version: &str, constraint: &str, mode: &str) -> Option<bool> {
    let c = constraint.trim();
    if mode == "exact" {
        let want = numeric_parts(c);
        if want.is_empty() {
            return None;
        }
        let have = numeric_parts(version);
        return Some(want.iter().enumerate().all(|(i, w)| have.get(i) == Some(w)));
    }
    let have = to_semver(version)?;
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(>=|<=|~=|==|!=|>|<|=|\^|~)?\s*v?(\d+(?:\.(?:\d+|x|\*)){0,2})").unwrap()
    });
    let mut any_alt = false;
    for alt in c.split("||") {
        let mut comps = vec![];
        for cap in TOKEN.captures_iter(alt) {
            let op = match cap.get(1).map(|m| m.as_str()).unwrap_or("") {
                "==" => "=",
                "~=" => "~",
                "!=" => return None,
                o => o,
            };
            let ver = cap[2].replace('x', "*");
            comps.push(format!("{op}{ver}"));
        }
        if comps.is_empty() {
            continue;
        }
        let req = semver::VersionReq::parse(&comps.join(", ")).ok()?;
        any_alt = true;
        if req.matches(&have) {
            return Some(true);
        }
    }
    if any_alt { Some(false) } else { None }
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

fn run_version(tool: &Tool) -> (bool, Option<String>) {
    let Some(out) = proc::run(tool.program, tool.args, Duration::from_secs(25)) else {
        return (false, None);
    };
    let re = Regex::new(tool.version_re).unwrap();
    let version = re.captures(&out.text).map(|c| c[1].to_string());
    // Windows ships a "python" stub that only opens the Store.
    if tool.id == "python" && version.is_none() {
        return (false, None);
    }
    (true, version)
}

fn android_home() -> Option<PathBuf> {
    std::env::var_os("ANDROID_HOME")
        .map(PathBuf::from)
        .or_else(|| android_sdk_candidates().into_iter().find(|p| p.is_dir()))
}

fn base_status(id: &str, name: &str) -> ToolStatus {
    ToolStatus {
        id: id.into(),
        name: name.into(),
        installed: false,
        version: None,
        ok: false,
        required: None,
        required_source: None,
        message: None,
        actions: vec![],
        docs_url: None,
        optional: false,
    }
}

fn check_special(id: &str, constraints: &[ConstraintIn]) -> Option<ToolStatus> {
    let os = std::env::consts::OS;
    match id {
        "android-licenses" => {
            let mut s = base_status(id, "Android SDK-Lizenzen");
            let accepted = android_home()
                .map(|h| h.join("licenses").join("android-sdk-license").is_file())
                .unwrap_or(false);
            s.installed = accepted;
            s.ok = accepted;
            if !accepted {
                s.message = Some("Die Android-SDK-Lizenzen wurden noch nicht akzeptiert. Du wirst jede Lizenz einzeln bestätigen.".into());
                let cmd = if which("sdkmanager").is_some() {
                    "sdkmanager --licenses"
                } else {
                    "flutter doctor --android-licenses"
                };
                s.actions.push(cmd_action("Lizenzen prüfen & akzeptieren", cmd.into(), false));
            }
            s.docs_url = Some("https://developer.android.com/studio/intro/update#download-with-gradle".into());
            Some(s)
        }
        "android-platform" => {
            let level = constraints.iter().find(|c| c.tool == "android-platform")?.constraint.clone();
            let mut s = base_status(id, &format!("Android SDK Platform {level}"));
            let installed = android_home()
                .map(|h| h.join("platforms").join(format!("android-{level}")).is_dir())
                .unwrap_or(false);
            s.installed = installed;
            s.ok = installed;
            s.required = Some(level.clone());
            s.required_source = constraints.iter().find(|c| c.tool == "android-platform").map(|c| c.source.clone());
            if !installed {
                s.message = Some(format!("Das Projekt kompiliert gegen Android API {level}."));
                s.actions.push(cmd_action(
                    &format!("API {level} installieren"),
                    format!("sdkmanager \"platforms;android-{level}\" \"platform-tools\""),
                    false,
                ));
            }
            Some(s)
        }
        "xcode-license" if os == "macos" => {
            let mut s = base_status(id, "Xcode-Lizenz & Erststart");
            let license = proc::run("xcodebuild", &["-license", "check"], Duration::from_secs(15)).map(|o| o.ok).unwrap_or(false);
            let first = proc::run("xcodebuild", &["-checkFirstLaunchStatus"], Duration::from_secs(15)).map(|o| o.ok).unwrap_or(false);
            s.installed = license && first;
            s.ok = s.installed;
            if !s.ok {
                s.message = Some("Die Xcode-Lizenz muss akzeptiert und die Erstinstallation abgeschlossen werden (benötigt dein Admin-Passwort).".into());
                s.actions.push(Action {
                    label: "Xcode-Lizenz lesen & akzeptieren".into(),
                    command: Some("sudo xcodebuild -license accept && sudo xcodebuild -runFirstLaunch".into()),
                    url: None,
                    confirm: Some("Mit dem Fortfahren akzeptierst du die Xcode- und Apple-SDK-Lizenzvereinbarung. Du kannst sie vorher unter https://www.apple.com/legal/sla/ nachlesen oder im Terminal mit „sudo xcodebuild -license“ vollständig anzeigen.".into()),
                    in_project: false,
                });
            }
            Some(s)
        }
        "docker-daemon" => {
            let mut s = base_status(id, "Docker läuft");
            let running = proc::run("docker", &["info", "--format", "{{.ServerVersion}}"], Duration::from_secs(10));
            s.installed = running.as_ref().map(|o| o.ok).unwrap_or(false);
            s.ok = s.installed;
            if s.ok {
                s.version = running.map(|o| o.text.trim().to_string());
            } else {
                s.message = Some("Der Docker-Dienst ist nicht gestartet.".into());
                let cmd = match os {
                    "macos" => "open -a Docker",
                    "windows" => "start \"\" \"C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe\"",
                    _ => "sudo systemctl start docker",
                };
                s.actions.push(cmd_action("Docker starten", cmd.into(), false));
            }
            Some(s)
        }
        "homebrew" if os == "macos" => {
            let mut s = base_status(id, "Homebrew");
            s.installed = which("brew").is_some();
            s.ok = s.installed;
            if !s.ok {
                s.message = Some("Homebrew wird für die Ein-Klick-Installation auf macOS verwendet.".into());
                s.actions.push(cmd_action(
                    "Homebrew installieren",
                    "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"".into(),
                    false,
                ));
            }
            s.docs_url = Some("https://brew.sh/".into());
            Some(s)
        }
        _ => None,
    }
}

fn fix_actions(tool: &str, required: &str, mode: &str) -> Vec<Action> {
    let mut v = vec![];
    match tool {
        "flutter" | "dart" => {
            if mode == "exact" {
                if which("fvm").is_some() {
                    v.push(cmd_action(
                        &format!("Flutter {required} per FVM installieren"),
                        format!("fvm install {required} && fvm use {required} --force"),
                        true,
                    ));
                } else {
                    v.push(cmd_action("FVM installieren", "dart pub global activate fvm".into(), false));
                }
            } else {
                v.push(cmd_action("Flutter aktualisieren", "flutter upgrade".into(), false));
            }
        }
        "node" => {
            let major = numeric_parts(required).first().copied().unwrap_or(20);
            if which("fnm").is_some() {
                v.push(cmd_action(&format!("Node {major} per fnm"), format!("fnm install {major} && fnm default {major}"), false));
            } else if which("volta").is_some() {
                v.push(cmd_action(&format!("Node {major} per Volta"), format!("volta install node@{major}"), false));
            } else if cfg!(target_os = "macos") {
                v.push(cmd_action(
                    &format!("Node {major} installieren"),
                    format!("brew install node@{major} && brew link --overwrite --force node@{major}"),
                    false,
                ));
            } else if cfg!(windows) {
                v.push(cmd_action("fnm installieren (Node-Versionsmanager)", "winget install --id Schniz.fnm -e".into(), false));
            } else {
                v.push(cmd_action("fnm installieren (Node-Versionsmanager)", "curl -fsSL https://fnm.vercel.app/install | bash".into(), false));
            }
        }
        "python" => {
            let ver = numeric_parts(required).iter().take(2).map(|n| n.to_string()).collect::<Vec<_>>().join(".");
            if which("uv").is_some() {
                v.push(cmd_action(&format!("Python {ver} per uv"), format!("uv python install {ver}"), false));
            } else if which("pyenv").is_some() {
                v.push(cmd_action(&format!("Python {ver} per pyenv"), format!("pyenv install {ver}"), false));
            } else if cfg!(target_os = "macos") {
                v.push(cmd_action(&format!("Python {ver} installieren"), format!("brew install python@{ver}"), false));
            }
        }
        "rust" => {
            v.push(cmd_action(&format!("Rust {required} installieren"), format!("rustup toolchain install {required}"), true));
        }
        _ => {}
    }
    v
}

fn check_one(id: &str, constraints: &[ConstraintIn]) -> Option<ToolStatus> {
    if let Some(s) = check_special(id, constraints) {
        return Some(s);
    }
    let os = std::env::consts::OS;
    let cat = catalog();
    let tool = cat.into_iter().find(|t| t.id == id)?;
    if let Some(only) = tool.only_os {
        if only != os {
            return None;
        }
    }
    let mut s = base_status(tool.id, tool.name);
    s.docs_url = Some(tool.docs.into());
    let (installed, version) = run_version(&tool);
    s.installed = installed;
    s.version = version.clone();
    s.ok = installed;

    if !installed {
        s.message = Some(format!("{} wurde nicht gefunden.", tool.name));
        if tool.id == "xcode" {
            s.actions.push(url_action("Xcode im App Store öffnen", "macappstore://apps.apple.com/app/id497799835"));
        } else if let Some(a) = install_action(tool.name, &tool.pkg) {
            if os == "macos" && a.command.as_deref().is_some_and(|c| c.starts_with("brew ")) && which("brew").is_none() {
                if let Some(brew) = check_special("homebrew", &[]) {
                    s.actions.extend(brew.actions);
                }
            }
            s.actions.push(a);
        }
        s.actions.push(url_action("Anleitung öffnen", tool.docs));
        return Some(s);
    }

    if let Some(c) = constraints.iter().find(|c| c.tool == tool.id) {
        s.required = Some(c.constraint.clone());
        s.required_source = Some(c.source.clone());
        if let Some(v) = &version {
            match satisfies(v, &c.constraint, &c.mode) {
                Some(true) => {}
                Some(false) => {
                    s.ok = false;
                    s.message = Some(format!(
                        "Installiert ist {v}, das Projekt verlangt {} ({}).",
                        c.constraint, c.source
                    ));
                    s.actions = fix_actions(tool.id, &c.constraint, &c.mode);
                }
                None => {}
            }
        }
    }
    Some(s)
}

#[tauri::command]
pub async fn check_tools(ids: Vec<String>, constraints: Vec<ConstraintIn>) -> Result<Vec<ToolStatus>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let handles: Vec<_> = ids
            .into_iter()
            .map(|id| {
                let c = constraints.clone();
                std::thread::spawn(move || check_one(&id, &c))
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::satisfies;

    #[test]
    fn ranges() {
        assert_eq!(satisfies("3.5.2", ">=3.0.0 <4.0.0", "range"), Some(true));
        assert_eq!(satisfies("2.19.0", ">=3.0.0 <4.0.0", "range"), Some(false));
        assert_eq!(satisfies("20.11.1", ">=18", "range"), Some(true));
        assert_eq!(satisfies("16.0.0", "^18 || ^20", "range"), Some(false));
        assert_eq!(satisfies("20.1.0", "^18 || ^20", "range"), Some(true));
        assert_eq!(satisfies("3.12.1", ">=3.10", "range"), Some(true));
        assert_eq!(satisfies("20.1.0", "18.x", "range"), Some(false));
        assert_eq!(satisfies("20.1.0", "lts/*", "range"), None);
    }

    #[test]
    fn exact() {
        assert_eq!(satisfies("20.11.1", "20", "exact"), Some(true));
        assert_eq!(satisfies("3.24.0", "3.22.1", "exact"), Some(false));
        assert_eq!(satisfies("3.24.0", "stable", "exact"), None);
    }
}
