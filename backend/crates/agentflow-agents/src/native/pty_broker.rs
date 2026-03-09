//! PTY Broker — spawns a CLI program inside a pseudo-terminal.
//!
//! Think of it as hiring an invisible operator: they sit in front of a virtual
//! display running the CLI, relay every byte of screen output to subscribers,
//! and forward any keystrokes/input you send back into the terminal.
//!
//! Architecture:
//!
//!   Caller (async)          Sync threads          PTY
//!   ──────────────          ────────────          ───
//!   input_tx  ──tokio──▶  writer thread ──▶  master write
//!   resize_tx ──tokio──▶  resize thread ──▶  master resize
//!                          reader thread ◀──  master read  ──▶  output_tx (broadcast)

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, PtySize};
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{info, warn};

pub const DEFAULT_COLS: u16 = 220;
pub const DEFAULT_ROWS: u16 = 50;

/// Live handle to a PTY session. Cheap to clone (Arc internals).
#[derive(Clone)]
pub struct PtyHandle {
    pub session_id: String,
    pub agent_id: String,
    /// Send raw bytes into the PTY (keystrokes, paste, etc.)
    pub input_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Request a terminal resize
    pub resize_tx: mpsc::UnboundedSender<(u16, u16)>,
    /// Subscribe to raw PTY output (ANSI bytes, control codes, etc.)
    pub output_tx: Arc<broadcast::Sender<Vec<u8>>>,
    /// Terminate the child process that owns this PTY.
    child_killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    /// Broadcasts whether the PTY process has exited.
    exit_tx: watch::Sender<bool>,
    exit_rx: watch::Receiver<bool>,
}

impl PtyHandle {
    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        self.input_tx
            .send(data.to_vec())
            .map_err(|_| anyhow!("PTY input channel closed"))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.resize_tx
            .send((cols, rows))
            .map_err(|_| anyhow!("PTY resize channel closed"))
    }

    pub fn subscribe_output(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    pub fn subscribe_exit(&self) -> watch::Receiver<bool> {
        self.exit_rx.clone()
    }

    pub fn shutdown(&self) -> Result<()> {
        if *self.exit_rx.borrow() {
            return Ok(());
        }

        let mut killer = self
            .child_killer
            .lock()
            .map_err(|_| anyhow!("PTY child killer lock poisoned"))?;

        match killer.kill() {
            Ok(()) => {
                let _ = self.exit_tx.send(true);
                Ok(())
            }
            Err(err) => {
                // On Windows the PTY child can still exit successfully even if the
                // immediate kill call reports a transient OS error. Give the wait
                // thread a short grace period to observe process exit before failing.
                for _ in 0..10 {
                    if *self.exit_rx.borrow() {
                        return Ok(());
                    }
                    thread::sleep(Duration::from_millis(50));
                }

                if *self.exit_rx.borrow() {
                    Ok(())
                } else {
                    Err(err.into())
                }
            }
        }
    }
}

/// Spawn `program args…` inside a PTY and return a live handle.
///
/// The program inherits the server's environment except for variables that
/// would make Claude Code refuse to start inside a parent Claude session.
pub fn spawn(
    session_id: String,
    agent_id: String,
    program: &str,
    args: &[&str],
    cols: u16,
    rows: u16,
) -> Result<PtyHandle> {
    let pty_system = native_pty_system();
    let size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system
        .openpty(size)
        .map_err(|e| anyhow!("openpty failed: {}", e))?;

    // On Windows wrap with cmd.exe so .cmd/.bat scripts are resolved correctly.
    #[cfg(target_os = "windows")]
    let (real_program, real_args) = {
        let mut all_args = vec!["/c", program];
        all_args.extend_from_slice(args);
        ("cmd.exe", all_args)
    };
    #[cfg(not(target_os = "windows"))]
    let (real_program, real_args) = (program, args.to_vec());

    let mut cmd = CommandBuilder::new(real_program);
    cmd.args(&real_args);

    // On Windows portable-pty falls back to USERPROFILE when no cwd is set,
    // which launches native CLIs outside the intended workspace.
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }

    // Proper terminal type — required on macOS/Linux for ANSI rendering.
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    // Forward PATH so the CLI can find its own helpers on all platforms.
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    // On Windows, PATHEXT lists executable extensions (.CMD, .EXE, etc.).
    // Without it, CreateProcess won't resolve `claude` → `claude.cmd`.
    if let Ok(pathext) = std::env::var("PATHEXT") {
        cmd.env("PATHEXT", pathext);
    }

    // Let the CLI think it is running in a fresh terminal, not nested.
    cmd.env_remove("CLAUDECODE");
    cmd.env_remove("CLAUDE_CODE_ENTRYPOINT");
    cmd.env_remove("CLAUDE_CODE_SESSION");

    // Slave side: the CLI's stdin/stdout/stderr all come from the PTY.
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| anyhow!("Failed to spawn '{}' in PTY: {}", program, e))?;
    let child_killer = Arc::new(Mutex::new(child.clone_killer()));

    // Take I/O handles from the master BEFORE dropping the slave reference.
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| anyhow!("PTY clone reader: {}", e))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| anyhow!("PTY take writer: {}", e))?;
    // Keep master alive for resize; Box it into a Send wrapper.
    let master = pair.master;

    // ── broadcast channel: PTY output → all WS subscribers ──────────────
    let (output_tx, _) = broadcast::channel::<Vec<u8>>(512);
    let output_tx = Arc::new(output_tx);

    // ── std channels bridge: tokio tasks → blocking threads ──────────────
    let (input_std_tx, input_std_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (resize_std_tx, resize_std_rx) = std::sync::mpsc::channel::<(u16, u16)>();

    // ── tokio channels: async callers → bridge tasks ─────────────────────
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();
    let (exit_tx, exit_rx) = watch::channel(false);

    // Bridge: tokio input → std channel
    tokio::spawn(async move {
        while let Some(data) = input_rx.recv().await {
            if input_std_tx.send(data).is_err() {
                break;
            }
        }
    });

    // Bridge: tokio resize → std channel
    tokio::spawn(async move {
        while let Some(r) = resize_rx.recv().await {
            if resize_std_tx.send(r).is_err() {
                break;
            }
        }
    });

    // Blocking thread: std channel → PTY write
    thread::spawn(move || {
        while let Ok(data) = input_std_rx.recv() {
            if writer.write_all(&data).is_err() {
                break;
            }
        }
    });

    // Blocking thread: resize events → PTY master resize
    thread::spawn(move || {
        while let Ok((cols, rows)) = resize_std_rx.recv() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    });

    let wait_exit_tx = exit_tx.clone();
    let wait_sid = session_id.clone();
    thread::spawn(move || {
        match child.wait() {
            Ok(status) => info!("PTY session {} child exited: {}", wait_sid, status),
            Err(err) => warn!("PTY session {} child wait failed: {}", wait_sid, err),
        }
        let _ = wait_exit_tx.send(true);
    });

    // Blocking thread: PTY read → broadcast
    let out_tx = output_tx.clone();
    let read_exit_tx = exit_tx.clone();
    let sid = session_id.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    info!("PTY session {} EOF", sid);
                    let _ = read_exit_tx.send(true);
                    break;
                }
                Err(err) => {
                    warn!("PTY session {} read failed: {}", sid, err);
                    let _ = read_exit_tx.send(true);
                    break;
                }
                Ok(n) => {
                    // Ignore send errors (no subscribers is fine)
                    let _ = out_tx.send(buf[..n].to_vec());
                }
            }
        }
    });

    let effective_cwd = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    info!(
        "PTY session {} spawned: {} {:?} cwd={}",
        session_id, program, args, effective_cwd
    );

    Ok(PtyHandle {
        session_id,
        agent_id,
        input_tx,
        resize_tx,
        output_tx,
        child_killer,
        exit_tx,
        exit_rx,
    })
}
