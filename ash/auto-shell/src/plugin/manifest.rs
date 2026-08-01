//! Plugin manifest — the parsed form of a `plugin.at` file (Plan 033 §3).
//!
//! A `plugin.at` file declares one plugin's metadata, what it contributes, and
//! the capabilities it needs:
//!
//! ```text
//! plugin {
//!     name        : "my-git-extras"
//!     version     : "0.1.0"
//!     author      : "zhaopuming"
//!     description : "Extra git SmartCommands"
//!     contributions : {
//!         completions : true
//!         functions   : true
//!         smart       : true
//!         config      : false
//!     }
//!     capabilities : {
//!         reads_fs       : true
//!         writes_fs      : true
//!         spawns_process : true
//!         uses_network   : false
//!     }
//!     min_ash_version : "0.5.0"
//!     enabled : true
//! }
//! ```
//!
//! `name` and `version` are required; everything else is optional with sensible
//! defaults (`enabled` defaults to `true`, all contributions/capabilities
//! default to `false`). The parser is intentionally minimal — it handles one
//! `plugin { ... }` block with optional `contributions { }` / `capabilities { }`
//! nested blocks, quoted-string values, and `true`/`false` booleans. Unknown
//! keys are ignored for forward-compat.
//!
//! Parsing style mirrors `smart_command::config::parse_at` (self-contained,
//! hand-rolled, no external `.at` parser dependency).

use std::fmt;

/// A parsed plugin manifest — the contents of a `plugin.at` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    /// Plugin name (required). Also the directory name under `plugins/`.
    pub name: String,
    /// Semantic version string (required), e.g. `"0.1.0"`.
    pub version: String,
    /// Plugin author (optional).
    pub author: Option<String>,
    /// One-line human description (optional).
    pub description: Option<String>,
    /// Homepage URL (optional).
    pub homepage: Option<String>,
    /// What content this plugin contributes.
    pub contributions: PluginContributions,
    /// Declared capabilities (shown to the user on first load).
    pub capabilities: Capabilities,
    /// Minimum ash version required (`min_ash_version`). `None` = any version.
    pub min_ash_version: Option<String>,
    /// Enable state. `ash plugin enable/disable` toggles this. Defaults `true`.
    pub enabled: bool,
}

/// What a plugin contributes (each flag gates loading one content type).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginContributions {
    /// Scan `completions/*.at` into the completion provider.
    pub completions: bool,
    /// `source` the `functions.ash` script.
    pub functions: bool,
    /// Scan `smart/<cmd>/` into the SmartCommand loader.
    pub smart: bool,
    /// Merge `config.at` into the main config (v1: placeholder).
    pub config: bool,
}

/// Declared capabilities — what the plugin's code is allowed to do. Used only
/// for the first-load warning (v1); not enforced. `is_empty()` means the plugin
/// declares no special capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// Reads from the filesystem.
    pub reads_fs: bool,
    /// Writes to the filesystem.
    pub writes_fs: bool,
    /// Spawns child processes.
    pub spawns_process: bool,
    /// Makes network calls.
    pub uses_network: bool,
}

impl Capabilities {
    /// True when no capability is declared (no warning will be shown).
    pub fn is_empty(&self) -> bool {
        !self.reads_fs && !self.writes_fs && !self.spawns_process && !self.uses_network
    }
}

/// Errors that can occur while parsing a `plugin.at` manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginError {
    /// The `name` field is missing.
    MissingName,
    /// The `version` field is missing.
    MissingVersion,
    /// The file is structurally malformed (no `plugin { }` block, bad value…).
    InvalidFormat(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::MissingName => write!(f, "plugin.at: missing 'name' field"),
            PluginError::MissingVersion => write!(f, "plugin.at: missing 'version' field"),
            PluginError::InvalidFormat(msg) => write!(f, "plugin.at: {}", msg),
        }
    }
}

impl std::error::Error for PluginError {}

impl PluginManifest {
    /// Build a minimal valid manifest (tests / programmatic construction).
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            author: None,
            description: None,
            homepage: None,
            contributions: PluginContributions::default(),
            capabilities: Capabilities::default(),
            min_ash_version: None,
            enabled: true,
        }
    }

    /// Serialize back to the `plugin.at` text form (round-trip with
    /// `parse_plugin_manifest`). Used by `ash plugin enable/disable` to rewrite
    /// the manifest after toggling `enabled`.
    pub fn to_manifest_text(&self) -> String {
        let mut out = String::new();
        out.push_str("plugin {\n");
        out.push_str(&format!(
            "    name        : \"{}\"\n",
            escape_string(&self.name)
        ));
        out.push_str(&format!(
            "    version     : \"{}\"\n",
            escape_string(&self.version)
        ));
        if let Some(a) = &self.author {
            out.push_str(&format!("    author      : \"{}\"\n", escape_string(a)));
        }
        if let Some(d) = &self.description {
            out.push_str(&format!("    description : \"{}\"\n", escape_string(d)));
        }
        if let Some(h) = &self.homepage {
            out.push_str(&format!("    homepage    : \"{}\"\n", escape_string(h)));
        }
        let c = &self.contributions;
        if c.completions || c.functions || c.smart || c.config {
            out.push_str("    contributions : {\n");
            out.push_str(&format!("        completions : {}\n", c.completions));
            out.push_str(&format!("        functions   : {}\n", c.functions));
            out.push_str(&format!("        smart       : {}\n", c.smart));
            out.push_str(&format!("        config      : {}\n", c.config));
            out.push_str("    }\n");
        }
        let cap = &self.capabilities;
        if !cap.is_empty() {
            out.push_str("    capabilities : {\n");
            out.push_str(&format!("        reads_fs       : {}\n", cap.reads_fs));
            out.push_str(&format!("        writes_fs      : {}\n", cap.writes_fs));
            out.push_str(&format!(
                "        spawns_process : {}\n",
                cap.spawns_process
            ));
            out.push_str(&format!("        uses_network   : {}\n", cap.uses_network));
            out.push_str("    }\n");
        }
        if let Some(m) = &self.min_ash_version {
            out.push_str(&format!("    min_ash_version : \"{}\"\n", escape_string(m)));
        }
        out.push_str(&format!("    enabled : {}\n", self.enabled));
        out.push_str("}\n");
        out
    }
}

/// Parse a `plugin.at` file's contents into a [`PluginManifest`].
///
/// Returns `Err` if the file has no `plugin { }` block, a bad value, or is
/// missing the required `name` / `version` fields.
pub fn parse_plugin_manifest(content: &str) -> Result<PluginManifest, PluginError> {
    let (key, inner) = parse_block(content)?;
    if key != "plugin" {
        return Err(PluginError::InvalidFormat(format!(
            "expected a 'plugin {{ }}' block, found '{}'",
            key
        )));
    }

    let mut manifest = PluginManifest::new(String::new(), String::new());

    // `inner` is the `(key, value_or_block)` entries of the plugin block.
    for entry in inner {
        match entry {
            // A nested block (`contributions {}` / `capabilities {}`): recurse
            // into its entries with a dotted prefix.
            BlockEntry::Nested(prefix, children) => {
                for child in children {
                    if let BlockEntry::Field(k, v) = child {
                        let dotted_key = format!("{}.{}", prefix, k);
                        apply_field(&mut manifest, &dotted_key, &v)?;
                    }
                    // Nested-within-nested entries are ignored (plugin.at has
                    // only one level of nesting).
                }
            }
            // A flat field (`name : "x"`).
            BlockEntry::Field(key, value) => apply_field(&mut manifest, &key, &value)?,
        }
    }

    if manifest.name.is_empty() {
        return Err(PluginError::MissingName);
    }
    if manifest.version.is_empty() {
        return Err(PluginError::MissingVersion);
    }
    Ok(manifest)
}

/// Apply one `key : value` entry to the manifest (string or bool interpretation).
fn apply_field(manifest: &mut PluginManifest, key: &str, value: &str) -> Result<(), PluginError> {
    match key {
        "name" => manifest.name = parse_string_value(value)?,
        "version" => manifest.version = parse_string_value(value)?,
        "author" => manifest.author = Some(parse_string_value(value)?),
        "description" => manifest.description = Some(parse_string_value(value)?),
        "homepage" => manifest.homepage = Some(parse_string_value(value)?),
        "min_ash_version" => manifest.min_ash_version = Some(parse_string_value(value)?),
        "enabled" => manifest.enabled = parse_bool_value(value)?,
        "contributions.completions" => {
            manifest.contributions.completions = parse_bool_value(value)?
        }
        "contributions.functions" => manifest.contributions.functions = parse_bool_value(value)?,
        "contributions.smart" => manifest.contributions.smart = parse_bool_value(value)?,
        "contributions.config" => manifest.contributions.config = parse_bool_value(value)?,
        "capabilities.reads_fs" => manifest.capabilities.reads_fs = parse_bool_value(value)?,
        "capabilities.writes_fs" => manifest.capabilities.writes_fs = parse_bool_value(value)?,
        "capabilities.spawns_process" => {
            manifest.capabilities.spawns_process = parse_bool_value(value)?
        }
        "capabilities.uses_network" => {
            manifest.capabilities.uses_network = parse_bool_value(value)?
        }
        _ => { /* ignore unknown fields for forward-compat */ }
    }
    Ok(())
}

/// One entry inside a block: either `key : value` or a nested `key { ... }`.
#[derive(Debug, Clone)]
enum BlockEntry {
    /// `key : value` (value is raw, caller interprets string/bool).
    Field(String, String),
    /// A nested block: `key { children }`.
    Nested(String, Vec<BlockEntry>),
}

/// Parse a `keyword { ... }` block from `content`. Returns `(keyword, entries)`.
/// Whitespace and `//` comments are skipped. Handles one level of nesting
/// (`plugin { contributions { ... } }`).
fn parse_block(content: &str) -> Result<(String, Vec<BlockEntry>), PluginError> {
    let trimmed = content.trim();
    // Read the leading keyword (e.g. `plugin`).
    let mut chars = trimmed.char_indices().peekable();
    let kw_start = 0usize;
    while let Some(&(i, c)) = chars.peek() {
        if c.is_alphanumeric() || c == '_' {
            chars.next();
            let _ = i;
        } else {
            break;
        }
    }
    let kw_end = chars.peek().map(|(i, _)| *i).unwrap_or(trimmed.len());
    let keyword: String = trimmed[kw_start..kw_end].to_string();
    if keyword.is_empty() {
        return Err(PluginError::InvalidFormat(
            "expected a block keyword".to_string(),
        ));
    }
    // Skip whitespace, expect '{'.
    skip_ws_comments(&mut chars, trimmed);
    match chars.next() {
        Some((_, '{')) => {}
        _ => {
            return Err(PluginError::InvalidFormat(format!(
                "expected '{{' after '{}'",
                keyword
            )))
        }
    }
    let entries = parse_block_body(trimmed, &mut chars)?;
    Ok((keyword, entries))
}

/// Parse block entries until the matching `}`. `chars` is positioned just past
/// the opening `{`.
fn parse_block_body(
    src: &str,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<Vec<BlockEntry>, PluginError> {
    let mut entries = Vec::new();
    loop {
        skip_ws_comments(chars, src);
        match chars.peek() {
            None => {
                return Err(PluginError::InvalidFormat(
                    "expected '}' to close block".to_string(),
                ))
            }
            Some((_, '}')) => {
                chars.next();
                return Ok(entries);
            }
            // Optional comma separator between fields/blocks. Both the
            // multi-line (one field per line, no comma) and the single-line
            // comma-separated forms (`{ a : true, b : true }`) are accepted.
            Some(&(_, ',')) => {
                chars.next(); // consume the comma
                continue;
            }
            Some(&(i, c)) if c.is_alphabetic() || c == '_' => {
                // Read an identifier (field key or nested-block keyword).
                let start = i;
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                let ident_end = chars.peek().map(|(i, _)| *i).unwrap_or(src.len());
                let ident: String = src[start..ident_end].to_string();
                skip_ws_comments(chars, src);
                match chars.peek() {
                    // Nested block: `ident { ... }`.
                    Some((_, '{')) => {
                        chars.next(); // consume '{'
                        let children = parse_block_body(src, chars)?;
                        entries.push(BlockEntry::Nested(ident, children));
                    }
                    // Field: `ident : value`, or `ident : { ... }` (nested block
                    // written with the `:` separator, the canonical plugin.at
                    // form for `contributions`/`capabilities`).
                    Some(&(_, ':')) => {
                        chars.next(); // consume ':'
                        skip_ws_comments(chars, src);
                        if matches!(chars.peek(), Some(&(_, '{'))) {
                            chars.next(); // consume '{'
                            let children = parse_block_body(src, chars)?;
                            entries.push(BlockEntry::Nested(ident, children));
                        } else {
                            let value = read_value(src, chars)?;
                            entries.push(BlockEntry::Field(ident, value));
                        }
                    }
                    _ => {
                        return Err(PluginError::InvalidFormat(format!(
                            "expected ':' or '{{' after '{}'",
                            ident
                        )))
                    }
                }
            }
            Some(&(i, c)) => {
                return Err(PluginError::InvalidFormat(format!(
                    "unexpected '{}' at offset {}",
                    c, i
                )))
            }
        }
    }
}

/// Read a value up to the end of its line, stopping early at an unquoted `}` or
/// `,`. Plugin.at field values are single-line (a quoted string or a bool), so:
/// - a `}` marks the end of a block on the same line (`{ a : true }`)
/// - a `,` separates fields on the same line (`{ a : true, b : true }`)
/// Neither must be swallowed into the value.
fn read_value(
    src: &str,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<String, PluginError> {
    skip_inline_ws(chars, src);
    let mut value = String::new();
    let mut in_string = false;
    while let Some(&(_, c)) = chars.peek() {
        if c == '\n' {
            break;
        }
        // Stop at an unquoted `}` (block close) or `,` (field separator).
        if (c == '}' || c == ',') && !in_string {
            break;
        }
        // Track string state so a `}`/`,` inside a quoted value isn't treated as
        // a block terminator / separator. (Plugin values rarely contain these,
        // but be safe.)
        if c == '"' {
            in_string = !in_string;
        }
        value.push(c);
        chars.next();
    }
    Ok(value.trim().to_string())
}

/// Skip whitespace and `//` comments (including newlines).
fn skip_ws_comments(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, src: &str) {
    loop {
        match chars.peek() {
            Some(&(_, c)) if c.is_whitespace() => {
                chars.next();
            }
            Some(&(i, '/')) if src[i..].starts_with("//") => {
                // Skip to end of line.
                while let Some(&(_, c)) = chars.peek() {
                    chars.next();
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => break,
        }
    }
}

/// Skip inline whitespace (spaces/tabs) only — not newlines or comments.
fn skip_inline_ws(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, _src: &str) {
    while let Some(&(_, c)) = chars.peek() {
        if c == ' ' || c == '\t' {
            chars.next();
        } else {
            break;
        }
    }
}

/// A value that is a quoted string → its content.
fn parse_string_value(raw: &str) -> Result<String, PluginError> {
    parse_leading_quoted_string(raw)
        .ok_or_else(|| PluginError::InvalidFormat(format!("expected a quoted string, got: {raw}")))
}

/// A value that is `true` / `false`.
fn parse_bool_value(raw: &str) -> Result<bool, PluginError> {
    match raw.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(PluginError::InvalidFormat(format!(
            "expected true/false, got: {other}"
        ))),
    }
}

/// If `s` starts with `"..."`, return the content. Handles `\"` and `\\`.
fn parse_leading_quoted_string(s: &str) -> Option<String> {
    let bytes: Vec<char> = s.chars().collect();
    if bytes.first() != Some(&'"') {
        return None;
    }
    let mut value = String::new();
    let mut i = 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                '"' => value.push('"'),
                '\\' => value.push('\\'),
                'n' => value.push('\n'),
                't' => value.push('\t'),
                other => value.push(other),
            }
            i += 2;
            continue;
        }
        if c == '"' {
            return Some(value);
        }
        value.push(c);
        i += 1;
    }
    None // unterminated
}

/// Escape a string for embedding in a quoted `.at` value.
fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"plugin {
    name        : "my-git-extras"
    version     : "0.1.0"
    author      : "zhaopuming"
    description : "Extra git SmartCommands and completion enhancements"
    homepage    : "https://github.com/zhaopuming/ash-git-extras"
    contributions : {
        completions : true
        functions   : true
        smart       : true
        config      : false
    }
    capabilities : {
        reads_fs       : true
        writes_fs      : true
        spawns_process : true
        uses_network   : false
    }
    min_ash_version : "0.5.0"
    enabled : true
}
"#;

    #[test]
    fn parse_full_manifest() {
        let m = parse_plugin_manifest(SAMPLE).unwrap();
        assert_eq!(m.name, "my-git-extras");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.author.as_deref(), Some("zhaopuming"));
        assert_eq!(
            m.description.as_deref(),
            Some("Extra git SmartCommands and completion enhancements")
        );
        assert_eq!(
            m.homepage.as_deref(),
            Some("https://github.com/zhaopuming/ash-git-extras")
        );
        assert_eq!(
            m.contributions,
            PluginContributions {
                completions: true,
                functions: true,
                smart: true,
                config: false,
            }
        );
        assert!(!m.capabilities.is_empty());
        assert!(m.capabilities.reads_fs);
        assert!(m.capabilities.writes_fs);
        assert!(m.capabilities.spawns_process);
        assert!(!m.capabilities.uses_network);
        assert_eq!(m.min_ash_version.as_deref(), Some("0.5.0"));
        assert!(m.enabled);
    }

    #[test]
    fn parse_minimal_manifest_only_required() {
        let content = r#"plugin {
    name    : "hello"
    version : "1.0.0"
}
"#;
        let m = parse_plugin_manifest(content).unwrap();
        assert_eq!(m.name, "hello");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.author, None);
        assert_eq!(m.description, None);
        assert_eq!(m.homepage, None);
        assert_eq!(m.contributions, PluginContributions::default());
        assert!(m.capabilities.is_empty());
        assert_eq!(m.min_ash_version, None);
        assert!(m.enabled, "enabled defaults to true");
    }

    #[test]
    fn parse_disabled_explicit_false() {
        let content = r#"plugin {
    name    : "off"
    version : "0.1.0"
    enabled : false
}
"#;
        let m = parse_plugin_manifest(content).unwrap();
        assert!(!m.enabled);
    }

    #[test]
    fn parse_contributions_partial() {
        let content = r#"plugin {
    name    : "p"
    version : "0.1.0"
    contributions : {
        completions : true
    }
}
"#;
        let m = parse_plugin_manifest(content).unwrap();
        assert!(m.contributions.completions);
        assert!(!m.contributions.functions, "unspecified defaults false");
        assert!(!m.contributions.smart);
        assert!(!m.contributions.config);
    }

    #[test]
    fn parse_capabilities_partial() {
        let content = r#"plugin {
    name    : "p"
    version : "0.1.0"
    capabilities : {
        uses_network : true
    }
}
"#;
        let m = parse_plugin_manifest(content).unwrap();
        assert!(!m.capabilities.is_empty());
        assert!(m.capabilities.uses_network);
        assert!(!m.capabilities.reads_fs);
    }

    #[test]
    fn error_missing_name() {
        let content = r#"plugin {
    version : "0.1.0"
}
"#;
        assert_eq!(
            parse_plugin_manifest(content).unwrap_err(),
            PluginError::MissingName
        );
    }

    #[test]
    fn error_missing_version() {
        let content = r#"plugin {
    name : "p"
}
"#;
        assert_eq!(
            parse_plugin_manifest(content).unwrap_err(),
            PluginError::MissingVersion
        );
    }

    #[test]
    fn error_no_plugin_block() {
        assert!(parse_plugin_manifest("not a plugin file").is_err());
    }

    #[test]
    fn error_missing_open_brace() {
        let content = "plugin name : \"x\" }";
        assert!(parse_plugin_manifest(content).is_err());
    }

    #[test]
    fn error_unterminated_block() {
        let content = "plugin { name : \"x\"";
        assert!(parse_plugin_manifest(content).is_err());
    }

    #[test]
    fn error_bad_bool_value() {
        let content = r#"plugin {
    name    : "p"
    version : "0.1.0"
    enabled : maybe
}
"#;
        assert!(parse_plugin_manifest(content).is_err());
    }

    #[test]
    fn error_bad_string_value() {
        let content = r#"plugin {
    name    : p
    version : "0.1.0"
}
"#;
        assert!(parse_plugin_manifest(content).is_err());
    }

    #[test]
    fn ignores_unknown_fields() {
        let content = r#"plugin {
    name    : "p"
    version : "0.1.0"
    future  : "ignored"
}
"#;
        let m = parse_plugin_manifest(content).unwrap();
        assert_eq!(m.name, "p");
    }

    #[test]
    fn ignores_comments() {
        let content = r#"plugin {
    // a comment
    name    : "p"
    version : "0.1.0"
}
"#;
        let m = parse_plugin_manifest(content).unwrap();
        assert_eq!(m.name, "p");
    }

    #[test]
    fn parse_escapes_in_strings() {
        let content = r#"plugin {
    name        : "p"
    version     : "0.1.0"
    description : "say \"hi\" and \\path"
}
"#;
        let m = parse_plugin_manifest(content).unwrap();
        assert_eq!(m.description.as_deref(), Some(r#"say "hi" and \path"#));
    }

    #[test]
    fn capabilities_is_empty_when_all_false() {
        assert!(Capabilities::default().is_empty());
        assert!(!Capabilities {
            reads_fs: true,
            ..Default::default()
        }
        .is_empty());
    }

    #[test]
    fn round_trip_full_manifest() {
        let original = parse_plugin_manifest(SAMPLE).unwrap();
        let text = original.to_manifest_text();
        let reparsed = parse_plugin_manifest(&text).unwrap();
        assert_eq!(original, reparsed, "round-trip must preserve all fields");
    }

    #[test]
    fn round_trip_minimal_manifest() {
        let original = PluginManifest::new("hello", "1.0.0");
        let text = original.to_manifest_text();
        let reparsed = parse_plugin_manifest(&text).unwrap();
        assert_eq!(original, reparsed);
    }

    #[test]
    fn round_trip_disabled() {
        let mut original = PluginManifest::new("off", "0.2.0");
        original.enabled = false;
        let text = original.to_manifest_text();
        let reparsed = parse_plugin_manifest(&text).unwrap();
        assert_eq!(original, reparsed);
        assert!(!reparsed.enabled);
    }

    #[test]
    fn nested_block_with_trailing_whitespace() {
        let content = "plugin {\n    name    :   \"p\"\n    version :   \"0.1.0\"\n    contributions :   {\n        smart : true\n    }\n}\n";
        let m = parse_plugin_manifest(content).unwrap();
        assert!(m.contributions.smart);
    }

    #[test]
    fn plugin_error_display() {
        assert!(format!("{}", PluginError::MissingName).contains("name"));
        assert!(format!("{}", PluginError::MissingVersion).contains("version"));
        assert!(format!("{}", PluginError::InvalidFormat("x".into())).contains("x"));
    }

    /// Comma-separated fields on a single line parse correctly (the natural
    /// `.at` form, e.g. `{ completions : true, functions : true }`). Previously
    /// a parser bug rejected this; it's now supported alongside the one-field-
    /// per-line form.
    #[test]
    fn single_line_comma_separated_block_fields_parse() {
        let content = r#"plugin {
    name    : "p"
    version : "0.1.0"
    contributions : { completions : true, functions : true }
    capabilities : { reads_fs : true, uses_network : false }
}
"#;
        let m = parse_plugin_manifest(content).unwrap();
        assert!(m.contributions.completions);
        assert!(m.contributions.functions);
        assert!(m.capabilities.reads_fs);
        assert!(!m.capabilities.uses_network);
    }

    /// Three fields on one line, all comma-separated.
    #[test]
    fn single_line_three_comma_separated_fields() {
        let content = r#"plugin {
    name    : "p"
    version : "0.1.0"
    capabilities : { reads_fs : true, writes_fs : true, uses_network : true }
}
"#;
        let m = parse_plugin_manifest(content).unwrap();
        assert!(m.capabilities.reads_fs);
        assert!(m.capabilities.writes_fs);
        assert!(m.capabilities.uses_network);
    }

    /// Two nested fields each on their own line parse correctly (canonical form).
    #[test]
    fn multi_line_block_fields_parse() {
        let content = "plugin {\n    name    : \"p\"\n    version : \"0.1.0\"\n    contributions : {\n        completions : true\n        functions   : true\n    }\n}\n";
        let m = parse_plugin_manifest(content).unwrap();
        assert!(m.contributions.completions);
        assert!(m.contributions.functions);
    }
}
