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
        let server = deps.contains_key("express")
            || deps.contains_key("fastify")
            || deps.contains_key("@nestjs/core")
            || deps.contains_key("next")
            || deps.contains_key("nuxt");
        let mut d = HashMap::new();
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
