//! Project detection: figures out what kind of project lives in a folder
//! (Flutter, React Native, Tauri, Node, Python, Docker – several may apply)
//! and which tool versions it asks for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DetectedType {
    /// flutter | react-native | tauri | node | python | docker
    pub kind: String,
    pub label: String,
    pub details: HashMap<String, String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VersionConstraint {
    /// Tool id from the requirements catalog (flutter, dart, node, python, rust, android-platform).
    pub tool: String,
    pub constraint: String,
    /// `exact` = pinned version (e.g. .nvmrc); `range` = semver-ish range.
    pub mode: String,
    pub source: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    pub branch: Option<String>,
    pub remote: Option<String>,
    pub github: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub path: String,
    pub name: String,
    pub types: Vec<DetectedType>,
    pub constraints: Vec<VersionConstraint>,
    pub git: Option<GitInfo>,
}

fn read(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&read(p)?).ok()
}

fn constraint(tool: &str, c: &str, mode: &str, source: &str) -> VersionConstraint {
    VersionConstraint {
        tool: tool.into(),
        constraint: c.trim().trim_matches('"').trim_matches('\'').to_string(),
        mode: mode.into(),
        source: source.into(),
    }
}

#[tauri::command]
pub fn detect_project(path: String) -> Result<ProjectInfo, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("Ordner nicht gefunden: {path}"));
    }
    let mut name = root.file_name().unwrap_or_default().to_string_lossy().to_string();
    let mut types = vec![];
    let mut constraints = vec![];

    let pkg = read_json(&root.join("package.json"));
    let deps: HashMap<String, String> = pkg
        .as_ref()
        .map(|p| {
            let mut m = HashMap::new();
            for key in ["dependencies", "devDependencies"] {
                if let Some(obj) = p[key].as_object() {
                    for (k, v) in obj {
                        m.insert(k.clone(), v.as_str().unwrap_or_default().to_string());
                    }
                }
            }
            m
        })
        .unwrap_or_default();
    if let Some(n) = pkg.as_ref().and_then(|p| p["name"].as_str()) {
        name = n.to_string();
    }

    // --- Flutter -----------------------------------------------------------
    if let Some(pubspec) = read(&root.join("pubspec.yaml")) {
        if let Ok(y) = serde_yaml::from_str::<serde_yaml::Value>(&pubspec) {
            let is_flutter = y["dependencies"]["flutter"].is_mapping() || y["flutter"].is_mapping();
            if let Some(n) = y["name"].as_str() {
                name = n.to_string();
            }
            if let Some(sdk) = y["environment"]["sdk"].as_str() {
                constraints.push(constraint("dart", sdk, "range", "pubspec.yaml"));
            }
            if let Some(f) = y["environment"]["flutter"].as_str() {
                constraints.push(constraint("flutter", f, "range", "pubspec.yaml"));
            }
            if is_flutter {
                let mut d = HashMap::new();
                let platforms: Vec<&str> = ["android", "ios", "web", "macos", "windows", "linux"]
                    .into_iter()
                    .filter(|p| root.join(p).is_dir())
                    .collect();
                d.insert("platforms".into(), platforms.join(","));
                if let Some(id) = ios_bundle_id(&root.join("ios")) {
                    d.insert("iosBundleId".into(), id);
                }
                if let Some(v) = fvm_version(&root) {
                    d.insert("fvm".into(), v.clone());
                    constraints.push(constraint("flutter", &v, "exact", "FVM"));
                }
                if let Some(sdk) = android_compile_sdk(&root.join("android").join("app")) {
                    constraints.push(constraint("android-platform", &sdk, "exact", "android/app/build.gradle"));
                }
                types.push(DetectedType { kind: "flutter".into(), label: "Flutter".into(), details: d });
            } else {
                types.push(DetectedType { kind: "dart".into(), label: "Dart".into(), details: HashMap::new() });
            }
        }
    }

    // --- React Native / Expo --------------------------------------------------
    if deps.contains_key("react-native") || deps.contains_key("expo") {
        let mut d = HashMap::new();
        d.insert("expo".into(), deps.contains_key("expo").to_string());
        d.insert("packageManager".into(), package_manager(&root).into());
        d.insert("android".into(), root.join("android").is_dir().to_string());
        d.insert("ios".into(), root.join("ios").is_dir().to_string());
        if let Some(sdk) = android_compile_sdk(&root.join("android").join("app")) {
            constraints.push(constraint("android-platform", &sdk, "exact", "android/app/build.gradle"));
        }
        let label = if deps.contains_key("expo") { "React Native (Expo)" } else { "React Native" };
        types.push(DetectedType { kind: "react-native".into(), label: label.into(), details: d });
    }

    // --- Tauri -----------------------------------------------------------------
    let tauri_dir = root.join("src-tauri");
    if tauri_dir.join("tauri.conf.json").is_file() || tauri_dir.join("Tauri.toml").is_file() {
        let mut d = HashMap::new();
        if let Some(conf) = read_json(&tauri_dir.join("tauri.conf.json")) {
            if let Some(p) = conf["productName"].as_str() {
                d.insert("productName".into(), p.into());
            }
        }
        d.insert("packageManager".into(), package_manager(&root).into());
        d.insert("android".into(), tauri_dir.join("gen").join("android").is_dir().to_string());
        d.insert("ios".into(), tauri_dir.join("gen").join("apple").is_dir().to_string());
        for f in ["rust-toolchain.toml", "rust-toolchain"] {
            for dir in [&root, &tauri_dir] {
                if let Some(t) = read(&dir.join(f)) {
                    let channel = toml::from_str::<toml::Value>(&t)
                        .ok()
                        .and_then(|v| v.get("toolchain")?.get("channel")?.as_str().map(String::from))
                        .unwrap_or_else(|| t.trim().to_string());
                    if channel.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        constraints.push(constraint("rust", &channel, "exact", f));
                    }
                }
            }
        }
        types.push(DetectedType { kind: "tauri".into(), label: "Tauri".into(), details: d });
    }

    // --- Node / Web --------------------------------------------------------------
    if let Some(p) = &pkg {
        let is_rn = types.iter().any(|t| t.kind == "react-native");
        let is_tauri = types.iter().any(|t| t.kind == "tauri");
        let scripts: Vec<String> = p["scripts"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        let framework = [
            ("electron", "Electron"),
            ("next", "Next.js"),
            ("nuxt", "Nuxt"),
            ("@angular/core", "Angular"),
            ("@sveltejs/kit", "SvelteKit"),
            ("astro", "Astro"),
            ("vite", "Vite"),
            ("react-scripts", "Create React App"),
            ("@nestjs/core", "NestJS"),
            ("express", "Express"),
            ("fastify", "Fastify"),
            ("vue", "Vue"),
            ("react", "React"),
        ]
        .iter()
        .find(|(dep, _)| deps.contains_key(*dep))
        .map(|(_, l)| l.to_string());
        let output = if deps.contains_key("next") {
            if root.join("out").is_dir() { "out" } else { ".next" }
        } else if deps.contains_key("react-scripts") {
            "build"
        } else {
            "dist"
        };
        let server = !deps.contains_key("electron")
            && (deps.contains_key("express")
            || deps.contains_key("fastify")
            || deps.contains_key("@nestjs/core")
            || deps.contains_key("next")
            || deps.contains_key("nuxt"));
        let mut d = HashMap::new();
        d.extend(installer_info(&root, p, &deps, std::env::consts::OS));
        d.insert("packageManager".into(), package_manager(&root).into());
        d.insert("scripts".into(), scripts.join(","));
        d.insert("outputDir".into(), output.into());
        d.insert("server".into(), server.to_string());
        if let Some(f) = &framework {
            d.insert("framework".into(), f.clone());
        }
        let label = match &framework {
            Some(f) => format!("Node · {f}"),
            None => "Node.js".into(),
        };
        if !is_rn && !is_tauri {
            types.push(DetectedType { kind: "node".into(), label, details: d });
        }
        if let Some(e) = p["engines"]["node"].as_str() {
            constraints.push(constraint("node", e, "range", "package.json engines"));
        }
        if let Some(v) = p["volta"]["node"].as_str() {
            constraints.push(constraint("node", v, "exact", "package.json volta"));
        }
    }
    for f in [".nvmrc", ".node-version"] {
        if let Some(v) = read(&root.join(f)) {
            let v = v.trim().trim_start_matches('v');
            if v.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                constraints.push(constraint("node", v, "exact", f));
            }
        }
    }

    // --- Python ------------------------------------------------------------------
    let py_markers = ["pyproject.toml", "requirements.txt", "setup.py", "Pipfile", "manage.py"];
    if py_markers.iter().any(|m| root.join(m).is_file()) {
        let mut d = HashMap::new();
        let reqs = read(&root.join("requirements.txt")).unwrap_or_default().to_lowercase();
        let pyproject = read(&root.join("pyproject.toml")).unwrap_or_default();
        let all = format!("{reqs}\n{}", pyproject.to_lowercase());
        let framework = [
            ("django", "Django"),
            ("fastapi", "FastAPI"),
            ("flask", "Flask"),
            ("streamlit", "Streamlit"),
        ]
        .iter()
        .find(|(k, _)| all.contains(k))
        .map(|(_, l)| l.to_string());
        let entry = ["manage.py", "main.py", "app.py", "server.py", "run.py"]
            .iter()
            .find(|f| root.join(f).is_file())
            .map(|s| s.to_string());
        let tool = if root.join("uv.lock").is_file() {
            "uv"
        } else if root.join("poetry.lock").is_file() {
            "poetry"
        } else if root.join("Pipfile").is_file() {
            "pipenv"
        } else {
            "pip"
        };
        d.insert("tool".into(), tool.into());
        if let Some(e) = entry {
            d.insert("entry".into(), e);
        }
        if let Some(f) = &framework {
            d.insert("framework".into(), f.clone());
        }
        if root.join("requirements.txt").is_file() {
            d.insert("requirements".into(), "requirements.txt".into());
        }
        if let Ok(t) = toml::from_str::<toml::Value>(&pyproject) {
            if let Some(n) = t.get("project").and_then(|p| p.get("name")).and_then(|n| n.as_str()) {
                if pkg.is_none() {
                    name = n.to_string();
                }
            }
            if let Some(r) = t.get("project").and_then(|p| p.get("requires-python")).and_then(|n| n.as_str()) {
                constraints.push(constraint("python", r, "range", "pyproject.toml"));
            }
        }
        if let Some(v) = read(&root.join(".python-version")) {
            if let Some(first) = v.lines().next() {
                constraints.push(constraint("python", first, "exact", ".python-version"));
            }
        }
        let label = match &framework {
            Some(f) => format!("Python · {f}"),
            None => "Python".into(),
        };
        types.push(DetectedType { kind: "python".into(), label, details: d });
    }

    // --- Docker ------------------------------------------------------------------
    let compose = ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"]
        .iter()
        .find(|f| root.join(f).is_file())
        .map(|s| s.to_string());
    if root.join("Dockerfile").is_file() || compose.is_some() {
        let mut d = HashMap::new();
        d.insert("dockerfile".into(), root.join("Dockerfile").is_file().to_string());
        if let Some(c) = compose {
            d.insert("compose".into(), c);
        }
        types.push(DetectedType { kind: "docker".into(), label: "Docker".into(), details: d });
    }

    Ok(ProjectInfo {
        path,
        name,
        types,
        constraints,
        git: git_info(&root),
    })
}

// ---------------------------------------------------------------------------
// Desktop installers (Electron & co.)
// ---------------------------------------------------------------------------

const MAC: &[&str] = &["mac", "macos", "osx", "darwin", "dmg"];
const WIN: &[&str] = &["win", "windows", "win32", "win64", "exe", "nsis", "msi"];
const LINUX: &[&str] = &["linux", "appimage", "deb", "rpm", "snap"];

fn script_tokens(body: &str) -> Vec<&str> {
    body.split(|c: char| c.is_whitespace() || "&;|\"'()".contains(c)).filter(|t| !t.is_empty()).collect()
}

/// Scripts that `name` runs via `npm run x`, `yarn x`, `pnpm x`, … (transitively).
fn referenced_scripts<'a>(scripts: &'a HashMap<String, String>, name: &str, depth: usize, out: &mut Vec<&'a str>) {
    let Some(body) = scripts.get(name) else { return };
    let tokens = script_tokens(body);
    for (i, tok) in tokens.iter().enumerate() {
        let prev = if i > 0 { tokens[i - 1] } else { "" };
        let runner = ["run", "yarn", "pnpm", "bun", "run-s", "run-p", "npm-run-all"].contains(&prev);
        if let Some((key, _)) = scripts.get_key_value(*tok) {
            if runner && key != name && !out.contains(&key.as_str()) {
                out.push(key.as_str());
                if depth < 5 {
                    referenced_scripts(scripts, key, depth + 1, out);
                }
            }
        }
    }
}

/// The body of a script including everything it runs.
fn expanded_body(scripts: &HashMap<String, String>, name: &str) -> String {
    let mut refs = vec![];
    referenced_scripts(scripts, name, 0, &mut refs);
    let mut body = scripts.get(name).cloned().unwrap_or_default();
    for r in refs {
        body.push('\n');
        body.push_str(&scripts[r]);
    }
    body
}

/// Which packager a script body uses to create installers.
fn packager_in(body: &str) -> Option<&'static str> {
    if body.contains("electron-builder") && !body.contains("install-app-deps") {
        Some("electron-builder")
    } else if body.contains("electron-forge make") || body.contains("forge make") {
        Some("electron-forge")
    } else if body.contains("electron-packager") || body.contains("@electron/packager") {
        Some("electron-packager")
    } else {
        None
    }
}

/// Picks the npm script that builds an installer for `os` (e.g. `dist:mac`)
/// and where the packager writes its output.
fn installer_info(root: &Path, pkg: &Value, deps: &HashMap<String, String>, os: &str) -> HashMap<String, String> {
    let scripts: HashMap<String, String> = pkg["scripts"]
        .as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect())
        .unwrap_or_default();
    let (mine, others): (&[&str], Vec<&[&str]>) = match os {
        "macos" => (MAC, vec![WIN, LINUX]),
        "windows" => (WIN, vec![MAC, LINUX]),
        _ => (LINUX, vec![MAC, WIN]),
    };
    let flag = |w: &[&str]| format!("--{}", w[0]);

    let mut best: Option<(i32, &str, &'static str)> = None;
    for name in scripts.keys() {
        let lower = name.to_lowercase();
        let base = lower.trim_start_matches("pre").trim_start_matches("post");
        // Lifecycle hooks (postinstall runs `electron-builder install-app-deps`)
        // and anything that publishes a release are never picked.
        if ["install", "prepare", "prepublish", "prepublishonly", "prepack", "postpack"].contains(&lower.as_str())
            || (base != lower && scripts.contains_key(base))
            || lower.contains("publish")
            || lower.contains("release")
            || lower.contains("deploy")
        {
            continue;
        }
        let body = expanded_body(&scripts, name);
        let Some(packager) = packager_in(&body) else { continue };
        if body.contains("--publish always") || body.contains("-p always") || body.contains("electron-forge publish") {
            continue;
        }
        // `--dir` only produces the unpacked app, no installer.
        if packager == "electron-builder" && script_tokens(&body).contains(&"--dir") {
            continue;
        }
        let segments: Vec<&str> = lower.split([':', '-', '_', '.']).collect();
        let named_mine = segments.iter().any(|s| mine.contains(s));
        let named_other = segments.iter().any(|s| others.iter().any(|o| o.contains(s)));
        let flags_mine = body.contains(&flag(mine));
        let flags_other = others.iter().any(|o| body.contains(&flag(o)));
        if (named_other && !named_mine) || (flags_other && !flags_mine && !named_mine) {
            continue;
        }
        let mut score = if named_mine || flags_mine { 30 } else { 20 };
        score += match segments[0] {
            "dist" | "make" | "installer" => 5,
            "package" | "pack" | "electron" => 3,
            "build" => 1,
            _ => 0,
        };
        if packager == "electron-packager" {
            score -= 10;
        }
        let better = match best {
            None => true,
            // Shorter names win ties: `dist:mac` over `dist:mac:universal`.
            Some((b, bn, _)) => score > b || (score == b && name.len() < bn.len()),
        };
        if better {
            best = Some((score, name.as_str(), packager));
        }
    }

    let mut d = HashMap::new();
    let packager = best.map(|b| b.2).or_else(|| {
        if deps.contains_key("electron-builder") {
            Some("electron-builder")
        } else if deps.keys().any(|k| k.starts_with("@electron-forge/")) {
            Some("electron-forge")
        } else if deps.contains_key("electron-packager") || deps.contains_key("@electron/packager") {
            Some("electron-packager")
        } else {
            None
        }
    });
    let Some(packager) = packager else { return d };
    d.insert("packager".into(), packager.into());
    if let Some((_, name, _)) = best {
        d.insert("installerScript".into(), name.into());
        let mut refs = vec![];
        referenced_scripts(&scripts, name, 0, &mut refs);
        let body = scripts.get(name).map(String::as_str).unwrap_or_default();
        let builds = name == "build" || refs.contains(&"build") || body.contains("vite build") || body.contains("electron-vite build");
        d.insert("installerRunsBuild".into(), builds.to_string());
    }
    let script_body = best.map(|b| expanded_body(&scripts, b.1)).unwrap_or_default();
    let output = match packager {
        "electron-builder" => electron_builder_output(root, pkg).unwrap_or_else(|| "dist".into()),
        "electron-forge" => "out/make".into(),
        _ => {
            let tokens = script_tokens(&script_body);
            tokens
                .iter()
                .enumerate()
                .find_map(|(i, t)| {
                    t.strip_prefix("--out=").map(String::from).or_else(|| (*t == "--out").then(|| tokens.get(i + 1).map(|s| s.to_string())).flatten())
                })
                .unwrap_or_else(|| ".".into())
        }
    };
    d.insert("installerOutput".into(), output.trim_end_matches('/').to_string());
    d
}

fn electron_builder_output(root: &Path, pkg: &Value) -> Option<String> {
    if let Some(o) = pkg["build"]["directories"]["output"].as_str() {
        return Some(o.into());
    }
    for f in ["electron-builder.json", "electron-builder.json5"] {
        if let Some(o) = read_json(&root.join(f)).and_then(|v| v["directories"]["output"].as_str().map(String::from)) {
            return Some(o);
        }
    }
    for f in ["electron-builder.yml", "electron-builder.yaml"] {
        let y = read(&root.join(f)).and_then(|t| serde_yaml::from_str::<serde_yaml::Value>(&t).ok());
        if let Some(o) = y.and_then(|v| v["directories"]["output"].as_str().map(String::from)) {
            return Some(o);
        }
    }
    None
}

fn package_manager(root: &Path) -> &'static str {
    if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if root.join("yarn.lock").is_file() {
        "yarn"
    } else if root.join("bun.lockb").is_file() || root.join("bun.lock").is_file() {
        "bun"
    } else {
        "npm"
    }
}

fn fvm_version(root: &Path) -> Option<String> {
    if let Some(v) = read_json(&root.join(".fvmrc")) {
        if let Some(s) = v["flutter"].as_str() {
            return Some(s.into());
        }
    }
    let v = read_json(&root.join(".fvm").join("fvm_config.json"))?;
    v["flutterSdkVersion"].as_str().map(String::from)
}

fn ios_bundle_id(ios_dir: &Path) -> Option<String> {
    let text = read(&ios_dir.join("Runner.xcodeproj").join("project.pbxproj"))?;
    let re = regex::Regex::new(r"PRODUCT_BUNDLE_IDENTIFIER = ([^;]+);").ok()?;
    let id = re
        .captures_iter(&text)
        .map(|c| c[1].trim().trim_matches('"').to_string())
        .find(|id| !id.ends_with("Tests") && !id.contains("$("));
    // Used in a shell command later – only accept valid bundle identifiers.
    id.filter(|v| v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
}

fn android_compile_sdk(app_dir: &Path) -> Option<String> {
    let text = read(&app_dir.join("build.gradle")).or_else(|| read(&app_dir.join("build.gradle.kts")))?;
    let re = regex::Regex::new(r"compileSdk(?:Version)?\s*=?\s*(\d{2})").ok()?;
    re.captures(&text).map(|c| c[1].to_string())
}

fn git_info(root: &Path) -> Option<GitInfo> {
    let git = root.join(".git");
    if !git.is_dir() {
        return None;
    }
    let branch = read(&git.join("HEAD"))
        .and_then(|h| h.trim().strip_prefix("ref: refs/heads/").map(String::from));
    let config = read(&git.join("config")).unwrap_or_default();
    let mut remote = None;
    let mut in_origin = false;
    for line in config.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_origin = l == "[remote \"origin\"]";
        } else if in_origin {
            if let Some(url) = l.strip_prefix("url = ") {
                remote = Some(url.to_string());
            }
        }
    }
    let github = remote.as_ref().and_then(|r| {
        let re = regex::Regex::new(r"github\.com[:/]([^/]+)/([^/]+?)(?:\.git)?/?$").ok()?;
        re.captures(r).map(|c| format!("{}/{}", &c[1], &c[2]))
    });
    Some(GitInfo { branch, remote, github })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_this_repo_as_tauri() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
        let info = detect_project(root.to_string_lossy().to_string()).unwrap();
        let tauri = info.types.iter().find(|t| t.kind == "tauri").expect("tauri erkannt");
        assert_eq!(tauri.details.get("packageManager").map(String::as_str), Some("npm"));
        assert_eq!(info.name, "easydeploy");
    }

    fn installer(scripts: serde_json::Value, deps: &[&str], os: &str) -> HashMap<String, String> {
        let pkg = serde_json::json!({ "scripts": scripts });
        let deps = deps.iter().map(|d| (d.to_string(), "1".to_string())).collect();
        installer_info(Path::new("/nonexistent"), &pkg, &deps, os)
    }

    #[test]
    fn picks_installer_script_for_os() {
        let scripts = serde_json::json!({
            "start": "electron .",
            "build": "tsc",
            "postinstall": "electron-builder install-app-deps",
            "pack": "electron-builder --dir",
            "dist": "npm run build && electron-builder",
            "dist:mac": "npm run build && electron-builder --mac",
            "dist:win": "npm run build && electron-builder --win",
            "release": "electron-builder --publish always"
        });
        let mac = installer(scripts.clone(), &["electron", "electron-builder"], "macos");
        assert_eq!(mac.get("installerScript").map(String::as_str), Some("dist:mac"));
        assert_eq!(mac.get("installerRunsBuild").map(String::as_str), Some("true"));
        assert_eq!(mac.get("installerOutput").map(String::as_str), Some("dist"));
        let win = installer(scripts.clone(), &["electron-builder"], "windows");
        assert_eq!(win.get("installerScript").map(String::as_str), Some("dist:win"));
        let linux = installer(scripts, &["electron-builder"], "linux");
        assert_eq!(linux.get("installerScript").map(String::as_str), Some("dist"));
    }

    #[test]
    fn follows_nested_scripts_and_forge() {
        let nested = installer(
            serde_json::json!({ "compile": "tsc", "electron:pack": "electron-builder -m", "dist": "npm run compile && npm run electron:pack" }),
            &[],
            "macos",
        );
        assert_eq!(nested.get("installerScript").map(String::as_str), Some("dist"));
        assert_eq!(nested.get("installerRunsBuild").map(String::as_str), Some("false"));
        let forge = installer(serde_json::json!({ "package": "electron-forge package", "make": "electron-forge make" }), &["@electron-forge/cli"], "macos");
        assert_eq!(forge.get("installerScript").map(String::as_str), Some("make"));
        assert_eq!(forge.get("installerOutput").map(String::as_str), Some("out/make"));
        let dep_only = installer(serde_json::json!({ "start": "electron ." }), &["electron-builder"], "macos");
        assert_eq!(dep_only.get("packager").map(String::as_str), Some("electron-builder"));
        assert!(dep_only.get("installerScript").is_none());
        assert!(installer(serde_json::json!({ "build": "vite build" }), &["vite"], "macos").is_empty());
    }

    #[test]
    fn detects_flutter_and_constraints() {
        let dir = std::env::temp_dir().join(format!("ed-flutter-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("android/app")).unwrap();
        std::fs::write(
            dir.join("pubspec.yaml"),
            "name: demo_app\nenvironment:\n  sdk: '>=3.3.0 <4.0.0'\ndependencies:\n  flutter:\n    sdk: flutter\n",
        )
        .unwrap();
        std::fs::write(dir.join(".fvmrc"), r#"{"flutter": "3.22.2"}"#).unwrap();
        std::fs::write(dir.join("android/app/build.gradle"), "android {\n    compileSdk = 34\n}\n").unwrap();
        let info = detect_project(dir.to_string_lossy().to_string()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(info.name, "demo_app");
        assert_eq!(info.types[0].kind, "flutter");
        assert!(info.constraints.iter().any(|c| c.tool == "dart" && c.constraint == ">=3.3.0 <4.0.0"));
        assert!(info.constraints.iter().any(|c| c.tool == "flutter" && c.constraint == "3.22.2" && c.mode == "exact"));
        assert!(info.constraints.iter().any(|c| c.tool == "android-platform" && c.constraint == "34"));
    }
}
