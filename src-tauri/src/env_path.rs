//! GUI apps (especially on macOS) start with a minimal PATH. We import the
//! user's login-shell PATH and add well-known SDK locations so that tools like
//! flutter, adb, cargo or docker are found no matter how the app was launched.

use std::path::PathBuf;

#[cfg(windows)]
const SEP: char = ';';
#[cfg(not(windows))]
const SEP: char = ':';

pub fn fix_environment() {
    let mut parts: Vec<String> = std::env::var("PATH")
        .unwrap_or_default()
        .split(SEP)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    #[cfg(unix)]
    if let Some(shell_path) = login_shell_path() {
        for p in shell_path.split(SEP).rev() {
            if !p.is_empty() && !parts.iter().any(|x| x == p) {
                parts.insert(0, p.to_string());
            }
        }
    }

    if std::env::var_os("ANDROID_HOME").is_none() {
        if let Some(sdk) = android_sdk_candidates().into_iter().find(|p| p.is_dir()) {
            std::env::set_var("ANDROID_HOME", &sdk);
        }
    }

    for extra in extra_dirs() {
        if extra.is_dir() {
            let s = extra.to_string_lossy().to_string();
            if !parts.iter().any(|x| x == &s) {
                parts.push(s);
            }
        }
    }

    let joined = parts.join(&SEP.to_string());
    std::env::set_var("PATH", joined);
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

fn extra_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut v = vec![
        home.join(".cargo").join("bin"),
        home.join(".pub-cache").join("bin"),
        home.join("fvm").join("default").join("bin"),
        home.join(".local").join("bin"),
        home.join(".bun").join("bin"),
    ];
    if let Some(sdk) = std::env::var_os("ANDROID_HOME") {
        let sdk = PathBuf::from(sdk);
        v.push(sdk.join("platform-tools"));
        v.push(sdk.join("cmdline-tools").join("latest").join("bin"));
        v.push(sdk.join("emulator"));
    }
    #[cfg(target_os = "macos")]
    {
        v.push(PathBuf::from("/opt/homebrew/bin"));
        v.push(PathBuf::from("/opt/homebrew/sbin"));
        v.push(PathBuf::from("/opt/homebrew/opt/rustup/bin"));
        v.push(PathBuf::from("/usr/local/bin"));
        v.push(PathBuf::from("/Applications/Docker.app/Contents/Resources/bin"));
    }
    #[cfg(target_os = "linux")]
    {
        v.push(PathBuf::from("/snap/bin"));
        v.push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(windows)]
    {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            let pf = PathBuf::from(pf);
            v.push(pf.join("Git").join("cmd"));
            v.push(pf.join("nodejs"));
            v.push(pf.join("Docker").join("Docker").join("resources").join("bin"));
        }
        v.push(PathBuf::from(r"C:\Windows\System32\OpenSSH"));
    }
    v
}

#[cfg(unix)]
fn login_shell_path() -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    if !std::path::Path::new(&shell).exists() {
        return None;
    }
    let mut child = Command::new(&shell)
        .args(["-ilc", "printf '__EDPATH__%s__EDPATH__' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > Duration::from_secs(5) => {
                let _ = child.kill();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(_) => return None,
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    let start = out.find("__EDPATH__")? + "__EDPATH__".len();
    let end = out[start..].find("__EDPATH__")? + start;
    Some(out[start..end].to_string())
}

/// Locate an executable on PATH (respecting PATHEXT on Windows).
pub fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    #[cfg(windows)]
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
        .split(';')
        .map(|s| s.to_lowercase())
        .collect();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        #[cfg(windows)]
        {
            for ext in &exts {
                let c = dir.join(format!("{program}{ext}"));
                if c.is_file() {
                    return Some(c);
                }
            }
        }
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
