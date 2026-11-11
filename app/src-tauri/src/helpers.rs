//! The helper processes: one that answers searches (kept while searches come), and jobs that run to their end and
//! report as they go (indexing, the model download, the speed check).

use crate::native::Job;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

/// `magpie-helper.exe` beside the app.
pub fn helper_path() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let folder = exe.parent().ok_or_else(|| anyhow!("the app has no folder"))?;
    let name = format!("magpie-helper{}", std::env::consts::EXE_SUFFIX);
    let beside = folder.join(&name);
    if beside.exists() {
        return Ok(beside);
    }
    bail!("{name} is missing from {}", folder.display())
}

fn command(arguments: &[&str]) -> Result<Command> {
    let mut command = Command::new(helper_path()?);
    command.args(arguments);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window
    }
    Ok(command)
}

type Waiting = Arc<Mutex<HashMap<u64, mpsc::Sender<Value>>>>;

struct Running {
    child: Child,
    stdin: ChildStdin,
}

/// The search helper. It quits by itself after ten idle minutes; the next request starts it again.
pub struct Serve {
    running: Mutex<Option<Running>>,
    waiting: Waiting,
    next: AtomicU64,
    job: Arc<Option<Job>>,
}

impl Serve {
    pub fn new(job: Arc<Option<Job>>) -> Serve {
        Serve { running: Mutex::new(None), waiting: Arc::default(), next: AtomicU64::new(1), job }
    }

    fn start(&self) -> Result<Running> {
        let mut child = command(&["serve"])?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| anyhow!("can't start the search helper: {error}"))?;
        if let Some(job) = self.job.as_ref() {
            job.add(&child);
        }
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("the helper has no input"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("the helper has no output"))?;
        let waiting = self.waiting.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
                if let Some(id) = value["id"].as_u64() {
                    if let Some(sender) = waiting.lock().unwrap().remove(&id) {
                        let _ = sender.send(value);
                    }
                }
            }
            // The helper is gone: whoever still waits gets an answer now rather than a timeout.
            for (_, sender) in waiting.lock().unwrap().drain() {
                let _ = sender.send(json!({"ok": false, "error": "the search helper stopped"}));
            }
        });
        Ok(Running { child, stdin })
    }

    /// Sends one request and waits for its answer.
    pub fn ask(&self, mut request: Value, timeout: Duration) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        request["id"] = json!(id);
        let (sender, receiver) = mpsc::channel();
        self.waiting.lock().unwrap().insert(id, sender);
        {
            let mut running = self.running.lock().unwrap();
            let alive = match running.as_mut() {
                Some(current) => matches!(current.child.try_wait(), Ok(None)),
                None => false,
            };
            if !alive {
                *running = Some(self.start()?);
            }
            let current = running.as_mut().unwrap();
            let sent = writeln!(current.stdin, "{request}").and_then(|_| current.stdin.flush());
            if sent.is_err() {
                // It quit between the check and the write: once more with a new one.
                *running = Some(self.start()?);
                let current = running.as_mut().unwrap();
                writeln!(current.stdin, "{request}")?;
                current.stdin.flush()?;
            }
        }
        let answer = receiver.recv_timeout(timeout).map_err(|_| {
            self.waiting.lock().unwrap().remove(&id);
            anyhow!("the search took too long")
        })?;
        if answer["ok"].as_bool() == Some(true) {
            Ok(answer["result"].clone())
        } else {
            bail!("{}", answer["error"].as_str().unwrap_or("the search failed"))
        }
    }

    pub fn stop(&self) {
        if let Some(mut running) = self.running.lock().unwrap().take() {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }
}

/// A helper that runs to its end. Each line it prints is one JSON event, handed to `on_event`; the last one
/// (`finished` or `error`) is also what `wait` returns.
pub struct Task {
    child: Arc<Mutex<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
}

impl Task {
    pub fn start(
        arguments: &[&str], job: &Option<Job>, mut on_event: impl FnMut(Value) + Send + 'static,
        on_end: impl FnOnce() + Send + 'static,
    ) -> Result<Task> {
        let mut child = command(arguments)?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| anyhow!("can't start the helper: {error}"))?;
        if let Some(job) = job {
            job.add(&child);
        }
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("the helper has no output"))?;
        let child = Arc::new(Mutex::new(child));
        let watched = child.clone();
        std::thread::spawn(move || {
            let mut ended = false;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    ended = ended || matches!(value["event"].as_str(), Some("finished" | "error"));
                    on_event(value);
                }
            }
            let _ = watched.lock().unwrap().wait();
            if !ended {
                on_event(json!({"event": "error", "error": "the helper stopped unexpectedly"}));
            }
            on_end();
        });
        Ok(Task { child, stdin: Mutex::new(stdin) })
    }

    /// A line to the helper: `pause`, `resume` or `stop`.
    pub fn tell(&self, word: &str) {
        if let Some(stdin) = self.stdin.lock().unwrap().as_mut() {
            let _ = writeln!(stdin, "{word}");
            let _ = stdin.flush();
        }
    }

    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
    }
}

/// Runs a helper command that prints one JSON line and ends (`status`, `estimate`).
pub fn once(arguments: &[&str]) -> Result<Value> {
    let output = command(arguments)?.stdin(Stdio::null()).stderr(Stdio::null()).output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().last().ok_or_else(|| anyhow!("the helper said nothing"))?;
    let value: Value = serde_json::from_str(line)?;
    if value["event"].as_str() == Some("error") {
        bail!("{}", value["error"].as_str().unwrap_or("the helper failed"));
    }
    Ok(value)
}
