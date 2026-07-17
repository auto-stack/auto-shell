//! Plan 028 M1.6: Verify all ~80 commands can be bridged into a ToolRegistry.

use auto_shell::Shell;
use ash_core::tool::Tool;

#[test]
fn shell_builds_tool_registry_with_all_commands() {
    let shell = Shell::new();
    let registry = shell.build_tool_registry();
    // We register ~80 commands; assert a healthy lower bound. If commands
    // are removed in future, update this number deliberately.
    assert!(
        registry.len() >= 70,
        "expected >=70 bridged tools, got {}",
        registry.len()
    );
}

#[test]
fn registry_contains_core_commands() {
    let shell = Shell::new();
    let registry = shell.build_tool_registry();
    for name in [
        "ls", "cat", "grep", "find", "rm", "cp", "mv", "mkdir", "echo", "pwd",
    ] {
        assert!(
            registry.get(name).is_some(),
            "expected tool '{}' in registry",
            name
        );
    }
}

#[test]
fn every_bridged_tool_has_valid_schema() {
    let shell = Shell::new();
    let registry = shell.build_tool_registry();
    let catalog = registry.catalog();
    assert!(!catalog.is_empty());
    for desc in &catalog {
        // Every descriptor must have a name, description, and an object schema.
        assert!(!desc.name.is_empty(), "tool with empty name");
        assert!(
            !desc.description.is_empty(),
            "tool {} has empty description",
            desc.name
        );
        assert_eq!(
            desc.parameters.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "tool {} parameters schema is not type=object",
            desc.name
        );
    }
}

#[test]
fn catalog_compact_omits_schemas() {
    let shell = Shell::new();
    let registry = shell.build_tool_registry();
    let compact = registry.catalog_compact();
    assert!(compact.len() >= 70);
    for desc in &compact {
        assert!(
            desc.parameters.is_empty(),
            "compact catalog entry {} has schema",
            desc.name
        );
    }
}

#[test]
fn registry_supports_invocation_returning_internal() {
    // Bridge tools are introspection-only. Calling invoke() must return a
    // Failed(Internal) result pointing the caller at `ash agent run`.
    let shell = Shell::new();
    let registry = shell.build_tool_registry();
    let ls = registry.get("ls").expect("ls should be registered");
    let ctx = ash_core::tool::ToolContext::default();
    let result = ls.invoke(&serde_json::Value::Null, &ctx);
    match result.status {
        ash_core::tool::ToolStatus::Failed(ash_core::tool::ErrorKind::Internal, msg) => {
            assert!(
                msg.contains("agent run"),
                "internal error should mention 'ash agent run', got: {}",
                msg
            );
        }
        other => panic!("expected Failed(Internal), got {:?}", other),
    }
}
