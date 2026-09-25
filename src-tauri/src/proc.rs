//! Small helpers to run non-interactive commands (version checks, device lists).

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::env_path::{current_path, which};

pub struct Output {
    pub ok: bool,
    pub text: String,
}

/// Builds a `Command` for `program`, resolving `.cmd`/`.bat` shims on Windows
/// and suppressing console windows.
pub fn command(program: &str) -> Command {
    let resolved = which(program)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| program.to_string());
    let mut cmd = Command::new(resolved);
    cmd.env("PATH", current_path());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Runs a command and returns combined stdout + stderr. `None` if it could not
/// be started (e.g. not installed) or timed out.
pub fn run(program: &str, args: &[&str], timeout: Duration) -> Option<Output> {
    let mut child = command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let mut stdout = child.stdout.take()?;
    let mut stderr = child.stderr.take()?;
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return None,
        }
    };
    let mut text = out_t.join().unwrap_or_default();
    let err = err_t.join().unwrap_or_default();
    if !err.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&err);
    }
    Some(Output {
        ok: status.success(),
        text,
    })
}
