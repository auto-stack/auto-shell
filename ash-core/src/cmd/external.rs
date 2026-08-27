use miette::{miette, IntoDiagnostic, Result};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

use crate::pipeline::ExternalStream;

/// Plan 074 E2: execute a single external command with stdout AND stderr
/// piped, streaming each output line to `tx` as it arrives (arrival-order
/// interleaving of both streams, like terminal inheritance looks). The
/// frontend renders a live tail preview from this channel and prints the
/// frozen output after completion.
///
/// Semantics mirror [`execute_external`]'s inherit path + the capture path's
/// return contract: success → `Some(trimmed combined output)` (`None` when
/// empty), failure → `Err("Command failed: {stderr}")` so the caller's
/// exit-code extraction chain stays unchanged. Direct spawn failure falls
/// back to the platform shell chain (un-tailed — same behavior as the normal
/// path, rare for real executables).
pub fn execute_external_tailed(
    input: &str,
    current_dir: &Path,
    tx: &std::sync::mpsc::Sender<String>,
) -> Result<Option<String>> {
    let parts = parse_command(input);
    if parts.is_empty() {
        return Ok(None);
    }
    let cmd_name = &parts[0];
    let args = &parts[1..];

    let mut cmd = Command::new(cmd_name);
    cmd.args(args)
        .current_dir(current_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    restore_sigint_in_child(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => {
            // Direct spawn failed (shell builtin, not on PATH, …) — run the
            // platform fallback chain un-tailed, exactly like the normal path.
            #[cfg(windows)]
            {
                if let Ok(ps_result) =
                    try_execute_powershell(cmd_name, args, current_dir, false)
                {
                    return Ok(ps_result);
                }
            }
            #[cfg(unix)]
            {
                for shell in &["sh", "bash", "zsh"] {
                    if let Ok(shell_result) =
                        try_execute_with_shell(cmd_name, args, current_dir, shell, false)
                    {
                        return Ok(shell_result);
                    }
                }
            }
            return Err(miette!("command not found: {}", cmd_name));
        }
    };

    // Two reader threads → one channel: lines interleave in arrival order.
    let read_pipe = |rd: Option<Box<dyn Read + Send>>, tx: std::sync::mpsc::Sender<String>| {
        std::thread::spawn(move || {
            let mut collected = String::new();
            if let Some(rd) = rd {
                for line in BufReader::new(rd).lines().map_while(Result::ok) {
                    let _ = tx.send(line.clone());
                    collected.push_str(&line);
                    collected.push('\n');
                }
            }
            collected
        })
    };
    let stdout_handle = read_pipe(
        child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        tx.clone(),
    );
    let stderr_handle = read_pipe(
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        tx.clone(),
    );

    let status = child.wait().into_diagnostic()?;
    let stdout_text = stdout_handle.join().unwrap_or_default();
    let stderr_text = stderr_handle.join().unwrap_or_default();

    if status.success() {
        let combined = format!("{stdout_text}{stderr_text}");
        let trimmed = combined.trim();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed.to_string()))
        }
    } else {
        Err(miette!("Command failed: {}", stderr_text.trim()))
    }
}

/// Execute an external command with platform-specific fallbacks.
///
/// When `capture_output` is false (default), the child process inherits
/// the terminal's stdin/stdout/stderr for real-time output — suitable for
/// standalone commands like `cargo build`.
///
/// When `capture_output` is true, stdout is captured and returned as a
/// string — suitable for pipeline usage where output feeds into the next
/// command.
///
/// On Windows: Tries command directly → PowerShell → CMD
/// On Unix: Tries command directly → sh (or bash/zsh if available)
pub fn execute_external(input: &str, current_dir: &Path, capture_output: bool) -> Result<Option<String>> {
    // Parse command and arguments
    let parts = parse_command(input);

    if parts.is_empty() {
        return Ok(None);
    }

    let cmd_name = &parts[0];
    let args = &parts[1..];

    // Try to execute the command directly first
    let direct_result = try_execute_command(cmd_name, args, current_dir, capture_output);

    // If direct execution failed, try platform-specific fallbacks
    if direct_result.is_err() {
        #[cfg(windows)]
        {
            // Windows: Try PowerShell, then CMD
            if let Ok(ps_result) = try_execute_powershell(cmd_name, args, current_dir, capture_output) {
                return Ok(ps_result);
            }
            // Note: We could try CMD here, but most things that work in CMD also work in PowerShell
        }

        #[cfg(unix)]
        {
            // Unix: Try sh, then bash, then zsh
            for shell in &["sh", "bash", "zsh"] {
                if let Ok(shell_result) = try_execute_with_shell(cmd_name, args, current_dir, shell, capture_output)
                {
                    return Ok(shell_result);
                }
            }
        }
    }

    direct_result
}

/// Spawn an external command and return a streaming ExternalStream.
///
/// Unlike `execute_external`, this spawns the process with piped stdout
/// and returns an `ExternalStream` that can be read incrementally.
/// This is the streaming equivalent of `capture_output = true`.
///
/// Stderr is inherited (goes to terminal) so the user can see error
/// messages in real time.
pub fn spawn_external_stream(input: &str, current_dir: &Path) -> Result<ExternalStream> {
    spawn_external_stream_impl(input, current_dir, None)
}

/// Spawn an external command with stdin data, returning a streaming ExternalStream.
///
/// Like `spawn_external_stream`, but pipes the given `stdin_data` to the
/// child process's stdin before returning. The write happens in a background
/// thread so the main thread can immediately start reading stdout.
pub fn spawn_external_stream_with_input(
    input: &str,
    current_dir: &Path,
    stdin_data: &str,
) -> Result<ExternalStream> {
    spawn_external_stream_impl(input, current_dir, Some(stdin_data))
}

/// Spawn an external command with its stdin connected directly to a previous
/// command's stdout pipe (true OS-level pipe chaining).
///
/// This avoids buffering intermediate data in memory: the kernel handles
/// the data flow between processes. Used for `external A | external B` chains.
///
/// Returns a streaming `ExternalStream` for the new process's stdout.
pub fn spawn_external_chained(
    input: &str,
    current_dir: &Path,
    stdin_source: ChildStdout,
) -> Result<ExternalStream> {
    let parts = parse_command(input);

    if parts.is_empty() {
        return Err(miette!("empty command"));
    }

    let cmd_name = &parts[0];
    let args = &parts[1..];

    // Direct spawn — chain only works with direct executables on PATH.
    // If the command isn't found, the error propagates (no string fallback).
    let mut cmd = Command::new(cmd_name);
    cmd.args(args)
        .current_dir(current_dir)
        .stdin(Stdio::from(stdin_source)) // OS pipe: prev stdout → this stdin
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    restore_sigint_in_child(&mut cmd);
    let child = cmd.spawn().into_diagnostic()?;
    Ok(ExternalStream::new(child))
}

/// Spawn an external command in the background (for `cmd &` syntax).
///
/// The child process inherits stdout/stderr (so output still goes to the
/// terminal) but has stdin set to null (no input). The caller receives the
/// raw `Child` handle for job-control tracking — we do **not** wait for it.
pub fn spawn_external_background(input: &str, current_dir: &Path) -> Result<Child> {
    let parts = parse_command(input);

    if parts.is_empty() {
        return Err(miette!("empty command"));
    }

    let cmd_name = &parts[0];
    let args = &parts[1..];

    // Try direct spawn first
    let direct = try_spawn_background(cmd_name, args, current_dir);
    if direct.is_ok() {
        return direct;
    }

    // Platform fallbacks
    #[cfg(windows)]
    {
        let ps_cmd = format!(
            "{}{}",
            cmd_name,
            args.iter()
                .map(|arg| format!(" \"{arg}\""))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", &ps_cmd])
            .current_dir(current_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        restore_sigint_in_child(&mut cmd);
        if let Ok(child) = cmd.spawn().into_diagnostic() {
            return Ok(child);
        }
    }

    #[cfg(unix)]
    {
        for shell in &["sh", "bash", "zsh"] {
            let shell_cmd = format!(
                "{} {}",
                cmd_name,
                args.iter()
                    .map(|arg| format!("\"{}\"", arg.replace('"', "\\\"")))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let mut cmd = Command::new(shell);
            cmd.arg("-c")
                .arg(&shell_cmd)
                .current_dir(current_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            restore_sigint_in_child(&mut cmd);
            if let Ok(child) = cmd.spawn().into_diagnostic() {
                return Ok(child);
            }
        }
    }

    direct
}

/// Try to spawn a background command directly.
fn try_spawn_background(cmd_name: &str, args: &[String], current_dir: &Path) -> Result<Child> {
    let mut cmd = Command::new(cmd_name);
    cmd.args(args)
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    restore_sigint_in_child(&mut cmd);
    cmd.spawn().into_diagnostic()
}

/// Internal: shared implementation for both spawn variants.
fn spawn_external_stream_impl(
    input: &str,
    current_dir: &Path,
    stdin_data: Option<&str>,
) -> Result<ExternalStream> {
    let parts = parse_command(input);

    if parts.is_empty() {
        return Err(miette!("empty command"));
    }

    let cmd_name = &parts[0];
    let args = &parts[1..];

    // Try direct spawn first
    let direct_result = try_spawn_command_impl(cmd_name, args, current_dir, stdin_data);

    if direct_result.is_err() {
        #[cfg(windows)]
        {
            if let Ok(ps_result) =
                try_spawn_powershell_impl(cmd_name, args, current_dir, stdin_data)
            {
                return Ok(ps_result);
            }
        }

        #[cfg(unix)]
        {
            for shell in &["sh", "bash", "zsh"] {
                if let Ok(shell_result) =
                    try_spawn_with_shell_impl(cmd_name, args, current_dir, shell, stdin_data)
                {
                    return Ok(shell_result);
                }
            }
        }
    }

    direct_result
}

/// Try to spawn a command directly with piped stdout (and optional stdin).
fn try_spawn_command_impl(
    cmd_name: &str,
    args: &[String],
    current_dir: &Path,
    stdin_data: Option<&str>,
) -> Result<ExternalStream> {
    let mut cmd = Command::new(cmd_name);
    cmd.args(args)
        .current_dir(current_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    }

    restore_sigint_in_child(&mut cmd);
    let child = cmd.spawn().into_diagnostic()?;

    match stdin_data {
        Some(data) => Ok(ExternalStream::new_with_stdin(child, data.to_string())),
        None => Ok(ExternalStream::new(child)),
    }
}

/// Try to spawn a command via PowerShell on Windows (with optional stdin).
#[cfg(windows)]
fn try_spawn_powershell_impl(
    cmd_name: &str,
    args: &[String],
    current_dir: &Path,
    stdin_data: Option<&str>,
) -> Result<ExternalStream> {
    let ps_cmd = format!(
        "{}{}",
        cmd_name,
        args.iter()
            .map(|arg| format!(" \"{arg}\""))
            .collect::<Vec<_>>()
            .join(" ")
    );

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", &ps_cmd])
        .current_dir(current_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    }

    restore_sigint_in_child(&mut cmd);
    let child = cmd.spawn().into_diagnostic()?;

    match stdin_data {
        Some(data) => Ok(ExternalStream::new_with_stdin(child, data.to_string())),
        None => Ok(ExternalStream::new(child)),
    }
}

/// Try to spawn a command via a Unix shell (with optional stdin).
#[cfg(unix)]
fn try_spawn_with_shell_impl(
    cmd_name: &str,
    args: &[String],
    current_dir: &Path,
    shell: &str,
    stdin_data: Option<&str>,
) -> Result<ExternalStream> {
    let shell_cmd = format!(
        "{} {}",
        cmd_name,
        args.iter()
            .map(|arg| format!("\"{}\"", arg.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(" ")
    );

    let mut cmd = Command::new(shell);
    cmd.arg("-c")
        .arg(&shell_cmd)
        .current_dir(current_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    }

    restore_sigint_in_child(&mut cmd);
    let child = cmd.spawn().into_diagnostic()?;

    match stdin_data {
        Some(data) => Ok(ExternalStream::new_with_stdin(child, data.to_string())),
        None => Ok(ExternalStream::new(child)),
    }
}

/// Try to execute a command directly using std::process::Command
///
/// When `capture_output` is false, uses `.status()` with inherited stdio
/// for real-time terminal output (e.g. `cargo build`).
///
/// When `capture_output` is true, uses `.output()` to capture stdout
/// for pipeline consumption.
fn try_execute_command(
    cmd_name: &str,
    args: &[String],
    current_dir: &Path,
    capture_output: bool,
) -> Result<Option<String>> {
    if capture_output {
        let mut cmd = Command::new(cmd_name);
        cmd.args(args)
            .current_dir(current_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        restore_sigint_in_child(&mut cmd);
        let output = cmd.output().into_diagnostic()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let trimmed = stdout.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(miette!("Command failed: {}", stderr.trim()))
        }
    } else {
        let mut cmd = Command::new(cmd_name);
        cmd.args(args).current_dir(current_dir);
        restore_sigint_in_child(&mut cmd);
        let status = cmd.status().into_diagnostic()?;

        if status.success() {
            Ok(None) // Output already went to terminal
        } else {
            Err(miette!(
                "Command failed with exit code: {}",
                status.code().unwrap_or(-1)
            ))
        }
    }
}

/// Try to execute a command via a Unix shell (sh/bash/zsh)
#[cfg(unix)]
fn try_execute_with_shell(
    cmd_name: &str,
    args: &[String],
    current_dir: &Path,
    shell: &str,
    capture_output: bool,
) -> Result<Option<String>> {
    // Build shell command: sh -c "cmd arg1 arg2..."
    let shell_cmd = format!(
        "{} {}",
        cmd_name,
        args.iter()
            .map(|arg| format!("\"{}\"", arg.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(" ")
    );

    if capture_output {
        let mut cmd = Command::new(shell);
        cmd.arg("-c")
            .arg(&shell_cmd)
            .current_dir(current_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        restore_sigint_in_child(&mut cmd);
        let output = cmd.output().into_diagnostic()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let trimmed = stdout.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(miette!("{} command failed: {}", shell, stderr.trim()))
        }
    } else {
        let mut cmd = Command::new(shell);
        cmd.arg("-c")
            .arg(&shell_cmd)
            .current_dir(current_dir);
        restore_sigint_in_child(&mut cmd);
        let status = cmd.status().into_diagnostic()?;

        if status.success() {
            Ok(None)
        } else {
            Err(miette!(
                "{} command failed with exit code: {}",
                shell,
                status.code().unwrap_or(-1)
            ))
        }
    }
}

/// Try to execute a command via PowerShell on Windows
#[cfg(windows)]
fn try_execute_powershell(
    cmd_name: &str,
    args: &[String],
    current_dir: &Path,
    capture_output: bool,
) -> Result<Option<String>> {
    // Build PowerShell command
    // Use -Command with encoded arguments
    let ps_cmd = format!(
        "{}{}",
        cmd_name,
        args.iter()
            .map(|arg| format!(" \"{arg}\""))
            .collect::<Vec<_>>()
            .join(" ")
    );

    if capture_output {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", &ps_cmd])
            .current_dir(current_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        restore_sigint_in_child(&mut cmd);
        let output = cmd.output().into_diagnostic()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let trimmed = stdout.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(miette!("PowerShell command failed: {}", stderr.trim()))
        }
    } else {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", &ps_cmd])
            .current_dir(current_dir);
        restore_sigint_in_child(&mut cmd);
        let status = cmd.status().into_diagnostic()?;

        if status.success() {
            Ok(None)
        } else {
            Err(miette!(
                "PowerShell command failed with exit code: {}",
                status.code().unwrap_or(-1)
            ))
        }
    }
}

/// Parse command into parts (respecting quotes)
pub fn parse_command(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' | '\t' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    parts.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

/// Unix: restore SIGINT to default in the child process.
///
/// The parent shell sets SIGINT to a handler that catches Ctrl+C
/// (so ASH survives). Without this fix, the child would inherit
/// the catch handler and also ignore Ctrl+C. We restore SIG_DFL
/// in the child so it terminates normally on Ctrl+C.
#[cfg(unix)]
fn libc_restore_sigint() {
    const SIGINT: i32 = 2;
    const SIG_DFL: usize = 0;
    extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    unsafe {
        signal(SIGINT, SIG_DFL);
    }
}

/// Apply SIGINT restoration pre_exec hook to a Command on Unix.
#[cfg(unix)]
fn restore_sigint_in_child(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            libc_restore_sigint();
            Ok(())
        });
    }
}

/// No-op on Windows (children handle Ctrl+C via console events).
#[cfg(windows)]
fn restore_sigint_in_child(_cmd: &mut std::process::Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let parts = parse_command("echo hello world");
        assert_eq!(parts, vec!["echo", "hello", "world"]);
    }

    #[test]
    fn test_parse_with_quotes() {
        let parts = parse_command("echo \"hello world\" 'foo bar'");
        assert_eq!(parts, vec!["echo", "hello world", "foo bar"]);
    }

    #[test]
    fn test_parse_mixed_quotes() {
        let parts = parse_command("echo \"it's\" 'foo\"bar'");
        assert_eq!(parts, vec!["echo", "it's", "foo\"bar"]);
    }

    #[test]
    fn test_parse_empty() {
        let parts = parse_command("");
        assert!(parts.is_empty());
    }

    #[test]
    fn test_parse_single_word() {
        let parts = parse_command("echo");
        assert_eq!(parts, vec!["echo"]);
    }

    /// Plan 309 / Task 1.1 — verify true OS-level pipe chaining.
    ///
    /// Two external `sort` processes are connected with a real kernel pipe:
    /// producer's `ChildStdout` becomes the consumer's stdin via
    /// `spawn_external_chained`, with NO in-memory buffering in between.
    /// `sort` exists on both Unix (GNU coreutils) and Windows (System32).
    #[test]
    fn test_spawn_external_chained_os_pipe() {
        let dir = std::env::temp_dir();

        // Producer: `sort` with piped stdin data → sorted stdout.
        let producer = spawn_external_stream_with_input(
            "sort",
            &dir,
            "cherry\napple\nbanana\n",
        )
        .expect("producer sort should spawn");

        // Hand the producer's raw stdout to the consumer via an OS pipe.
        let prev_stdout = producer.into_raw_stdout();
        let consumer =
            spawn_external_chained("sort", &dir, prev_stdout).expect("consumer should chain");

        let output = consumer
            .read_all()
            .expect("should read consumer output");
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines, vec!["apple", "banana", "cherry"]);
    }

    /// Plan 309 / Task 1.1 — the chained pipe must not deadlock on large
    /// volumes, proving the kernel pipe (not an in-memory buffer) carries
    /// the data. If we buffered, this would either OOM or block forever.
    #[test]
    fn test_spawn_external_chained_large_volume_streaming() {
        let dir = std::env::temp_dir();

        // Generate ~200k lines via the shell, sort once (producer), sort
        // again (consumer) — fully OS-piped.
        //
        // We use `yes`-style volume without `yes`: feed a big string into the
        // producer's stdin instead (still exercises the full OS pipe path).
        let big: String = (0..200_000)
            .map(|i| format!("line-{i}\n"))
            .collect();

        let producer =
            spawn_external_stream_with_input("sort", &dir, &big).expect("producer should spawn");
        let prev_stdout = producer.into_raw_stdout();
        let consumer =
            spawn_external_chained("sort", &dir, prev_stdout).expect("consumer should chain");

        let output = consumer.read_all().expect("should read all without deadlock");
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 200_000);
        // Verify it is actually sorted (proves the consumer sort ran on real data).
        let mut sorted = lines.to_vec();
        sorted.sort_unstable();
        assert_eq!(lines, sorted.as_slice());
    }

    // ── Plan 074 E2: tailed execution ─────────────────────────────────

    fn tailed_run(cmd: &str) -> (Vec<String>, Result<Option<String>>) {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        // Separate thread so we can drain while it runs (mirrors the
        // frontend render thread). `cmd` is owned by the closure ('static).
        let cmd = cmd.to_string();
        let t = {
            let tx = tx.clone();
            std::thread::spawn(move || {
                execute_external_tailed(&cmd, std::path::Path::new("."), &tx)
            })
        };
        drop(tx); // only the worker's clones remain — loop ends when they drop
        let mut lines = Vec::new();
        for line in rx {
            lines.push(line);
        }
        (lines, t.join().expect("tailed thread"))
    }

    #[test]
    fn tailed_streams_lines_and_returns_output() {
        #[cfg(windows)]
        let cmd = "cmd /c echo tailed_hello";
        #[cfg(unix)]
        let cmd = "echo tailed_hello";
        let (lines, result) = tailed_run(cmd);
        assert!(
            lines.iter().any(|l| l.contains("tailed_hello")),
            "live channel got the line: {lines:?}"
        );
        assert_eq!(result.unwrap().unwrap().trim(), "tailed_hello");
    }

    #[test]
    fn tailed_nonzero_exit_is_error_with_code() {
        #[cfg(windows)]
        let cmd = "cmd /c exit 3";
        #[cfg(unix)]
        let cmd = "sh -c \"exit 3\"";
        let (_lines, result) = tailed_run(cmd);
        assert!(result.is_err(), "non-zero exit must surface as Err");
    }

    #[test]
    fn tailed_multi_line_output_all_lines_arrive() {
        #[cfg(windows)]
        let cmd = "cmd /c echo one&echo two";
        #[cfg(unix)]
        let cmd = "printf \"one\ntwo\n\"";
        let (lines, result) = tailed_run(cmd);
        assert!(lines.iter().any(|l| l.contains("one")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("two")), "{lines:?}");
        let out = result.unwrap().unwrap();
        assert!(out.contains("one") && out.contains("two"), "{out}");
    }
}
