//! ExternalStream — streaming output from an external child process.
//!
//! Wraps a spawned child process's stdout pipe, providing line-by-line
//! iteration. This is ASH's equivalent of nushell's `ByteStream::child()`.

use std::io::{BufRead, BufReader, Read};
use std::process::{ChildStdout, ExitStatus};
use std::sync::{Arc, Mutex};

/// Streaming output from an external command's stdout pipe.
///
/// Supports line-by-line iteration via `lines()` or full collection
/// via `read_all()`.
///
/// Plan 040 M5: the child's OS PID is captured up front so [`kill`] can
/// terminate the process externally (e.g. on user cancel), even though the
/// `Child` handle itself lives on the background exit-status thread.
pub struct ExternalStream {
    reader: BufReader<ChildStdout>,
    exit_status: Arc<Mutex<Option<ExitStatus>>>,
    /// The child's OS PID, captured before the handle moved to the wait
    /// thread. `None` once the process has been killed or already exited.
    pid: Arc<Mutex<Option<u32>>>,
}

impl ExternalStream {
    /// Create an ExternalStream from a spawned child process.
    ///
    /// The child must have been spawned with `stdout(Stdio::piped())`.
    /// A background thread is spawned to collect the exit status.
    pub fn new(child: std::process::Child) -> Self {
        Self::from_piped_stdout(child)
    }

    /// Create an ExternalStream that also pipes data to the child's stdin.
    ///
    /// The child must have been spawned with both `stdin(Stdio::piped())`
    /// and `stdout(Stdio::piped())`. The stdin data is written in a
    /// background thread so the main thread can immediately start reading
    /// stdout without blocking.
    pub fn new_with_stdin(mut child: std::process::Child, stdin_data: String) -> Self {
        if let Some(mut stdin) = child.stdin.take() {
            std::thread::spawn(move || {
                use std::io::Write;
                let _ = stdin.write_all(stdin_data.as_bytes());
                // stdin is dropped here, closing the pipe → child sees EOF
            });
        }
        Self::from_piped_stdout(child)
    }

    /// Internal: take stdout from child and spawn exit-status thread.
    fn from_piped_stdout(mut child: std::process::Child) -> Self {
        let exit_status: Arc<Mutex<Option<ExitStatus>>> = Arc::new(Mutex::new(None));
        let status_handle = exit_status.clone();

        // Plan 040 M5: capture the PID before moving the handle into the wait
        // thread, so kill() can reach the process by PID later. Stored as an
        // Option so kill() can clear it (one-shot).
        let pid = Arc::new(Mutex::new(Some(child.id())));

        // Take stdout before moving child into background thread
        let stdout = child.stdout.take().expect("stdout was piped");

        // Background thread: wait for process to exit and record status
        std::thread::spawn(move || {
            if let Ok(status) = child.wait() {
                let mut lock = status_handle.lock().unwrap();
                *lock = Some(status);
            }
        });

        Self {
            reader: BufReader::new(stdout),
            exit_status,
            pid,
        }
    }

    /// Plan 040 M5: terminate the underlying child process. Best-effort:
    /// sends a kill to the OS PID. Safe to call after the process has already
    /// exited (the OS returns "no such process", which we ignore). Clears the
    /// stored PID so a second call is a no-op.
    ///
    /// This lets a frontend cancel a long external command for real, rather
    /// than just stopping to read its stdout (which orphans the process).
    pub fn kill(&self) {
        let pid = {
            let mut slot = self.pid.lock().unwrap();
            slot.take() // kill is one-shot
        };
        if let Some(pid) = pid {
            kill_process(pid);
        }
    }

    /// Plan 040 M5: a clonable handle to the kill slot, so a consumer that
    /// moves the stream into `lines()` (consuming it) can still kill the
    /// process by PID from the cloned handle. One-shot: `kill()` clears it.
    pub fn kill_handle(&self) -> Arc<Mutex<Option<u32>>> {
        self.pid.clone()
    }

    /// Plan 040 M5: kill the process referenced by a handle returned from
    /// [`kill_handle`]. Lets a consumer that already moved the stream into
    /// `lines()` still terminate the child. One-shot: clears the PID. No-op if
    /// the process already exited (or was already killed).
    pub fn kill_from_handle(handle: &Arc<Mutex<Option<u32>>>) {
        let pid = {
            let mut slot = handle.lock().unwrap();
            slot.take()
        };
        if let Some(pid) = pid {
            kill_process(pid);
        }
    }

    /// Return an iterator over the lines of output.
    ///
    /// Each item is a `Result<String>` — an `Err` means the pipe was
    /// broken (e.g. the child crashed).
    pub fn lines(self) -> impl Iterator<Item = Result<String, std::io::Error>> {
        self.reader.lines()
    }

    /// Read all remaining output into a single String.
    pub fn read_all(mut self) -> Result<String, std::io::Error> {
        let mut buf = String::new();
        self.reader.read_to_string(&mut buf)?;
        Ok(buf)
    }

    /// Read all remaining output, trimming trailing whitespace.
    pub fn read_all_trimmed(self) -> Option<String> {
        match self.read_all() {
            Ok(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Err(_) => None,
        }
    }

    /// Consume the ExternalStream and return the raw `ChildStdout`.
    ///
    /// This is used for **OS-level pipe chaining**: the stdout of one
    /// external process becomes the stdin of the next without buffering
    /// in memory. The background exit-status thread continues running
    /// independently (it owns the `Child`, not the stdout).
    ///
    /// # Panics
    ///
    /// Panics if the `BufReader` internal buffer is not empty (i.e. data
    /// has already been read from this stream). In practice this is never
    /// called after reading — only during pipeline construction.
    pub fn into_raw_stdout(self) -> ChildStdout {
        self.reader.into_inner()
    }

    /// Get the exit status if the process has finished.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.exit_status.lock().unwrap().clone()
    }

    /// Clone the shared exit-status handle (Plan 057: ash-server streaming
    /// path polls it after draining stdout to surface the child's real exit
    /// code — consuming the stream via `lines()` would orphan the only other
    /// reference).
    pub fn exit_status_handle(&self) -> Arc<Mutex<Option<ExitStatus>>> {
        self.exit_status.clone()
    }

    /// Returns true if the process has finished.
    pub fn is_finished(&self) -> bool {
        self.exit_status.lock().unwrap().is_some()
    }
}

/// Plan 040 M5: terminate a process by PID, cross-platform.
///
/// - Unix: `kill -9 <pid>` via the system `kill` utility (avoids a libc dep).
/// - Windows: `taskkill /PID <pid> /T /F` (force-kill the process tree, so
///   spawned grandchildren die too — important for e.g. a cancelled build).
///
/// Failures are ignored: the process may have already exited (ESRCH / "no such
/// process"), in which case there's nothing to kill.
fn kill_process(pid: u32) {
    let result = if cfg!(unix) {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
    } else {
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
    };
    // Ignore the result — a missing process is the expected "already done".
    let _ = result;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn test_external_stream_read_all() {
        let child = Command::new("echo")
            .arg("hello world")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("echo should spawn");

        let stream = ExternalStream::new(child);
        let output = stream.read_all().expect("should read all");
        assert!(output.contains("hello world"));
    }

    #[test]
    fn test_external_stream_lines() {
        let child = Command::new("echo")
            .arg("line1\nline2")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("echo should spawn");

        let stream = ExternalStream::new(child);
        let lines: Vec<String> = stream
            .lines()
            .filter_map(|l| l.ok())
            .collect();

        assert!(!lines.is_empty());
    }

    #[test]
    fn test_external_stream_with_stdin() {
        // `sort` reads stdin and writes sorted lines to stdout
        let child = Command::new("sort")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("sort should spawn");

        let stream = ExternalStream::new_with_stdin(child, "cherry\napple\nbanana\n".to_string());
        let output = stream.read_all().expect("should read all");
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines, vec!["apple", "banana", "cherry"]);
    }

    #[test]
    fn test_external_stream_exit_status() {
        let child = Command::new("echo")
            .arg("test")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("echo should spawn");

        let stream = ExternalStream::new(child);
        // Give the process time to finish
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(stream.is_finished());
        let status = stream.exit_status().unwrap();
        assert!(status.success());
    }
}
