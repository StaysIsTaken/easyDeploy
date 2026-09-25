//! GUI apps (especially on macOS) start with a minimal PATH. We import the
//! user's login-shell PATH and add well-known SDK locations so that tools like
//! flutter, adb, cargo or docker are found no matter how the app was launched.
//!
//! The PATH is not only computed once at startup: [`refresh`] re-reads the
//! shell profile (Unix) or the registry (Windows) so tools the user installs
//! while the app is running are picked up too, and [`locate`] searches common
//! install locations (nvm, fnm, Homebrew kegs, …) when a tool is not on PATH.
//! Child processes get the current value via [`current_path`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant};

#[cfg(windows)]
const SEP: char = ';';
#[cfg(not(windows))]
const SEP: char = ':';

/// The PATH used for lookups and child processes. The process environment is
/// only written once at startup (before other threads exist); later updates
/// live here and are passed explicitly to every spawned command.
static PATH: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new(std::env::var("PATH").unwrap_or_default()));

/// Start time of the last refresh; also serialises concurrent refreshes.
static LAST_REFRESH: Mutex<Option<Instant>> = Mutex::new(None);

pub fn fix_environment() {
    if std::env::var_os("ANDROID_HOME").is_none() {
        if let Some(sdk) = android_sdk_candidates().into_iter().find(|p| p.is_dir()) {
            std::env::set_var("ANDROID_HOME", &sdk);
        }
    }
    let joined = compute_path();
    std::env::set_var("PATH", &joined);
    set_path(joined);
    *LAST_REFRESH.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

/// Re-reads the user's PATH so that tools installed while the app is running
/// (or by the user outside of it) are found. Calls arriving while another
/// refresh has just run are coalesced.
pub fn refresh() {
    let asked = Instant::now();
    let mut last = LAST_REFRESH.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_some_and(|t| t + Duration::from_secs(1) >= asked) {
        return;
    }
    *last = Some(Instant::now());
    let joined = compute_path();
    set_path(joined);
}

/// The PATH child processes should run with.
pub fn current_path() -> OsString {
    OsString::from(PATH.read().unwrap_or_else(|e| e.into_inner()).as_str())
}

fn set_path(p: String) {
    *PATH.write().unwrap_or_else(|e| e.into_inner()) = p;
}

/// Appends a directory to the PATH (used when [`locate`] found a tool).
fn add_dir(dir: &Path) {
    let s = dir.to_string_lossy().to_string();
    let mut path = PATH.write().unwrap_or_else(|e| e.into_inner());
    if !path.split(SEP).any(|x| x == s) {
        if !path.is_empty() {
            path.push(SEP);
        }
        path.push_str(&s);
    }
}

fn compute_path() -> String {
    let mut parts: Vec<String> = PATH
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .split(SEP)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    // Entries from the user's own configuration win over what we already had,
    // so the Node/Rust version the user selected in their shell is used.
    let mut preferred: Vec<String> = vec![];
    #[cfg(unix)]
    preferred.extend(login_shell_paths());
    #[cfg(windows)]
    preferred.extend(registry_paths());
    for p in preferred.iter().rev() {
        if p.is_empty() || !Path::new(p).is_dir() {
            continue;
        }
        parts.retain(|x| x != p);
        parts.insert(0, p.clone());
    }

    for extra in extra_dirs() {
        if extra.is_dir() {
            let s = extra.to_string_lossy().to_string();
            if !parts.iter().any(|x| x == &s) {
                parts.push(s);
            }
        }
    }
    parts.join(&SEP.to_string())
}

pub fn android_sdk_candidates() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut v = vec![];
    if let Some(p) = std::env::var_os("ANDROID_SDK_ROOT") {
        v.push(PathBuf::from(p));
    }
    #[cfg(target_os = "macos")]
    {
        v.push(home.join("Library/Android/sdk"));
        v.push(PathBuf::from("/opt/homebrew/share/android-commandlinetools"));
        v.push(PathBuf::from("/usr/local/share/android-commandlinetools"));
    }
    #[cfg(target_os = "linux")]
    {
        v.push(home.join("Android/Sdk"));
        v.push(PathBuf::from("/usr/lib/android-sdk"));
        v.push(PathBuf::from("/opt/android-sdk"));
    }
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        v.push(local.join("Android").join("Sdk"));
    }
    let _ = &home;
    v
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Well-known tool directories that are added to the PATH when they exist.
fn extra_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let cargo_home = env_dir("CARGO_HOME").unwrap_or_else(|| home.join(".cargo"));
    let mut v = vec![
        cargo_home.join("bin"),
        home.join(".pub-cache").join("bin"),
        home.join("fvm").join("default").join("bin"),
        home.join(".local").join("bin"),
        home.join(".bun").join("bin"),
        home.join(".volta").join("bin"),
        home.join(".deno").join("bin"),
        home.join("flutter").join("bin"),
        home.join("development").join("flutter").join("bin"),
    ];
    if let Some(java) = env_dir("JAVA_HOME") {
        v.push(java.join("bin"));
    }
    if let Some(sdk) = std::env::var_os("ANDROID_HOME") {
        let sdk = PathBuf::from(sdk);
        v.push(sdk.join("platform-tools"));
        v.push(sdk.join("cmdline-tools").join("latest").join("bin"));
        v.push(sdk.join("emulator"));
    }
    #[cfg(unix)]
    {
        v.extend(nvm_default_bin());
        v.push(home.join(".asdf").join("shims"));
        v.push(home.join(".local").join("share").join("mise").join("shims"));
        v.push(home.join(".pyenv").join("shims"));
        v.push(home.join(".npm-global").join("bin"));
        v.push(home.join("n").join("bin"));
        v.push(home.join(".yarn").join("bin"));
        for fnm in fnm_dirs() {
            v.push(fnm.join("aliases").join("default").join("bin"));
        }
        if let Some(pnpm) = env_dir("PNPM_HOME") {
            v.push(pnpm);
        }
    }
    #[cfg(target_os = "macos")]
    {
        v.push(home.join("Library").join("pnpm"));
        v.push(PathBuf::from("/opt/homebrew/bin"));
        v.push(PathBuf::from("/opt/homebrew/sbin"));
        v.push(PathBuf::from("/opt/homebrew/opt/rustup/bin"));
        v.push(PathBuf::from("/usr/local/bin"));
        v.push(PathBuf::from("/usr/local/opt/rustup/bin"));
        v.push(PathBuf::from("/Applications/Docker.app/Contents/Resources/bin"));
    }
    #[cfg(target_os = "linux")]
    {
        v.push(home.join(".local").join("share").join("pnpm"));
        v.push(PathBuf::from("/snap/bin"));
        v.push(PathBuf::from("/usr/local/bin"));
        v.push(PathBuf::from("/opt/flutter/bin"));
    }
    #[cfg(windows)]
    {
        if let Some(pf) = env_dir("ProgramFiles") {
            v.push(pf.join("Git").join("cmd"));
            v.push(pf.join("nodejs"));
            v.push(pf.join("GitHub CLI"));
            v.push(pf.join("Docker").join("Docker").join("resources").join("bin"));
        }
        if let Some(nvm) = env_dir("NVM_SYMLINK") {
            v.push(nvm);
        }
        v.push(PathBuf::from(r"C:\nvm4w\nodejs"));
        v.push(PathBuf::from(r"C:\src\flutter\bin"));
        if let Some(appdata) = env_dir("APPDATA") {
            v.push(appdata.join("npm"));
        }
        if let Some(local) = dirs::data_local_dir() {
            v.push(local.join("Microsoft").join("WinGet").join("Links"));
            v.push(local.join("Volta").join("bin"));
            v.push(local.join("pnpm"));
        }
        v.push(home.join("scoop").join("shims"));
        v.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin"));
        v.push(PathBuf::from(r"C:\Windows\System32\OpenSSH"));
    }
    v
}

#[cfg(unix)]
fn fnm_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut v = vec![];
    if let Some(d) = env_dir("FNM_DIR") {
        v.push(d);
    }
    if let Some(d) = dirs::data_dir() {
        v.push(d.join("fnm"));
    }
    v.push(home.join(".local").join("share").join("fnm"));
    v.push(home.join(".fnm"));
    v
}

#[cfg(unix)]
fn nvm_dir() -> PathBuf {
    env_dir("NVM_DIR").unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".nvm"))
}

/// The bin folder of nvm's default Node version. nvm only activates itself in
/// interactive shells, so without this Node is invisible to GUI apps.
#[cfg(unix)]
fn nvm_default_bin() -> Option<PathBuf> {
    let root = nvm_dir();
    let alias = std::fs::read_to_string(root.join("alias").join("default")).unwrap_or_default();
    let alias = alias.trim().trim_start_matches('v');
    let versions = subdirs(&root.join("versions").join("node"));
    let prefixed: Vec<_> = versions
        .iter()
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            let name = name.trim_start_matches('v');
            !alias.is_empty()
                && alias.chars().next().is_some_and(|c| c.is_ascii_digit())
                && (name == alias || name.starts_with(&format!("{alias}.")))
        })
        .cloned()
        .collect();
    let pool = if prefixed.is_empty() { versions } else { prefixed };
    newest(pool).map(|p| p.join("bin"))
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

fn version_key(p: &Path) -> Vec<u64> {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// Picks the folder whose name contains the highest version number.
#[cfg(any(unix, test))]
fn newest(dirs: Vec<PathBuf>) -> Option<PathBuf> {
    dirs.into_iter().max_by_key(|p| version_key(p))
}

/// Folders that may contain `program` although they are not on the PATH:
/// versions managed by nvm/fnm, keg-only Homebrew formulae, JDKs, …
fn search_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut v: Vec<PathBuf> = vec![];
    let mut newest_bins = |parent: PathBuf, suffix: &[&str]| {
        let mut all = subdirs(&parent);
        // Newest version first.
        all.sort_by_key(|p| std::cmp::Reverse(version_key(p)));
        for d in all {
            let mut p = d;
            for s in suffix {
                p = p.join(s);
            }
            v.push(p);
        }
    };
    #[cfg(unix)]
    {
        newest_bins(nvm_dir().join("versions").join("node"), &["bin"]);
        for fnm in fnm_dirs() {
            newest_bins(fnm.join("node-versions"), &["installation", "bin"]);
        }
        newest_bins(home.join(".rustup").join("toolchains"), &["bin"]);
    }
    #[cfg(target_os = "macos")]
    {
        newest_bins(PathBuf::from("/opt/homebrew/opt"), &["bin"]);
        newest_bins(PathBuf::from("/usr/local/opt"), &["bin"]);
        newest_bins(PathBuf::from("/Library/Java/JavaVirtualMachines"), &["Contents", "Home", "bin"]);
    }
    #[cfg(target_os = "linux")]
    {
        newest_bins(PathBuf::from("/usr/lib/jvm"), &["bin"]);
        newest_bins(PathBuf::from("/opt"), &["bin"]);
    }
    #[cfg(windows)]
    {
        if let Some(pf) = env_dir("ProgramFiles") {
            for vendor in ["Eclipse Adoptium", "Java", "Microsoft", "Zulu"] {
                newest_bins(pf.join(vendor), &["bin"]);
            }
        }
        if let Some(local) = dirs::data_local_dir() {
            newest_bins(local.join("Programs").join("Python"), &[]);
            newest_bins(local.join("nvm"), &[]);
        }
        newest_bins(home.join(".rustup").join("toolchains"), &["bin"]);
    }
    let _ = &home;
    v
}

/// Finds `program` even when it is not on the PATH and, if found, adds its
/// folder to the PATH so later commands (builds, installs) find it too.
pub fn locate(program: &str) -> Option<PathBuf> {
    if let Some(p) = which(program) {
        return Some(p);
    }
    for dir in search_dirs() {
        if let Some(p) = find_in(&dir, program) {
            add_dir(&dir);
            return Some(p);
        }
    }
    None
}

#[cfg(unix)]
fn login_shell_paths() -> Vec<String> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| Path::new(s).exists())
        .or_else(|| {
            ["/bin/zsh", "/bin/bash", "/bin/sh"].into_iter().find(|s| Path::new(s).exists()).map(String::from)
        });
    let Some(shell) = shell else { return vec![] };
    let mut out: Vec<String> = vec![];
    // A login shell reads ~/.profile / ~/.zprofile, an interactive one reads
    // ~/.bashrc / ~/.zshrc (where nvm, fnm & co. usually hook themselves in).
    // bash never reads ~/.bashrc as a login shell, so ask it twice.
    let mut flag_sets = vec!["-ilc"];
    if shell.ends_with("bash") {
        flag_sets.push("-ic");
    }
    // Run both probes in parallel so slow profiles don't add up.
    let probes: Vec<_> = flag_sets
        .into_iter()
        .map(|flags| {
            let shell = shell.clone();
            std::thread::spawn(move || shell_path(&shell, flags))
        })
        .collect();
    for probe in probes {
        if let Some(p) = probe.join().ok().flatten() {
            for part in p.split(SEP) {
                if !part.is_empty() && !out.iter().any(|x| x == part) {
                    out.push(part.to_string());
                }
            }
        }
    }
    out
}

#[cfg(unix)]
fn shell_path(shell: &str, flags: &str) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let mut child = Command::new(shell)
        .args([flags, "printf '__EDPATH__%s__EDPATH__' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });

    // Shell profiles with nvm or oh-my-zsh can take a few seconds.
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > Duration::from_secs(10) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(_) => return None,
        }
    }
    let out = reader.join().ok()?;
    let start = out.find("__EDPATH__")? + "__EDPATH__".len();
    let end = out[start..].find("__EDPATH__")? + start;
    Some(out[start..end].to_string())
}

/// The machine and user PATH as currently stored in the registry. Installers
/// (winget, rustup, Node) update it, but running processes keep the PATH they
/// were started with.
#[cfg(windows)]
fn registry_paths() -> Vec<String> {
    let keys = [
        r"HKCU\Environment",
        r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
    ];
    let mut out: Vec<String> = vec![];
    for key in keys {
        let Some(o) = crate::proc::run("reg", &["query", key, "/v", "Path"], Duration::from_secs(5)) else {
            continue;
        };
        for line in o.text.lines() {
            let line = line.trim();
            if !line.to_ascii_lowercase().starts_with("path ") {
                continue;
            }
            let Some(value) = line.split_once("REG_").and_then(|(_, r)| r.split_once(char::is_whitespace)) else {
                continue;
            };
            for part in value.1.trim().split(';') {
                let part = expand_vars(part.trim());
                if !part.is_empty() && !out.contains(&part) {
                    out.push(part);
                }
            }
        }
    }
    out
}

/// Expands `%VAR%` references like cmd.exe does.
#[cfg(any(windows, test))]
fn expand_vars(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn find_in(dir: &Path, program: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let exts: Vec<String> = std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect();
        for ext in &exts {
            let c = dir.join(format!("{program}{ext}"));
            if c.is_file() {
                return Some(c);
            }
        }
    }
    let candidate = dir.join(program);
    candidate.is_file().then_some(candidate)
}

/// Locate an executable on PATH (respecting PATHEXT on Windows).
pub fn which(program: &str) -> Option<PathBuf> {
    let path = current_path();
    std::env::split_paths(&path).find_map(|dir| find_in(&dir, program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_windows_vars() {
        std::env::set_var("ED_TEST_HOME", r"C:\Users\me");
        assert_eq!(expand_vars(r"%ED_TEST_HOME%\.cargo\bin"), r"C:\Users\me\.cargo\bin");
        assert_eq!(expand_vars(r"%ED_UNSET_VAR%\x"), r"%ED_UNSET_VAR%\x");
        assert_eq!(expand_vars("50% off"), "50% off");
    }

    #[test]
    fn picks_newest_version_folder() {
        let dirs = vec![PathBuf::from("v18.20.1"), PathBuf::from("v20.9.0"), PathBuf::from("v20.11.1")];
        assert_eq!(newest(dirs), Some(PathBuf::from("v20.11.1")));
    }

    #[cfg(unix)]
    #[test]
    fn locates_tools_outside_path() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("ed-nvm-test-{}", std::process::id()));
        for v in ["v18.20.1", "v20.11.1"] {
            let bin = root.join("versions").join("node").join(v).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let exe = bin.join("ed-fake-node");
            std::fs::write(&exe, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::env::set_var("NVM_DIR", &root);
        assert!(which("ed-fake-node").is_none());
        let found = locate("ed-fake-node").expect("found via nvm");
        assert!(found.to_string_lossy().contains("v20.11.1"), "{}", found.display());
        // The folder is now on the PATH used for child processes.
        assert_eq!(which("ed-fake-node"), Some(found));
        let _ = std::fs::remove_dir_all(&root);
    }
}
