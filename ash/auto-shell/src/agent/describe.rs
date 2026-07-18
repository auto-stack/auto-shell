//! Plan 028 M2.3: `ash agent describe-tools` and `describe-policy`.

use crate::shell::Shell;

/// `ash agent describe-tools [--format json|compact] [--filter <csv>]`
///
/// Exports the tool catalog: every registered command's name, description,
/// and JSON Schema (or just name/description in compact mode). Optional
/// `--filter` keeps only tools whose name starts with one of the given
/// comma-separated prefixes.
pub fn describe_tools(args: &[String]) -> i32 {
    let mut format = "json";
    let mut filter: Option<Vec<String>> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                match args.get(i + 1).map(|s| s.as_str()) {
                    Some("json") | Some("compact") => {
                        format = match args[i + 1].as_str() {
                            "compact" => "compact",
                            _ => "json",
                        };
                        i += 2;
                        continue;
                    }
                    Some(_) => {
                        eprintln!("ash agent describe-tools: --format must be json|compact");
                        return 2;
                    }
                    None => {
                        eprintln!("ash agent describe-tools: --format requires a value");
                        return 2;
                    }
                }
            }
            "--filter" => {
                if let Some(v) = args.get(i + 1) {
                    filter = Some(v.split(',').map(|s| s.trim().to_string()).collect());
                    i += 2;
                    continue;
                } else {
                    eprintln!("ash agent describe-tools: --filter requires a value");
                    return 2;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let shell = Shell::new();
    let registry = shell.build_tool_registry();

    let catalog = if format == "compact" {
        registry.catalog_compact()
    } else {
        registry.catalog()
    };

    let filtered: Vec<_> = match &filter {
        None => catalog,
        Some(prefixes) => catalog
            .into_iter()
            .filter(|d| prefixes.iter().any(|p| d.name.starts_with(p) || &d.name == p))
            .collect(),
    };

    let tools_json: Vec<serde_json::Value> = filtered.iter().map(|d| d.to_json()).collect();
    let envelope = serde_json::json!({
        "schema_version": "1",
        "tool_count": tools_json.len(),
        "tools": tools_json,
    });
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
    0
}

/// `ash agent describe-policy`
///
/// Exports a capability-only summary of the current security policy. Does
/// NOT include specific sandbox paths or deny-list contents — only booleans
/// and counts. Safe to surface in Agent system prompts.
pub fn describe_policy() -> i32 {
    let shell = Shell::new();
    let summary = shell.policy.summarize();
    let env = serde_json::json!({
        "schema_version": "1",
        "policy": summary,
        "note": "Capability-only summary. Specific sandbox paths and deny-list contents are NOT exposed.",
    });
    println!("{}", serde_json::to_string_pretty(&env).unwrap());
    0
}
