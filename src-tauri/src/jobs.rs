//! Job runner: executes deploy pipelines step by step inside a real
//! pseudo-terminal, streams the output to the UI and detects interactive
//! prompts (passwords, host keys, licenses, y/n questions) so the UI can ask
//! the user instead of the process silently hanging.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, LazyLock, Mutex};
use std::time::Duration;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::env_path::which;
use crate::{github, secrets, share};

// ---------------------------------------------------------------------------
// Request types (sent by the frontend)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConn {
    pub host: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub key_path: Option<String>,
    /// Keychain key of a stored password; used to auto-answer the SSH password prompt.
    pub password_secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StepSpec {
    #[serde(rename_all = "camelCase")]
    Shell {
        name: String,
        command: String,
        cwd: Option<String>,
        env: Option<HashMap<String, String>>,
        allow_failure: Option<bool>,
    },
    #[serde(rename_all = "camelCase")]
    Upload {
        name: String,
        source: String,
        remote_path: String,
        ssh: SshConn,
        excludes: Option<Vec<String>>,
        clean: Option<bool>,
    },
    #[serde(rename_all = "camelCase")]
    Remote {
        name: String,
        command: String,
        ssh: SshConn,
    },
    #[serde(rename_all = "camelCase")]
    Copy {
        name: String,
        source: String,
        dest: String,
    },
    #[serde(rename_all = "camelCase")]
    Share {
        name: String,
        source: String,
        title: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Open { name: String, target: String },
    #[serde(rename_all = "camelCase")]
    GitClone { name: String, url: String, dest: String },
    #[serde(rename_all = "camelCase")]
    GhDispatch {
        name: String,
        owner: String,
        repo: String,
        workflow: String,
        git_ref: String,
        inputs: Option<HashMap<String, String>>,
    },
}

impl StepSpec {
    fn name(&self) -> &str {
        match self {
            StepSpec::Shell { name, .. }
            | StepSpec::Upload { name, .. }
            | StepSpec::Remote { name, .. }
            | StepSpec::Copy { name, .. }
            | StepSpec::Share { name, .. }
            | StepSpec::Open { name, .. }
            | StepSpec::GitClone { name, .. }
            | StepSpec::GhDispatch { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    pub job_id: String,
    pub title: String,
    pub base_dir: Option<String>,
    pub steps: Vec<StepSpec>,
    pub env: Option<HashMap<String, String>>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

// ---------------------------------------------------------------------------
// Events (sent to the frontend)
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputEvent<'a> {
    job_id: &'a str,
    data: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StepEvent<'a> {
    job_id: &'a str,
    index: usize,
    status: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptOption {
    label: String,
    value: String,
    primary: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptEvent<'a> {
    job_id: &'a str,
    prompt_id: u64,
    kind: &'a str,
    message: String,
    context: String,
    options: Vec<PromptOption>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoneEvent<'a> {
    job_id: &'a str,
    success: bool,
    cancelled: bool,
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// Job state
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct JobManager {
    jobs: Arc<Mutex<HashMap<String, Arc<JobHandle>>>>,
}

#[derive(Default)]
struct JobHandle {
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>,
    size: Mutex<(u16, u16)>,
    cancelled: AtomicBool,
}

static PROMPT_SEQ: AtomicU64 = AtomicU64::new(1);

#[tauri::command]
pub fn job_start(app: AppHandle, state: State<'_, JobManager>, req: JobRequest) -> Result<(), String> {
    let handle = Arc::new(JobHandle::default());
    *handle.size.lock().unwrap() = (req.cols.unwrap_or(120), req.rows.unwrap_or(30));
    {
        let mut jobs = state.jobs.lock().unwrap();
        if jobs.contains_key(&req.job_id) {
            return Err("Job läuft bereits".into());
        }
        jobs.insert(req.job_id.clone(), handle.clone());
    }
    let jobs = state.jobs.clone();
    std::thread::spawn(move || {
        let job_id = req.job_id.clone();
        let result = run_job(&app, &handle, req);
        let cancelled = handle.cancelled.load(Ordering::SeqCst);
        let (success, error) = match result {
            Ok(ok) => (ok && !cancelled, None),
            Err(e) => {
                emit_line(&app, &job_id, &format!("\x1b[31m✖ {e}\x1b[0m"));
                (false, Some(e))
            }
        };
        jobs.lock().unwrap().remove(&job_id);
        let _ = app.emit(
            "job://done",
            DoneEvent { job_id: &job_id, success, cancelled, error },
        );
    });
    Ok(())
}

#[tauri::command]
pub fn job_input(state: State<'_, JobManager>, job_id: String, data: String) -> Result<(), String> {
    let handle = state.jobs.lock().unwrap().get(&job_id).cloned();
    let Some(handle) = handle else { return Ok(()) };
    let mut w = handle.writer.lock().unwrap();
    if let Some(w) = w.as_mut() {
        w.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        let _ = w.flush();
    }
    Ok(())
}

#[tauri::command]
pub fn job_resize(state: State<'_, JobManager>, job_id: String, cols: u16, rows: u16) {
    let handle = state.jobs.lock().unwrap().get(&job_id).cloned();
    if let Some(handle) = handle {
        *handle.size.lock().unwrap() = (cols, rows);
        if let Some(m) = handle.master.lock().unwrap().as_ref() {
            let _ = m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }
}

#[tauri::command]
pub fn job_cancel(state: State<'_, JobManager>, job_id: String) {
    let handle = state.jobs.lock().unwrap().get(&job_id).cloned();
    if let Some(handle) = handle {
        handle.cancelled.store(true, Ordering::SeqCst);
        if let Some(k) = handle.killer.lock().unwrap().as_mut() {
            let _ = k.kill();
        }
    }
}

// ---------------------------------------------------------------------------
// Pipeline execution
// ---------------------------------------------------------------------------

fn emit_raw(app: &AppHandle, job_id: &str, data: String) {
    let _ = app.emit("job://output", OutputEvent { job_id, data });
}

fn emit_line(app: &AppHandle, job_id: &str, line: &str) {
    emit_raw(app, job_id, format!("{line}\r\n"));
}

fn emit_step(app: &AppHandle, job_id: &str, index: usize, status: &str) {
    let _ = app.emit("job://step", StepEvent { job_id, index, status });
}

fn resolve(base: &Option<PathBuf>, p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        return path;
    }
    match base {
        Some(b) => b.join(path),
        None => path,
    }
}

fn run_job(app: &AppHandle, handle: &Arc<JobHandle>, req: JobRequest) -> Result<bool, String> {
    let job_id = req.job_id.as_str();
    let base = req.base_dir.as_ref().map(PathBuf::from);
    let total = req.steps.len();
    emit_line(app, job_id, &format!("\x1b[1;35m◆ {}\x1b[0m", req.title));

    for (i, step) in req.steps.iter().enumerate() {
        if handle.cancelled.load(Ordering::SeqCst) {
            for j in i..total {
                emit_step(app, job_id, j, "skipped");
            }
            emit_line(app, job_id, "\x1b[33m■ Abgebrochen\x1b[0m");
            return Ok(false);
        }
        emit_step(app, job_id, i, "running");
        emit_line(
            app,
            job_id,
            &format!("\r\n\x1b[1;36m▶ Schritt {}/{}: {}\x1b[0m", i + 1, total, step.name()),
        );

        let result = run_step(app, handle, job_id, &base, &req.env, step);
        match result {
            Ok(true) => {
                emit_step(app, job_id, i, "success");
                emit_line(app, job_id, &format!("\x1b[32m✔ {}\x1b[0m", step.name()));
            }
            Ok(false) | Err(_) => {
                if let Err(e) = &result {
                    emit_line(app, job_id, &format!("\x1b[31m{e}\x1b[0m"));
                }
                let status = if handle.cancelled.load(Ordering::SeqCst) { "skipped" } else { "failed" };
                emit_step(app, job_id, i, status);
                for j in (i + 1)..total {
                    emit_step(app, job_id, j, "skipped");
                }
                emit_line(
                    app,
                    job_id,
                    &format!("\x1b[31m✖ Fehlgeschlagen: {}\x1b[0m", step.name()),
                );
                return Ok(false);
            }
        }
    }
    emit_line(app, job_id, "\r\n\x1b[1;32m✔ Fertig!\x1b[0m");
    Ok(true)
}

fn run_step(
    app: &AppHandle,
    handle: &Arc<JobHandle>,
    job_id: &str,
    base: &Option<PathBuf>,
    job_env: &Option<HashMap<String, String>>,
    step: &StepSpec,
) -> Result<bool, String> {
    match step {
        StepSpec::Shell { command, cwd, env, allow_failure, .. } => {
            let dir = match cwd {
                Some(c) if !c.is_empty() => resolve(base, c),
                _ => base.clone().unwrap_or_else(|| dirs::home_dir().unwrap_or_default()),
            };
            let mut all_env = job_env.clone().unwrap_or_default();
            if let Some(e) = env {
                all_env.extend(e.clone());
            }
            let cmd = shell_command(command, &dir, &all_env)?;
            let ok = run_pty(app, handle, job_id, cmd, None)?;
            if !ok && allow_failure.unwrap_or(false) && !handle.cancelled.load(Ordering::SeqCst) {
                emit_line(app, job_id, "\x1b[33m(Fehler ignoriert – Schritt ist optional)\x1b[0m");
                return Ok(true);
            }
            Ok(ok)
        }
        StepSpec::Upload { source, remote_path, ssh, excludes, clean, .. } => {
            let src = resolve(base, source);
            if !src.exists() {
                return Err(format!("Quelle nicht gefunden: {}", src.display()));
            }
            let (dir, item) = if src.is_dir() {
                (src.clone(), ".".to_string())
            } else {
                (
                    src.parent().map(Path::to_path_buf).unwrap_or_default(),
                    src.file_name().unwrap_or_default().to_string_lossy().to_string(),
                )
            };
            validate_ssh(ssh)?;
            let rp = remote_path_arg(remote_path)?;
            let mut tar = String::from("tar");
            if cfg!(target_os = "macos") {
                tar.push_str(" --no-xattrs --no-mac-metadata");
            }
            tar.push_str(&format!(" -C {}", quote_local(&dir.to_string_lossy())?));
            for ex in excludes.iter().flatten() {
                tar.push_str(&format!(" --exclude={}", quote_local(ex)?));
            }
            tar.push_str(&format!(" -cf - {}", quote_local(&item)?));

            let clean_cmd = if clean.unwrap_or(false) {
                check_clean_target(&rp)?;
                format!(" && find '{rp}' -mindepth 1 -delete")
            } else {
                String::new()
            };
            let remote = format!("mkdir -p '{rp}'{clean_cmd} && tar -C '{rp}' -xf -");
            let ssh_cmd = ssh_command_line(ssh, false)?;
            let line = format!("{tar} | {ssh_cmd} \"{remote}\"");
            emit_line(
                app,
                job_id,
                &format!("\x1b[2m$ Übertrage {} → {}:{}\x1b[0m", src.display(), ssh.host, remote_path),
            );
            let mut env = job_env.clone().unwrap_or_default();
            env.insert("COPYFILE_DISABLE".into(), "1".into());
            let cwd = base.clone().unwrap_or_else(|| dirs::home_dir().unwrap_or_default());
            let cmd = shell_command(&line, &cwd, &env)?;
            run_pty(app, handle, job_id, cmd, ssh.password_secret.clone())
        }
        StepSpec::Remote { command, ssh, .. } => {
            validate_ssh(ssh)?;
            let ssh_bin = which("ssh").ok_or("ssh wurde nicht gefunden")?;
            let mut cmd = CommandBuilder::new(ssh_bin);
            for a in ssh_args(ssh, true) {
                cmd.arg(a);
            }
            cmd.arg(ssh_dest(ssh));
            cmd.arg(command);
            if let Some(b) = base {
                cmd.cwd(b);
            }
            emit_line(app, job_id, &format!("\x1b[2m$ ssh {} {}\x1b[0m", ssh_dest(ssh), command));
            run_pty(app, handle, job_id, with_term(cmd), ssh.password_secret.clone())
        }
        StepSpec::Copy { source, dest, .. } => {
            let src = resolve(base, source);
            let dst_dir = PathBuf::from(dest);
            if !src.exists() {
                return Err(format!("Quelle nicht gefunden: {}", src.display()));
            }
            std::fs::create_dir_all(&dst_dir).map_err(|e| format!("Zielordner: {e}"))?;
            let target = dst_dir.join(src.file_name().unwrap_or_default());
            if let (Ok(s_abs), Ok(d_abs)) = (src.canonicalize(), dst_dir.canonicalize()) {
                if d_abs.starts_with(&s_abs) {
                    return Err("Der Zielordner liegt innerhalb der Quelle.".into());
                }
            }
            let mut count = 0usize;
            copy_recursive(&src, &target, &mut count).map_err(|e| e.to_string())?;
            emit_line(app, job_id, &format!("{count} Datei(en) kopiert nach {}", target.display()));
            Ok(true)
        }
        StepSpec::Share { source, title, .. } => {
            let src = resolve(base, source);
            let title = title.clone().unwrap_or_else(|| {
                src.file_name().unwrap_or_default().to_string_lossy().to_string()
            });
            let info = share::start_share(app, &src, &title)?;
            emit_line(app, job_id, "Download im Netzwerk bereit. Öffne auf dem anderen Gerät:");
            for u in &info.urls {
                emit_line(app, job_id, &format!("  \x1b[1;4m{}\x1b[0m", u.url));
            }
            Ok(true)
        }
        StepSpec::Open { target, .. } => {
            let t = if target.contains("://") { PathBuf::from(target) } else { resolve(base, target) };
            tauri_plugin_opener::open_path(t.to_string_lossy().to_string(), None::<&str>)
                .map_err(|e| e.to_string())?;
            Ok(true)
        }
        StepSpec::GitClone { url, dest, .. } => {
            let git = which("git").ok_or("git wurde nicht gefunden")?;
            let mut cmd = CommandBuilder::new(git);
            if !url.starts_with("https://") && !url.starts_with("git@") && !url.starts_with("ssh://") {
                return Err(format!("Nicht unterstützte Repository-URL: {url}"));
            }
            cmd.args(["clone", "--progress", "--", url.as_str(), dest.as_str()]);
            cmd.cwd(dirs::home_dir().unwrap_or_default());
            // Authenticate GitHub HTTPS clones with the stored token without
            // persisting it in .git/config.
            if url.starts_with("https://github.com/") {
                if let Some(tok) = secrets::get(github::TOKEN_KEY) {
                    use base64::Engine;
                    let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{tok}"));
                    cmd.env("GIT_CONFIG_COUNT", "1");
                    cmd.env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraheader");
                    cmd.env("GIT_CONFIG_VALUE_0", format!("AUTHORIZATION: basic {basic}"));
                }
            }
            emit_line(app, job_id, &format!("\x1b[2m$ git clone {url} {dest}\x1b[0m"));
            run_pty(app, handle, job_id, with_term(cmd), None)
        }
        StepSpec::GhDispatch { owner, repo, workflow, git_ref, inputs, .. } => {
            let inputs = inputs.clone().unwrap_or_default();
            tauri::async_runtime::block_on(github::dispatch(owner, repo, workflow, git_ref, &inputs))?;
            emit_line(
                app,
                job_id,
                &format!("Workflow „{workflow}“ auf {owner}/{repo}@{git_ref} gestartet."),
            );
            emit_line(app, job_id, &format!("https://github.com/{owner}/{repo}/actions"));
            Ok(true)
        }
    }
}

fn copy_recursive(src: &Path, dst: &Path, count: &mut usize) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        // Symlinks are skipped: following them could copy data from outside
        // the build folder or loop forever.
        return Ok(());
    }
    if meta.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()), count)?;
        }
    } else {
        std::fs::copy(src, dst)?;
        *count += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shell & SSH command construction
// ---------------------------------------------------------------------------

fn with_term(mut cmd: CommandBuilder) -> CommandBuilder {
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd
}

/// Runs `line` through the platform shell. On Windows the line is written to a
/// temporary batch file to avoid cmd.exe's quoting rules.
fn shell_command(line: &str, cwd: &Path, env: &HashMap<String, String>) -> Result<CommandBuilder, String> {
    #[cfg(windows)]
    let mut cmd = {
        let file = std::env::temp_dir().join(format!(
            "easydeploy-{}-{}.cmd",
            std::process::id(),
            PROMPT_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        // The batch file deletes itself when it finishes.
        let script = format!(
            "@echo off\r\nchcp 65001 >nul\r\n{}\r\nset ED_EXIT=%errorlevel%\r\n(goto) 2>nul & del \"%~f0\" & exit /b %ED_EXIT%\r\n",
            line
        );
        std::fs::write(&file, script).map_err(|e| e.to_string())?;
        let comspec = std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
        let mut c = CommandBuilder::new(comspec);
        c.args(["/d", "/c"]);
        c.arg(file.to_string_lossy().to_string());
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let shell = if Path::new("/bin/bash").exists() { "/bin/bash" } else { "/bin/sh" };
        let mut c = CommandBuilder::new(shell);
        c.args(["-c", line]);
        c
    };
    if !cwd.is_dir() {
        return Err(format!("Arbeitsordner existiert nicht: {}", cwd.display()));
    }
    cmd.cwd(cwd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    Ok(with_term(cmd))
}

/// Quotes a value for the local shell. On Unix single quotes disable every
/// expansion (`$()`, backticks, globs); cmd.exe has no such quoting, so
/// characters it would still interpret are rejected instead.
#[cfg(not(windows))]
fn quote_local(s: &str) -> Result<String, String> {
    Ok(format!("'{}'", s.replace('\'', "'\\''")))
}

#[cfg(windows)]
fn quote_local(s: &str) -> Result<String, String> {
    if s.chars().any(|c| matches!(c, '"' | '%' | '!' | '^' | '\n' | '\r')) {
        return Err(format!("Ungültiges Zeichen im Pfad bzw. Argument: {s}"));
    }
    Ok(format!("\"{s}\""))
}

/// Remote paths are embedded in a shell command on the server, so only a
/// conservative character set is allowed.
fn remote_path_arg(p: &str) -> Result<String, String> {
    let p = p.trim();
    if !p.chars().all(|c| c.is_ascii_alphanumeric() || "._-/~+@ ".contains(c)) {
        return Err(format!(
            "Remote-Pfad „{p}“ enthält unzulässige Zeichen (erlaubt: Buchstaben, Ziffern, . _ - / ~ + @ Leerzeichen)"
        ));
    }
    if p.split('/').any(|seg| seg == "..") {
        return Err("Remote-Pfad darf kein „..“ enthalten".into());
    }
    Ok(match p {
        "" | "~" | "~/" => ".".to_string(),
        _ if p.starts_with("~/") => p[2..].trim_end_matches('/').to_string(),
        _ => p.trim_end_matches('/').to_string(),
    })
}

/// Refuses to wipe directories that are obviously not a deploy folder.
fn check_clean_target(rp: &str) -> Result<(), String> {
    let segments: Vec<&str> = rp.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    let too_shallow = if rp.starts_with('/') { segments.len() < 2 } else { segments.is_empty() };
    if too_shallow {
        return Err(format!(
            "„Ziel vorher leeren“ ist für „{rp}“ zu gefährlich – bitte einen eigenen Unterordner als Remote-Pfad verwenden."
        ));
    }
    Ok(())
}

/// Host and user end up as ssh arguments; a leading '-' would be parsed as an
/// option (e.g. -oProxyCommand=…), so both are validated strictly.
fn validate_ssh(ssh: &SshConn) -> Result<(), String> {
    let host_ok = !ssh.host.is_empty()
        && !ssh.host.starts_with('-')
        && ssh.host.chars().all(|c| c.is_ascii_alphanumeric() || ".-_:[]%".contains(c));
    if !host_ok {
        return Err(format!("Ungültiger Host: „{}“", ssh.host));
    }
    if let Some(u) = &ssh.user {
        let user_ok = u.is_empty() || (!u.starts_with('-') && u.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)));
        if !user_ok {
            return Err(format!("Ungültiger Benutzername: „{u}“"));
        }
    }
    Ok(())
}

fn ssh_dest(ssh: &SshConn) -> String {
    match &ssh.user {
        Some(u) if !u.is_empty() => format!("{u}@{}", ssh.host),
        _ => ssh.host.clone(),
    }
}

fn ssh_args(ssh: &SshConn, tty: bool) -> Vec<String> {
    let mut a = vec![];
    if tty {
        a.push("-t".into());
    }
    if let Some(p) = ssh.port {
        if p != 22 {
            a.push("-p".into());
            a.push(p.to_string());
        }
    }
    if let Some(k) = &ssh.key_path {
        if !k.is_empty() {
            a.push("-i".into());
            a.push(expand_home(k));
        }
    }
    a.push("-o".into());
    a.push("ConnectTimeout=15".into());
    a.push("-o".into());
    a.push("ServerAliveInterval=15".into());
    a
}

fn ssh_command_line(ssh: &SshConn, tty: bool) -> Result<String, String> {
    let mut parts = vec!["ssh".to_string()];
    for a in ssh_args(ssh, tty) {
        parts.push(quote_local(&a)?);
    }
    parts.push(quote_local(&ssh_dest(ssh))?);
    Ok(parts.join(" "))
}

fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest).to_string_lossy().to_string();
        }
    }
    p.to_string()
}

// ---------------------------------------------------------------------------
// PTY execution + prompt detection
// ---------------------------------------------------------------------------

static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]").unwrap()
});
static SSH_PW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(^|\s|\()\S+@\S+('s)?\)? ?password:\s*$").unwrap());

struct PromptRule {
    kind: &'static str,
    re: Regex,
    options: &'static [(&'static str, &'static str, bool)],
}

static RULES: LazyLock<Vec<PromptRule>> = LazyLock::new(|| {
    vec![
        PromptRule {
            kind: "hostkey",
            re: Regex::new(r"(?i)continue connecting \(yes/no(/\[fingerprint\])?\)\?\s*$").unwrap(),
            options: &[("Vertrauen & verbinden", "yes\r", true), ("Abbrechen", "no\r", false)],
        },
        PromptRule {
            kind: "license",
            re: Regex::new(r"(?i)accept\?\s*\(y/n\):?\s*$").unwrap(),
            options: &[("Akzeptieren", "y\r", true), ("Ablehnen", "n\r", false)],
        },
        PromptRule {
            kind: "license",
            re: Regex::new(r"(?i)(agree to (all )?the (source )?agreements?|by typing 'agree').*$").unwrap(),
            options: &[("Zustimmen", "Y\r", true), ("Ablehnen", "N\r", false)],
        },
        PromptRule {
            kind: "secret",
            re: Regex::new(r"(?i)(password|passwort|passphrase|kennwort|\bpin\b)[^\n]{0,80}:\s*$").unwrap(),
            options: &[],
        },
        PromptRule {
            kind: "confirm",
            re: Regex::new(r"(?i)\[Y\] Yes\s+\[N\] No.*:\s*$").unwrap(),
            options: &[("Ja", "Y\r", true), ("Nein", "N\r", false)],
        },
        PromptRule {
            kind: "confirm",
            re: Regex::new(r"(?i)\((yes/no)\)\s*[?:]?\s*$|\[(yes/no)\]\s*[?:]?\s*$").unwrap(),
            options: &[("Ja", "yes\r", true), ("Nein", "no\r", false)],
        },
        PromptRule {
            kind: "confirm",
            re: Regex::new(r"(?i)[\[(]y/n[\])]\s*[?:]?\s*$").unwrap(),
            options: &[("Ja", "y\r", true), ("Nein", "n\r", false)],
        },
    ]
});

struct Detector {
    text: String,
    newlines: u64,
    last_prompt_line: Option<u64>,
    auto_secret: Option<String>,
    auto_used: bool,
}

impl Detector {
    fn feed(&mut self, chunk: &str) {
        let clean = ANSI_RE.replace_all(chunk, "");
        self.newlines += clean.matches('\n').count() as u64;
        self.text.push_str(&clean);
        if self.text.len() > 16_000 {
            let cut = self.text.len() - 12_000;
            let cut = (cut..self.text.len()).find(|i| self.text.is_char_boundary(*i)).unwrap_or(0);
            self.text.drain(..cut);
        }
    }

    fn current_line(&self) -> &str {
        let line = self.text.rsplit('\n').next().unwrap_or("");
        let trimmed = line.trim_end_matches('\r');
        match trimmed.rfind('\r') {
            Some(i) => &trimmed[i + 1..],
            None => trimmed,
        }
    }

    fn context(&self) -> String {
        let lines: Vec<&str> = self.text.lines().collect();
        let start = lines.len().saturating_sub(60);
        lines[start..]
            .iter()
            .map(|l| l.rsplit('\r').next().unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn mentions_agreement(context: &str) -> bool {
    let tail: Vec<&str> = context.lines().rev().take(8).collect();
    let tail = tail.join(" ").to_lowercase();
    ["agreement", "license", "licence", "lizenz", "terms"].iter().any(|k| tail.contains(k))
}

enum Detected {
    Prompt(&'static PromptRule, &'static str, String, String),
    AutoSecret(String),
}

fn detect(d: &mut Detector) -> Option<Detected> {
    let line = d.current_line().to_string();
    if line.trim().is_empty() || d.last_prompt_line == Some(d.newlines) {
        return None;
    }
    for rule in RULES.iter() {
        if rule.re.is_match(&line) {
            d.last_prompt_line = Some(d.newlines);
            if rule.kind == "secret" && !d.auto_used && SSH_PW_RE.is_match(&line) {
                if let Some(secret) = d.auto_secret.as_ref().and_then(|k| secrets::get(k)) {
                    d.auto_used = true;
                    return Some(Detected::AutoSecret(secret));
                }
            }
            let context = d.context();
            let kind = if rule.kind == "confirm" && mentions_agreement(&context) { "license" } else { rule.kind };
            return Some(Detected::Prompt(rule, kind, line.trim().to_string(), context));
        }
    }
    None
}

fn run_pty(
    app: &AppHandle,
    handle: &Arc<JobHandle>,
    job_id: &str,
    cmd: CommandBuilder,
    auto_secret: Option<String>,
) -> Result<bool, String> {
    let (cols, rows) = *handle.size.lock().unwrap();
    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("Terminal konnte nicht geöffnet werden: {e}"))?;

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Befehl konnte nicht gestartet werden: {e}"))?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    *handle.writer.lock().unwrap() = Some(writer);
    *handle.master.lock().unwrap() = Some(pair.master);
    *handle.killer.lock().unwrap() = Some(child.clone_killer());

    let (done_tx, done_rx) = mpsc::channel::<()>();
    let app2 = app.clone();
    let job2 = job_id.to_string();
    let handle2 = handle.clone();
    let reader_thread = std::thread::spawn(move || {
        let mut det = Detector {
            text: String::new(),
            newlines: 0,
            last_prompt_line: None,
            auto_secret,
            auto_used: false,
        };
        let mut buf = [0u8; 8192];
        let mut carry: Vec<u8> = Vec::new();
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            carry.extend_from_slice(&buf[..n]);
            let valid = match std::str::from_utf8(&carry) {
                Ok(_) => carry.len(),
                Err(e) if e.error_len().is_none() => e.valid_up_to(),
                Err(_) => carry.len(),
            };
            let text = String::from_utf8_lossy(&carry[..valid]).to_string();
            carry.drain(..valid);
            if text.is_empty() {
                continue;
            }
            det.feed(&text);
            emit_raw(&app2, &job2, text);

            match detect(&mut det) {
                Some(Detected::AutoSecret(secret)) => {
                    if let Some(w) = handle2.writer.lock().unwrap().as_mut() {
                        let _ = w.write_all(format!("{secret}\r").as_bytes());
                        let _ = w.flush();
                    }
                    emit_raw(&app2, &job2, "\x1b[2m[easyDeploy: gespeichertes Passwort eingesetzt]\x1b[0m".into());
                }
                Some(Detected::Prompt(rule, kind, message, context)) => {
                    let options = rule
                        .options
                        .iter()
                        .map(|(l, v, p)| PromptOption { label: l.to_string(), value: v.to_string(), primary: *p })
                        .collect();
                    let _ = app2.emit(
                        "job://prompt",
                        PromptEvent {
                            job_id: &job2,
                            prompt_id: PROMPT_SEQ.fetch_add(1, Ordering::SeqCst),
                            kind,
                            message,
                            context,
                            options,
                        },
                    );
                }
                None => {}
            }
        }
        let _ = done_tx.send(());
    });

    let status = child.wait().map_err(|e| e.to_string())?;
    // Give the reader a moment to drain remaining output, then close the PTY
    // (on Windows the reader only unblocks once the master is dropped).
    let drained = done_rx.recv_timeout(Duration::from_millis(1500)).is_ok();
    *handle.killer.lock().unwrap() = None;
    *handle.writer.lock().unwrap() = None;
    *handle.master.lock().unwrap() = None;
    if !drained {
        let _ = done_rx.recv_timeout(Duration::from_millis(1500));
    }
    drop(reader_thread);

    if !status.success() {
        emit_line(app, job_id, &format!("\x1b[2m(Exit-Code {})\x1b[0m", status.exit_code()));
    }
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_for(context: &str, auto_secret: Option<&str>) -> Option<String> {
        let mut d = Detector {
            text: String::new(),
            newlines: 0,
            last_prompt_line: None,
            auto_secret: auto_secret.map(String::from),
            auto_used: false,
        };
        d.feed(context);
        match detect(&mut d) {
            Some(Detected::Prompt(_, kind, _, _)) => Some(kind.to_string()),
            Some(Detected::AutoSecret(_)) => Some("auto".into()),
            None => None,
        }
    }

    #[test]
    fn detects_prompts() {
        assert_eq!(
            kind_for("The authenticity of host 'x' can't be established.\r\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ", None).as_deref(),
            Some("hostkey")
        );
        assert_eq!(kind_for("deploy@10.0.0.2's password: ", None).as_deref(), Some("secret"));
        assert_eq!(kind_for("[sudo] password for jan: ", None).as_deref(), Some("secret"));
        assert_eq!(kind_for("Enter passphrase for key '/home/a/.ssh/id_ed25519': ", None).as_deref(), Some("secret"));
        assert_eq!(kind_for("License android-sdk-license:\n---\nTerms...\nAccept? (y/N): ", None).as_deref(), Some("license"));
        assert_eq!(kind_for("Need to get 5 MB.\nDo you want to continue? [Y/n] ", None).as_deref(), Some("confirm"));
        assert_eq!(
            kind_for("The `msstore` source requires that you view the following agreements before using.\nDo you agree to all the source agreements terms?\n[Y] Yes  [N] No: ", None).as_deref(),
            Some("license")
        );
        assert_eq!(kind_for(":: Proceed with installation? [Y/n] ", None).as_deref(), Some("confirm"));
    }

    #[test]
    fn ignores_normal_output() {
        assert_eq!(kind_for("   Compiling serde v1.0.0\n", None), None);
        assert_eq!(kind_for("Running Gradle task 'assembleRelease'...", None), None);
        assert_eq!(kind_for("Mapping: ", None), None);
        assert_eq!(kind_for("password updated successfully\n", None), None);
    }

    #[test]
    fn same_prompt_is_reported_once() {
        let mut d = Detector {
            text: String::new(),
            newlines: 0,
            last_prompt_line: None,
            auto_secret: None,
            auto_used: false,
        };
        d.feed("Proceed? [y/N] ");
        assert!(detect(&mut d).is_some());
        d.feed("y");
        assert!(detect(&mut d).is_none());
        d.feed("\r\nNext question? [y/N] ");
        assert!(detect(&mut d).is_some());
    }

    #[test]
    fn remote_paths() {
        assert_eq!(remote_path_arg("~/apps/x").unwrap(), "apps/x");
        assert_eq!(remote_path_arg("~").unwrap(), ".");
        assert_eq!(remote_path_arg("/var/www").unwrap(), "/var/www");
        assert!(remote_path_arg("/var/'x").is_err());
        assert!(remote_path_arg("/srv/$(reboot)").is_err());
        assert!(remote_path_arg("/srv/`id`").is_err());
        assert!(remote_path_arg("/srv/../etc").is_err());
    }

    #[test]
    fn clean_guard() {
        assert!(check_clean_target(".").is_err());
        assert!(check_clean_target("/").is_err());
        assert!(check_clean_target("/var").is_err());
        assert!(check_clean_target("/var/www/app").is_ok());
        assert!(check_clean_target("apps/x").is_ok());
    }

    #[test]
    fn ssh_validation() {
        let mk = |host: &str, user: &str| SshConn {
            host: host.into(),
            port: None,
            user: Some(user.into()),
            key_path: None,
            password_secret: None,
        };
        assert!(validate_ssh(&mk("192.168.1.2", "deploy")).is_ok());
        assert!(validate_ssh(&mk("server.example.de", "")).is_ok());
        assert!(validate_ssh(&mk("-oProxyCommand=touch /tmp/x", "a")).is_err());
        assert!(validate_ssh(&mk("host", "-oProxyCommand=x")).is_err());
        assert!(validate_ssh(&mk("host;rm", "a")).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn quoting_blocks_expansion() {
        assert_eq!(quote_local("a b").unwrap(), "'a b'");
        assert_eq!(quote_local("$(x)").unwrap(), "'$(x)'");
        assert_eq!(quote_local("it's").unwrap(), "'it'\\''s'");
    }
}
