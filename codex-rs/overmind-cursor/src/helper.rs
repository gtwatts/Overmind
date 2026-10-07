//! Spawns and supervises the bundled Node helper (`../helper`).

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

const HELPER_DIR_ENV: &str = "OVERMIND_CURSOR_HELPER_DIR";
const NODE_ENV: &str = "OVERMIND_NODE";
const ENTRY: &str = "overmind-entry.mjs";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

struct RunningHelper {
    child: Child,
    port: u16,
}

/// One helper per Overmind process. Its stdin pipe is held here and never
/// written; the helper exits when the pipe closes, i.e. when Overmind exits.
static HELPER: Mutex<Option<RunningHelper>> = Mutex::new(None);

/// Returns the loopback port of a live helper, starting (and on first use,
/// building) it if needed.
pub(crate) fn ensure_running(codex_home: &Path) -> io::Result<u16> {
    let mut guard = HELPER
        .lock()
        .map_err(|_| io::Error::other("Cursor helper lock poisoned"))?;
    if let Some(running) = guard.as_mut()
        && matches!(running.child.try_wait(), Ok(None))
    {
        return Ok(running.port);
    }
    let dir = resolve_helper_dir(codex_home)?;
    let log_path = codex_home.join("log").join("overmind-cursor-helper.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    ensure_built(&dir, &log_path)?;
    let running = spawn(&dir, codex_home, &log_path)?;
    let port = running.port;
    *guard = Some(running);
    Ok(port)
}

fn resolve_helper_dir(codex_home: &Path) -> io::Result<PathBuf> {
    let candidates = [
        std::env::var_os(HELPER_DIR_ENV).map(PathBuf::from),
        Some(codex_home.join("overmind").join("cursor-helper")),
        Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("helper")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|dir| dir.join(ENTRY).is_file() && dir.join("package.json").is_file())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "Cursor helper not found; set {HELPER_DIR_ENV} or copy codex-rs/overmind-cursor/helper to {}",
                    codex_home.join("overmind").join("cursor-helper").display()
                ),
            )
        })
}

/// Installs dependencies and compiles the helper the first time it is used.
fn ensure_built(dir: &Path, log_path: &Path) -> io::Result<()> {
    if dir.join("dist").join("index.js").is_file() {
        return Ok(());
    }
    tracing::info!("building the Overmind Cursor helper in {}", dir.display());
    if !dir.join("node_modules").is_dir() {
        run_npm(dir, &["ci", "--no-audit", "--no-fund"], log_path)?;
    }
    run_npm(dir, &["run", "build:server"], log_path)
}

fn run_npm(dir: &Path, args: &[&str], log_path: &Path) -> io::Result<()> {
    let status = Command::new("npm")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(append_log(log_path)?)
        .stderr(append_log(log_path)?)
        .status()
        .map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("Cursor helper needs Node.js 22.19+ and npm on PATH ({err})"),
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "`npm {}` failed in {} ({status}); see {}",
            args.join(" "),
            dir.display(),
            log_path.display()
        )))
    }
}

fn spawn(dir: &Path, codex_home: &Path, log_path: &Path) -> io::Result<RunningHelper> {
    let state_dir = codex_home.join("overmind").join("cursor-helper-state");
    create_private_dir(&state_dir)?;
    let port = free_loopback_port()?;
    let node = std::env::var_os(NODE_ENV).unwrap_or_else(|| "node".into());
    let mut child = Command::new(node)
        .arg(ENTRY)
        .current_dir(dir)
        .envs(helper_env(port, &state_dir))
        .env_remove("CURSOR_API_KEY")
        .env_remove("GATEWAY_ACCESS_KEY")
        .stdin(Stdio::piped())
        .stdout(append_log(log_path)?)
        .stderr(append_log(log_path)?)
        .spawn()
        .map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("failed to start the Cursor helper (needs Node.js 22.19+): {err}"),
            )
        })?;
    let started = Instant::now();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "Cursor helper exited during startup ({status}); see {}",
                log_path.display()
            )));
        }
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            tracing::info!("Overmind Cursor helper listening on {addr}");
            return Ok(RunningHelper { child, port });
        }
        if started.elapsed() > STARTUP_TIMEOUT {
            let _ = child.kill();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "Cursor helper did not start within {}s; see {}",
                    STARTUP_TIMEOUT.as_secs(),
                    log_path.display()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Environment for the helper process. BYOK auth: the helper keeps no Cursor
/// credential; Overmind sends the key per request as the bearer token.
fn helper_env(port: u16, state_dir: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("HOST", Ipv4Addr::LOCALHOST.to_string()),
        ("PORT", port.to_string()),
        ("AUTH_MODE", "byok".to_string()),
        ("STATE_DIR", state_dir.display().to_string()),
        (
            "EMPTY_WORKSPACE_DIR",
            state_dir.join("empty-workspace").display().to_string(),
        ),
        // Codex sends the full transcript plus every tool schema each turn.
        ("MAX_BODY_BYTES", (64 * 1024 * 1024).to_string()),
        ("GLOBAL_ACTIVE_RUNS", "6".to_string()),
        ("PER_CREDENTIAL_ACTIVE_RUNS", "4".to_string()),
        ("HOSTED_SEARCH_MODE", "auto".to_string()),
        ("OVERMIND_CODEX_COMPAT", "1".to_string()),
        ("LOG_LEVEL", "info".to_string()),
    ]
}

fn free_loopback_port() -> io::Result<u16> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

fn append_log(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

#[cfg(test)]
#[path = "helper_tests.rs"]
mod tests;
