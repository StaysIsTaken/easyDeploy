//! macOS privacy permissions for Electron apps built with electron-builder.
//!
//! electron-builder signs with the hardened runtime by default and, when the
//! project does not configure entitlements, uses a template without
//! `com.apple.security.device.audio-input` / `…device.camera`. Such an app
//! can never access the microphone or camera. On top of that macOS needs
//! `NSMicrophoneUsageDescription` / `NSCameraUsageDescription` in Info.plist.
//! This module finds out whether the app uses those devices, whether the
//! build config covers them and can add the missing pieces.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

struct Permission {
    id: &'static str,
    label: &'static str,
    entitlement: &'static str,
    usage_key: &'static str,
    usage_text: &'static str,
}

const PERMISSIONS: &[Permission] = &[
    Permission {
        id: "microphone",
        label: "Mikrofon",
        entitlement: "com.apple.security.device.audio-input",
        usage_key: "NSMicrophoneUsageDescription",
        usage_text: "Diese App benötigt Zugriff auf das Mikrofon.",
    },
    Permission {
        id: "camera",
        label: "Kamera",
        entitlement: "com.apple.security.device.camera",
        usage_key: "NSCameraUsageDescription",
        usage_text: "Diese App benötigt Zugriff auf die Kamera.",
    },
];

/// electron-builder's own default entitlements, kept so the fix does not
/// remove anything the app got before.
const DEFAULT_ENTITLEMENTS: &[&str] = &[
    "com.apple.security.cs.allow-jit",
    "com.apple.security.cs.allow-unsigned-executable-memory",
    "com.apple.security.cs.disable-library-validation",
];

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PermissionIssue {
    pub id: String,
    pub label: String,
    /// Source file that uses the device.
    pub used_in: String,
    pub missing_entitlement: bool,
    pub missing_usage_description: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MacPermissionReport {
    pub applies: bool,
    pub issues: Vec<PermissionIssue>,
    /// The config lives in package.json / electron-builder.json and can be edited.
    pub fixable: bool,
    pub config_file: Option<String>,
    /// What to add by hand when the config cannot be edited automatically.
    pub manual: Option<String>,
    pub app_id: Option<String>,
    /// False when the config is JavaScript and could not be inspected.
    pub verified: bool,
}

enum ConfigSource {
    /// `"build"` key in package.json (also used when there is no config yet).
    PackageJson,
    Json(PathBuf),
    /// yml / json5 / js – readable at best, never rewritten.
    ReadOnly(PathBuf, Option<Value>),
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

fn load_config(root: &Path) -> Option<(ConfigSource, Value)> {
    let pkg = read_json(&root.join("package.json"))?;
    let deps_have = |k: &str| ["dependencies", "devDependencies"].iter().any(|d| pkg[*d].get(k).is_some());
    let uses_builder = deps_have("electron-builder")
        || pkg["scripts"].as_object().is_some_and(|s| s.values().any(|v| v.as_str().unwrap_or_default().contains("electron-builder")));
    if pkg.get("build").is_some_and(Value::is_object) {
        return Some((ConfigSource::PackageJson, pkg["build"].clone()));
    }
    if let Some(v) = read_json(&root.join("electron-builder.json")) {
        return Some((ConfigSource::Json(root.join("electron-builder.json")), v));
    }
    for f in ["electron-builder.yml", "electron-builder.yaml"] {
        let p = root.join(f);
        if let Ok(text) = std::fs::read_to_string(&p) {
            let v = serde_yaml::from_str::<serde_yaml::Value>(&text).ok().and_then(|y| serde_json::to_value(y).ok());
            return Some((ConfigSource::ReadOnly(p, v.clone()), v.unwrap_or(Value::Null)));
        }
    }
    for f in ["electron-builder.json5", "electron-builder.config.js", "electron-builder.config.cjs", "electron-builder.config.mjs", "electron-builder.config.ts", "electron-builder.toml"] {
        let p = root.join(f);
        if p.is_file() {
            return Some((ConfigSource::ReadOnly(p, None), Value::Null));
        }
    }
    uses_builder.then(|| (ConfigSource::PackageJson, Value::Object(Map::new())))
}

// ---------------------------------------------------------------------------
// Usage detection
// ---------------------------------------------------------------------------

static MIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:askForMediaAccess|getMediaAccessStatus)\(\s*['"`]microphone|webkitSpeechRecognition|\bSpeechRecognition\b"#).unwrap()
});
static CAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:askForMediaAccess|getMediaAccessStatus)\(\s*['"`]camera"#).unwrap());
static GUM_AUDIO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\baudio\s*:\s*(?:true|\{)").unwrap());
static GUM_VIDEO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bvideo\s*:\s*(?:true|\{)").unwrap());

const SKIP_DIRS: &[&str] = &["node_modules", "dist", "out", "build", "release", "coverage", "target", "vendor", "public"];
const SOURCE_EXTS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs", "vue", "svelte", "html"];

/// Returns (permission id, relative file) for devices the app's code uses.
fn scan_usage(root: &Path) -> Vec<(&'static str, String)> {
    fn walk(root: &Path, dir: &Path, depth: usize, budget: &mut usize, found: &mut Vec<(&'static str, String)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            if *budget == 0 || found.len() == PERMISSIONS.len() {
                return;
            }
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(ft) = e.file_type() else { continue };
            if name.starts_with('.') || ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth < 8 && !SKIP_DIRS.contains(&name.as_str()) {
                    walk(root, &p, depth + 1, budget, found);
                }
                continue;
            }
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !SOURCE_EXTS.contains(&ext.as_str()) || name.ends_with(".min.js") {
                continue;
            }
            *budget -= 1;
            if e.metadata().map(|m| m.len() > 1_000_000).unwrap_or(true) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let gum = text.contains("getUserMedia");
            let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            let mut hit = |id: &'static str| {
                if !found.iter().any(|(f, _)| *f == id) {
                    found.push((id, rel.clone()));
                }
            };
            if MIC.is_match(&text) || (gum && GUM_AUDIO.is_match(&text)) {
                hit("microphone");
            }
            if CAM.is_match(&text) || (gum && GUM_VIDEO.is_match(&text)) {
                hit("camera");
            }
        }
    }
    let mut found = vec![];
    let mut budget = 5000;
    walk(root, root, 0, &mut budget, &mut found);
    found
}

// ---------------------------------------------------------------------------
// Config inspection
// ---------------------------------------------------------------------------

fn build_resources(cfg: &Value) -> String {
    cfg["directories"]["buildResources"].as_str().unwrap_or("build").trim_end_matches('/').to_string()
}

/// The entitlements files electron-builder will actually use. `None` means
/// its built-in template (which lacks all device entitlements).
fn entitlement_files(root: &Path, cfg: &Value) -> Vec<Option<PathBuf>> {
    let res = build_resources(cfg);
    let pick = |key: &str, default: &str| -> Option<PathBuf> {
        match cfg["mac"][key].as_str() {
            Some(p) => Some(root.join(p)),
            None => Some(root.join(&res).join(default)).filter(|p| p.is_file()),
        }
    };
    // Without an inherit file electron-builder falls back to its template,
    // not to the main entitlements.
    vec![pick("entitlements", "entitlements.mac.plist"), pick("entitlementsInherit", "entitlements.mac.inherit.plist")]
}

fn plist_has_true(path: &Path, key: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    let Some(pos) = text.find(&format!("<key>{key}</key>")) else { return false };
    text[pos..].split("</key>").nth(1).is_some_and(|rest| rest.trim_start().starts_with("<true/>"))
}

fn check(root: &Path) -> MacPermissionReport {
    let mut report = MacPermissionReport::default();
    let Some((source, cfg)) = load_config(root) else { return report };
    report.applies = cfg!(target_os = "macos") || cfg.get("mac").is_some();
    if !report.applies {
        return report;
    }
    report.app_id = cfg["appId"].as_str().map(String::from);
    let hardened = cfg["mac"]["hardenedRuntime"].as_bool().unwrap_or(true);
    let files = entitlement_files(root, &cfg);
    let cfg_known = !matches!(source, ConfigSource::ReadOnly(_, None));
    report.verified = cfg_known;

    for (id, used_in) in scan_usage(root) {
        let perm = PERMISSIONS.iter().find(|p| p.id == id).unwrap();
        let missing_entitlement = hardened
            && cfg_known
            && files.iter().any(|f| f.as_ref().is_none_or(|p| !plist_has_true(p, perm.entitlement)));
        let missing_usage_description = cfg_known && cfg["mac"]["extendInfo"][perm.usage_key].as_str().is_none_or(str::is_empty);
        if missing_entitlement || missing_usage_description || !cfg_known {
            report.issues.push(PermissionIssue {
                id: id.into(),
                label: perm.label.into(),
                used_in,
                missing_entitlement: missing_entitlement || !cfg_known,
                missing_usage_description: missing_usage_description || !cfg_known,
            });
        }
    }

    match &source {
        ConfigSource::PackageJson => {
            report.fixable = true;
            report.config_file = Some("package.json".into());
        }
        ConfigSource::Json(p) => {
            report.fixable = true;
            report.config_file = Some(rel(root, p));
        }
        ConfigSource::ReadOnly(p, _) => {
            report.config_file = Some(rel(root, p));
            if !report.issues.is_empty() {
                report.manual = Some(manual_snippet(&report.issues, &build_resources(&cfg)));
            }
        }
    }
    report
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn manual_snippet(issues: &[PermissionIssue], res: &str) -> String {
    let mut s = format!(
        "mac:\n  hardenedRuntime: true\n  entitlements: {res}/entitlements.mac.plist\n  entitlementsInherit: {res}/entitlements.mac.plist\n  extendInfo:\n"
    );
    for i in issues {
        let p = PERMISSIONS.iter().find(|p| p.id == i.id).unwrap();
        s.push_str(&format!("    {}: \"{}\"\n", p.usage_key, p.usage_text));
    }
    s.push_str(&format!("\n# {res}/entitlements.mac.plist braucht:\n"));
    for i in issues {
        let p = PERMISSIONS.iter().find(|p| p.id == i.id).unwrap();
        s.push_str(&format!("#   <key>{}</key><true/>\n", p.entitlement));
    }
    s
}

// ---------------------------------------------------------------------------
// Fixing
// ---------------------------------------------------------------------------

fn new_plist(keys: &[&str]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n  <dict>\n",
    );
    for k in keys {
        s.push_str(&format!("    <key>{k}</key>\n    <true/>\n"));
    }
    s.push_str("  </dict>\n</plist>\n");
    s
}

/// Sets `key` to `<true/>` in an XML plist, keeping everything else as is.
fn plist_set_true(text: &str, key: &str) -> Result<String, String> {
    let tag = format!("<key>{key}</key>");
    if let Some(pos) = text.find(&tag) {
        let after = pos + tag.len();
        let rest = &text[after..];
        let trimmed = rest.trim_start();
        let ws = rest.len() - trimmed.len();
        if trimmed.starts_with("<true/>") {
            return Ok(text.to_string());
        }
        if let Some(stripped) = trimmed.strip_prefix("<false/>") {
            return Ok(format!("{}{}<true/>{}", &text[..after], &rest[..ws], stripped));
        }
        return Err(format!("{key} hat einen unerwarteten Wert"));
    }
    let end = text.rfind("</dict>").ok_or("Die Entitlements-Datei ist keine XML-Plist")?;
    let indent = text[..end].rsplit('\n').next().filter(|s| s.trim().is_empty()).unwrap_or("");
    let step = if text.contains("\n\t<key>") || text.contains("\n\t\t<key>") { "\t" } else { "  " };
    let inner = format!("{indent}{step}");
    Ok(format!(
        "{}{}{tag}\n{inner}<true/>\n{indent}</dict>{}",
        text[..end].trim_end_matches([' ', '\t']),
        inner,
        &text[end + "</dict>".len()..]
    ))
}

fn write_json(path: &Path, original: &str, value: &Value) -> Result<(), String> {
    let indent: &[u8] = if original.contains("\n\t") { b"\t" } else if original.contains("\n    \"") && !original.contains("\n  \"") { b"    " } else { b"  " };
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent);
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    value.serialize(&mut ser).map_err(|e| e.to_string())?;
    buf.push(b'\n');
    std::fs::write(path, buf).map_err(|e| format!("{}: {e}", path.display()))
}

fn obj<'a>(v: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    let map = v.as_object_mut().expect("object");
    if !map.get(key).is_some_and(Value::is_object) {
        map.insert(key.into(), Value::Object(Map::new()));
    }
    map.get_mut(key).and_then(Value::as_object_mut).expect("object")
}

fn fix(root: &Path, ids: &[String]) -> Result<Vec<String>, String> {
    let (source, _) = load_config(root).ok_or("Keine electron-builder-Konfiguration gefunden")?;
    let (file, key): (PathBuf, Option<&str>) = match source {
        ConfigSource::PackageJson => (root.join("package.json"), Some("build")),
        ConfigSource::Json(p) => (p, None),
        ConfigSource::ReadOnly(p, _) => {
            return Err(format!("{} kann nicht automatisch geändert werden – bitte von Hand ergänzen.", rel(root, &p)))
        }
    };
    let original = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let mut doc: Value = serde_json::from_str(&original).map_err(|e| e.to_string())?;
    let perms: Vec<&Permission> = PERMISSIONS.iter().filter(|p| ids.iter().any(|i| i == p.id)).collect();
    if perms.is_empty() {
        return Ok(vec![]);
    }
    let mut changes = vec![];

    let cfg: &mut Value = match key {
        Some(k) => {
            obj(&mut doc, k);
            doc.get_mut(k).unwrap()
        }
        None => &mut doc,
    };
    let res = build_resources(cfg);
    let current = cfg.clone();
    let mac = obj(cfg, "mac");

    // Entitlements: extend the files in use, or create one and point to it.
    let mut targets: Vec<PathBuf> = vec![];
    let mut main_path = String::new();
    for (k, default) in [("entitlements", "entitlements.mac.plist"), ("entitlementsInherit", "entitlements.mac.inherit.plist")] {
        let path = match current["mac"][k].as_str() {
            Some(p) => p.to_string(),
            None => {
                let d = format!("{res}/{default}");
                // Main and inherit share one file unless the project has its own.
                let p = if root.join(&d).is_file() || main_path.is_empty() { d } else { main_path.clone() };
                mac.insert(k.into(), Value::String(p.clone()));
                changes.push(format!("mac.{k} → {p}"));
                p
            }
        };
        if main_path.is_empty() {
            main_path = path.clone();
        }
        let full = root.join(&path);
        if !targets.contains(&full) {
            targets.push(full);
        }
    }
    for t in &targets {
        let keys: Vec<&str> = perms.iter().map(|p| p.entitlement).collect();
        let text = if t.is_file() {
            let mut text = std::fs::read_to_string(t).map_err(|e| e.to_string())?;
            for k in &keys {
                text = plist_set_true(&text, k).map_err(|e| format!("{}: {e}", rel(root, t)))?;
            }
            text
        } else {
            let mut all: Vec<&str> = DEFAULT_ENTITLEMENTS.to_vec();
            all.extend(keys.iter());
            if let Some(dir) = t.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            new_plist(&all)
        };
        std::fs::write(t, text).map_err(|e| format!("{}: {e}", t.display()))?;
        changes.push(format!("{}: {}", rel(root, t), keys.join(", ")));
    }

    let info = {
        if !mac.get("extendInfo").is_some_and(Value::is_object) {
            mac.insert("extendInfo".into(), Value::Object(Map::new()));
        }
        mac.get_mut("extendInfo").and_then(Value::as_object_mut).unwrap()
    };
    for p in &perms {
        if info.get(p.usage_key).and_then(Value::as_str).is_none_or(str::is_empty) {
            info.insert(p.usage_key.into(), Value::String(p.usage_text.into()));
            changes.push(format!("mac.extendInfo.{} gesetzt", p.usage_key));
        }
    }

    write_json(&file, &original, &doc)?;
    Ok(changes)
}

#[tauri::command]
pub fn check_mac_permissions(path: String) -> MacPermissionReport {
    check(Path::new(&path))
}

#[tauri::command]
pub fn fix_mac_permissions(path: String, permissions: Vec<String>) -> Result<Vec<String>, String> {
    fix(Path::new(&path), &permissions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str, pkg: &str, source: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ed-macperm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("package.json"), pkg).unwrap();
        std::fs::write(root.join("src/recorder.ts"), source).unwrap();
        root
    }

    const PKG: &str = "{\n  \"name\": \"demo\",\n  \"version\": \"1.0.0\",\n  \"scripts\": { \"dist:mac\": \"electron-builder --mac\" },\n  \"build\": { \"appId\": \"com.example.demo\", \"mac\": { \"target\": \"dmg\" } },\n  \"devDependencies\": { \"electron-builder\": \"^25\" }\n}\n";

    #[test]
    fn finds_and_fixes_missing_microphone_permission() {
        let root = project("fix", PKG, "navigator.mediaDevices.getUserMedia({ audio: true })");
        let report = check(&root);
        // PKG has a mac section, so the check applies on every host OS.
        assert!(report.applies);
        assert_eq!(report.issues.len(), 1);
        let issue = &report.issues[0];
        assert_eq!(issue.id, "microphone");
        assert!(issue.missing_entitlement && issue.missing_usage_description);
        assert_eq!(issue.used_in, "src/recorder.ts");
        assert!(report.fixable);

        let changes = fix(&root, &["microphone".into()]).unwrap();
        assert!(!changes.is_empty());
        assert!(check(&root).issues.is_empty(), "nach dem Fix keine Probleme mehr");

        let plist = std::fs::read_to_string(root.join("build/entitlements.mac.plist")).unwrap();
        assert!(plist.contains("com.apple.security.device.audio-input"));
        assert!(plist.contains("com.apple.security.cs.allow-jit"));
        let pkg = std::fs::read_to_string(root.join("package.json")).unwrap();
        // Key order of package.json is preserved.
        assert!(pkg.find("\"name\"").unwrap() < pkg.find("\"devDependencies\"").unwrap());
        assert!(pkg.contains("NSMicrophoneUsageDescription"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extends_existing_entitlements_file() {
        let pkg = PKG.replace("\"target\": \"dmg\"", "\"entitlements\": \"assets/ent.plist\", \"extendInfo\": { \"NSMicrophoneUsageDescription\": \"Für Sprachnotizen\" }");
        let root = project("existing", &pkg, "systemPreferences.askForMediaAccess('microphone')");
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(
            root.join("assets/ent.plist"),
            "<plist version=\"1.0\">\n<dict>\n\t<key>com.apple.security.cs.allow-jit</key>\n\t<true/>\n\t<key>com.apple.security.device.audio-input</key>\n\t<false/>\n</dict>\n</plist>\n",
        )
        .unwrap();
        let report = check(&root);
        assert!(report.issues[0].missing_entitlement);
        assert!(!report.issues[0].missing_usage_description);
        fix(&root, &["microphone".into()]).unwrap();
        let plist = std::fs::read_to_string(root.join("assets/ent.plist")).unwrap();
        assert!(plist.contains("<key>com.apple.security.device.audio-input</key>\n\t<true/>"), "{plist}");
        let pkg = std::fs::read_to_string(root.join("package.json")).unwrap();
        assert!(pkg.contains("Für Sprachnotizen"), "vorhandene Beschreibung bleibt");
        assert!(check(&root).issues.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ignores_apps_without_media_access() {
        let root = project("none", PKG, "console.log('hello')");
        assert!(check(&root).issues.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn inserts_missing_plist_key() {
        let text = "<plist>\n  <dict>\n    <key>a</key>\n    <true/>\n  </dict>\n</plist>\n";
        let out = plist_set_true(text, "b").unwrap();
        assert_eq!(out, "<plist>\n  <dict>\n    <key>a</key>\n    <true/>\n    <key>b</key>\n    <true/>\n  </dict>\n</plist>\n");
    }
}
