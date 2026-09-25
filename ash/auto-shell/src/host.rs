// Plan 011 (MS3-B): ShellHost bridge implementation for ash.
//
// `ShellHostImpl` is registered on the AutoVM (`vm.set_host`) so that
// AutoLang natives `system()` / `system_status()` / `export()` / `exit()`
// can call back into the shell. The hard part: native shims run *inside*
// `session.run()`, which itself runs while `&mut Shell` is borrowed — so the
// host cannot own the Shell. Instead it holds a raw `*mut Shell` that ash
// sets/clears around each `session.run()` call (single-threaded; the pointer
// is only dereferenced while `run()` holds the borrow live, which is exactly
// when the Shell is alive and not moving).
//
// SAFETY: `ShellPtr(*mut Shell)` is `!Send` by default, but ash scripts run
// on a single thread, so we mark it `Send + Sync` to satisfy the
// `Arc<dyn ShellHost>` (`Send + Sync`) requirement. The pointer is only
// touched on the owning thread.

use crate::shell::Shell;
use std::sync::{Arc, Mutex};

/// Raw pointer to the Shell, marked Send+Sync for single-threaded use.
#[derive(Clone, Copy)]
struct ShellPtr(*mut Shell);
unsafe impl Send for ShellPtr {}
unsafe impl Sync for ShellPtr {}

/// Mutable host state behind a Mutex (so &self methods can mutate it).
struct HostState {
    /// Set by ash before `session.run()`, cleared after. Null when not running.
    shell: ShellPtr,
    /// Set by `exit()`; the script loop checks this between lines.
    exit_requested: bool,
    exit_code: i32,
}

impl HostState {
    fn new() -> Self {
        Self {
            shell: ShellPtr(std::ptr::null_mut()),
            exit_requested: false,
            exit_code: 0,
        }
    }

    /// SAFETY: caller must ensure `shell` outlives the run and is on the same
    /// thread. ash sets this immediately before `session.run()`.
    unsafe fn set_shell(&mut self, shell: *mut Shell) {
        self.shell = ShellPtr(shell);
    }

    fn clear_shell(&mut self) {
        self.shell = ShellPtr(std::ptr::null_mut());
    }

    /// SAFETY: only call while `shell` is non-null and the borrow is live
    /// (i.e. inside `session.run()` on the owning thread).
    unsafe fn shell(&self) -> Option<&mut Shell> {
        let p = self.shell.0;
        if p.is_null() {
            None
        } else {
            Some(&mut *p)
        }
    }
}

/// The ash-side ShellHost. Shared (Arc) so it can live on the VM as
/// `Option<SharedHost>`. Created once per Shell and reused.
pub struct ShellHostImpl {
    state: Arc<Mutex<HostState>>,
}

impl ShellHostImpl {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(HostState::new())),
        }
    }

    /// A clonable handle to install on the VM. The inner state is shared.
    pub fn shared(&self) -> Arc<dyn auto_lang::host::ShellHost> {
        // Bridge type: holds the same Arc<Mutex<HostState>>.
        Arc::new(ShellHostBridge {
            state: self.state.clone(),
        })
    }

    /// SAFETY: call immediately before `session.run()`. `shell` must remain
    /// alive and pinned on this thread until `end_run()` is called.
    pub unsafe fn begin_run(&self, shell: *mut Shell) {
        let mut s = self.state.lock().unwrap();
        s.set_shell(shell);
        s.exit_requested = false;
        s.exit_code = 0;
    }

    /// Call immediately after `session.run()` returns. Clears the pointer.
    pub fn end_run(&self) {
        let mut s = self.state.lock().unwrap();
        s.clear_shell();
    }

    /// True if `exit()` was called during the last run.
    pub fn exit_requested(&self) -> bool {
        self.state.lock().unwrap().exit_requested
    }

    /// The code passed to `exit()` (0 if none).
    pub fn exit_code(&self) -> i32 {
        self.state.lock().unwrap().exit_code
    }

    /// PLAN-082 F2 follow-up: clear `exit()` bookkeeping between
    /// REPL-driven script runs (`reset_script_state`) — the file-mode caller
    /// exits the process instead of reusing the host.
    pub fn reset_exit_state(&self) {
        let mut s = self.state.lock().unwrap();
        s.exit_requested = false;
        s.exit_code = 0;
    }
}

/// The Send+Sync bridge stored on the VM. Forwards to the shared state.
struct ShellHostBridge {
    state: Arc<Mutex<HostState>>,
}

impl auto_lang::host::ShellHost for ShellHostBridge {
    fn system(&self, cmd: &str) -> String {
        let s = self.state.lock().unwrap();
        // SAFETY: only non-null during session.run() on the owning thread.
        unsafe {
            match s.shell() {
                Some(shell) => {
                    // execute_capture runs in bash-compat mode so structured
                    // commands (find/ls/wc) return bash-style plain text
                    // instead of ratatui tables — `system()` callers expect
                    // machine-readable stdout (Plan 036 defect-A fix).
                    match shell.execute_capture(cmd) {
                        Ok(Some(out)) => out.trim_end_matches('\n').to_string(),
                        Ok(None) => String::new(),
                        Err(_) => String::new(),
                    }
                }
                None => String::new(),
            }
        }
    }

    fn system_status(&self) -> i32 {
        let s = self.state.lock().unwrap();
        unsafe {
            match s.shell() {
                Some(shell) => shell.last_exit_code(),
                None => 0,
            }
        }
    }

    fn export(&self, key: &str, val: &str) {
        let s = self.state.lock().unwrap();
        unsafe {
            if let Some(shell) = s.shell() {
                shell.set_env_var(key, val);
            }
        }
    }

    fn exit(&self, code: i32) {
        let mut s = self.state.lock().unwrap();
        s.exit_requested = true;
        s.exit_code = code;
    }

    fn exit_requested(&self) -> bool {
        self.state.lock().unwrap().exit_requested
    }

    fn requested_exit_code(&self) -> i32 {
        self.state.lock().unwrap().exit_code
    }

    /// PLAN-082: `shell_query(cmd)` bridge — run the pipeline through
    /// [`Shell::execute_query`] and hand the final-stage structured value
    /// (records) back to the VM. Failures degrade to an empty record array
    /// after printing, mirroring `system()`'s error posture.
    fn query(&self, cmd: &str) -> auto_val::Value {
        let s = self.state.lock().unwrap();
        // SAFETY: only non-null during session.run() on the owning thread.
        unsafe {
            match s.shell() {
                Some(shell) => shell.execute_query(cmd).unwrap_or_else(|e| {
                    eprintln!("Error: {}", e);
                    auto_val::Value::Array(auto_val::Array::new())
                }),
                None => auto_val::Value::Array(auto_val::Array::new()),
            }
        }
    }

    /// PLAN-082: `shell_run(cmd)` bridge — execute through the shell's own
    /// output path (interpolation + printing + error latching live in
    /// [`Shell::execute_host_run`]).
    fn run(&self, cmd: &str) {
        let s = self.state.lock().unwrap();
        // SAFETY: only non-null during session.run() on the owning thread.
        unsafe {
            if let Some(shell) = s.shell() {
                shell.execute_host_run(cmd);
            }
        }
    }
}
