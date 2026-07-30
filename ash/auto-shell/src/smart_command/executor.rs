//! SmartCommand executor — run a spec's `.ash` body (Plan 029 §A.6).
//!
//! The body is an ash script (AutoLang + `> cmd` / `system()` lines) run via
//! [`Shell::execute_script_content`]. Positional arguments from the user are
//! injected as `$1`, `$2`, … via [`Shell::set_script_args`] (Plan 034's
//! mechanism), so bodies reference them with `$1`/`$2`/`$@`/`$#` exactly like a
//! normal ash script.
//!
//! The AI judgment step (SmartCommandRole + Agent) is NOT part of the body in
//! v1 — bodies are deterministic. An AI step, when added later, runs before
//! or after the body from the executor, not as a body native (that would
//! require new AutoVM natives in auto-lang). This keeps v1 cross-repo-free.

use miette::Result;

use crate::shell::Shell;

use super::config::SmartCommandSpec;

/// Run a SmartCommand's body script with the given positional arguments.
///
/// The body's output is printed live to stdout as it executes (that's how
/// `execute_script_content` works — each command prints immediately). This
/// function returns `Ok(())` on success and propagates script errors.
///
/// `args` become `$1`, `$2`, … inside the body.
pub fn execute(spec: &SmartCommandSpec, args: &[String], shell: &mut Shell) -> Result<()> {
    let body_path = spec.body_path().ok_or_else(|| {
        miette::miette!(
            "SmartCommand '{}': no body file (source_path not set or body empty)",
            spec.name
        )
    })?;
    let body_content = std::fs::read_to_string(&body_path).map_err(|e| {
        miette::miette!(
            "SmartCommand '{}': cannot read body '{}': {}",
            spec.name,
            body_path.display(),
            e
        )
    })?;

    // Inject the user's positional args so $1/$2/$@/$# resolve in the body.
    shell.set_script_args(args.to_vec());

    shell.execute_script_content(&body_content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn spec_with_body(dir: &std::path::Path, body_name: &str, body_content: &str) -> SmartCommandSpec {
        let body_path = dir.join(body_name);
        fs::write(&body_path, body_content).unwrap();
        let mut spec = SmartCommandSpec::new("test", "test command");
        spec.body = body_name.to_string();
        // source_path points at a .at file in the same dir; body resolves
        // relative to its parent.
        spec.source_path = Some(dir.join("test.at"));
        spec
    }

    #[test]
    fn runs_simple_body_echo() {
        let tmp = std::env::temp_dir().join("ash_smart_exec_echo");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let spec = spec_with_body(&tmp, "body.ash", "> echo hello-from-body\n");

        let mut shell = Shell::new();
        let result = execute(&spec, &[], &mut shell);
        assert!(result.is_ok(), "{:?}", result);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn injects_positional_args_runs_without_error() {
        // The body references $1; execute must inject the arg via
        // set_script_args so $1 expands. We verify the body runs without
        // error (the value is exercised end-to-end in the manual `ash smart
        // run` test, since stdout isn't capturable in a lib test here).
        let tmp = std::env::temp_dir().join("ash_smart_exec_args");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let spec = spec_with_body(&tmp, "body.ash", "var x = $1\n> echo got-$x\n");

        let mut shell = Shell::new();
        let result = execute(&spec, &["mytarget".to_string()], &mut shell);
        assert!(result.is_ok(), "body referencing $1 should run: {:?}", result);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn errors_when_body_file_missing() {
        let tmp = std::env::temp_dir().join("ash_smart_exec_missing");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let mut spec = SmartCommandSpec::new("x", "desc");
        spec.body = "nonexistent.ash".to_string();
        spec.source_path = Some(tmp.join("x.at"));

        let mut shell = Shell::new();
        let result = execute(&spec, &[], &mut shell);
        assert!(result.is_err(), "missing body should error");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn errors_when_source_path_unset() {
        let spec = SmartCommandSpec::new("x", "desc");
        // body empty → no body_path
        let mut shell = Shell::new();
        assert!(execute(&spec, &[], &mut shell).is_err());
    }

    #[test]
    fn runs_autolang_logic_in_body() {
        // A body that uses AutoLang control flow + system(), not just echo.
        let tmp = std::env::temp_dir().join("ash_smart_exec_auto");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let body = "var names = [\"a\", \"b\", \"c\"]\nfor n in names {\n  > echo item-$n\n}\n";
        let spec = spec_with_body(&tmp, "body.ash", body);

        let mut shell = Shell::new();
        let result = execute(&spec, &[], &mut shell);
        assert!(result.is_ok(), "AutoLang body should run: {:?}", result);
        let _ = fs::remove_dir_all(&tmp);
    }
}
