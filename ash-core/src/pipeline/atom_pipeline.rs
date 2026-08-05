//! AtomPipeline — the typed pipeline data enum
//!
//! Replaces `PipelineData(Value|Text)` with a richer enum that carries
//! semantic type information. Every variant preserves the ability to
//! convert back to plain text for display or legacy compatibility.

use auto_val::Value;
use super::atom::{Atom, AtomType};
use super::atom_stream::AtomStream;
use super::external_stream::ExternalStream;

/// Typed pipeline data flowing between shell commands.
///
/// | Variant | Purpose |
/// |---------|---------|
/// | `Atom` | Single typed value (most common) |
/// | `Stream` | Lazy iteration over in-memory Atoms |
/// | `ExternalStream` | Streaming output from an external child process |
/// | `Text` | Plain text (external commands, legacy) |
/// | `Empty` | No output (side-effect commands) |
pub enum AtomPipeline {
    /// Single typed value
    Atom(Atom),
    /// Lazy stream of typed values (in-memory cursor)
    Stream(AtomStream),
    /// Streaming output from an external child process (I/O backed)
    ExternalStream(ExternalStream),
    /// Plain text (external commands, legacy compatibility)
    Text(String),
    /// Plan 042 M6 (B1): syntax-highlighted code spans. Carries structured
    /// color data (RGB + bold/italic) so frontends render without ANSI/HTML.
    Code { spans: Vec<Vec<crate::renderer::CodeSpan>>, language: String },
    /// No data
    Empty,
}

impl AtomPipeline {
    // ── Constructors ─────────────────────────────────────

    /// Create an AtomPipeline from a Value and explicit type tag.
    pub fn atom(value: Value, atom_type: AtomType) -> Self {
        AtomPipeline::Atom(Atom::new(value, atom_type))
    }

    /// Create a FileList pipeline.
    pub fn file_list(value: Value) -> Self {
        AtomPipeline::Atom(Atom::file_list(value))
    }

    /// Create a ProcessList pipeline.
    pub fn process_list(value: Value) -> Self {
        AtomPipeline::Atom(Atom::process_list(value))
    }

    /// Create a plain-text pipeline.
    pub fn text(s: impl Into<String>) -> Self {
        AtomPipeline::Text(s.into())
    }

    /// Create an empty pipeline (no data).
    pub fn empty() -> Self {
        AtomPipeline::Empty
    }

    /// Create from an existing Atom.
    pub fn from_atom(atom: Atom) -> Self {
        AtomPipeline::Atom(atom)
    }

    /// Create from a stream of Atoms.
    pub fn from_stream(stream: AtomStream) -> Self {
        AtomPipeline::Stream(stream)
    }

    // ── Query methods ────────────────────────────────────

    /// Get a reference to the inner Atom, if this is the Atom variant.
    pub fn as_atom(&self) -> Option<&Atom> {
        match self {
            AtomPipeline::Atom(a) => Some(a),
            _ => None,
        }
    }

    /// Get the type tag (Nothing for Text/Empty/Stream).
    pub fn atom_type(&self) -> AtomType {
        match self {
            AtomPipeline::Atom(a) => a.atom_type(),
            AtomPipeline::Stream(_) => AtomType::Nothing, // streams don't have a single type
            AtomPipeline::ExternalStream(_) => AtomType::Text, // external output is text
            AtomPipeline::Text(_) => AtomType::Text,
            AtomPipeline::Code { .. } => AtomType::Text, // code is text with color metadata
            AtomPipeline::Empty => AtomType::Nothing,
        }
    }

    /// Check if this is an Atom variant.
    pub fn is_atom(&self) -> bool {
        matches!(self, AtomPipeline::Atom(_))
    }

    /// Check if this is a Stream variant.
    pub fn is_stream(&self) -> bool {
        matches!(self, AtomPipeline::Stream(_))
    }

    /// Check if this is plain text.
    pub fn is_text(&self) -> bool {
        matches!(self, AtomPipeline::Text(_))
    }

    /// Check if this is an ExternalStream variant.
    pub fn is_external_stream(&self) -> bool {
        matches!(self, AtomPipeline::ExternalStream(_))
    }

    /// Extract the ExternalStream, if this is that variant.
    /// Returns `None` for all other variants.
    pub fn into_external_stream(self) -> Option<ExternalStream> {
        match self {
            AtomPipeline::ExternalStream(es) => Some(es),
            _ => None,
        }
    }

    /// Check if this is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            AtomPipeline::Empty => true,
            AtomPipeline::Atom(a) => a.is_empty(),
            AtomPipeline::Text(s) => s.is_empty(),
            AtomPipeline::Stream(s) => s.total_count() == 0,
            AtomPipeline::ExternalStream(_) => false, // stream not yet read
            AtomPipeline::Code { spans, .. } => spans.iter().all(|line| line.is_empty()),
        }
    }

    /// Check if this carries structured (typed) data.
    pub fn is_structured(&self) -> bool {
        match self {
            AtomPipeline::Atom(a) => a.is_structured(),
            AtomPipeline::Stream(_) => true,
            AtomPipeline::Code { .. } => true, // Plan 042 M6: Code is structured
            _ => false,
        }
    }

    // ── Conversion ───────────────────────────────────────

    /// Convert to display text (consumes self).
    pub fn into_text(self) -> String {
        match self {
            AtomPipeline::Atom(a) => a.into_text(),
            AtomPipeline::Stream(mut s) => {
                let items: Vec<String> = s.collect_remaining().iter().map(|a| a.as_text()).collect();
                items.join("\n")
            }
            AtomPipeline::ExternalStream(es) => {
                es.read_all().unwrap_or_default()
            }
            AtomPipeline::Text(s) => s,
            AtomPipeline::Code { spans, .. } => {
                spans.iter().map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n")
            }
            AtomPipeline::Empty => String::new(),
        }
    }

    /// Get display text without consuming.
    pub fn as_text(&self) -> String {
        match self {
            AtomPipeline::Atom(a) => a.as_text(),
            AtomPipeline::Stream(s) => {
                let items: Vec<String> = s.items.iter().map(|a: &Atom| a.as_text()).collect();
                items.join("\n")
            }
            // ExternalStream cannot be read without consuming; indicate pending
            AtomPipeline::ExternalStream(_) => "<external stream>".to_string(),
            AtomPipeline::Text(s) => s.clone(),
            AtomPipeline::Code { .. } => "<code>".to_string(),
            AtomPipeline::Empty => String::new(),
        }
    }

    /// Extract the inner Value, if this is an Atom variant.
    pub fn into_value(self) -> Option<Value> {
        match self {
            AtomPipeline::Atom(a) => Some(a.value),
            AtomPipeline::Stream(s) => Some(s.into_atom_list().value),
            AtomPipeline::ExternalStream(es) => {
                Some(Value::str(&es.read_all().unwrap_or_default()))
            }
            AtomPipeline::Text(s) => Some(Value::str(&s)),
            AtomPipeline::Code { spans, .. } => {
                let text = spans.iter()
                    .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(Value::str(&text))
            }
            AtomPipeline::Empty => None,
        }
    }

    /// Extract the `Value` a structured-pipeline DSL stage should operate on.
    ///
    /// This is the input source for `Shell::execute_pipeline_with_auto`'s DSL
    /// dispatch (filter/sort/count/uniq/...). Critically, unlike `into_value`
    /// (which returns the whole external output as a single `Value::Str`), this
    /// splits an `ExternalStream` into its **lines as a `Value::Array`** so that
    /// row-oriented operators (`count`, `sort`, `uniq`, `reverse`, ...) see the
    /// rows instead of silently dropping them to an empty array.
    ///
    /// Plan 031 M0.1 — fixes the silent-data-loss bug where `printf '...' | count`
    /// returned `0` because the external command's stream was discarded.
    pub fn into_dsl_input(self) -> Value {
        use auto_val::Array;
        match self {
            AtomPipeline::Atom(a) => a.value,
            AtomPipeline::Stream(s) => s.into_atom_list().value,
            AtomPipeline::ExternalStream(es) => {
                // Split the streamed output into one `Value` per line, matching
                // how a list of rows would flow through `operators::apply`.
                let text = es.read_all().unwrap_or_default();
                Value::str_array(text.lines().map(|l| l.to_string()).collect::<Vec<_>>())
            }
            AtomPipeline::Text(s) => Value::str(&s),
            AtomPipeline::Code { spans, .. } => {
                let lines: Vec<String> = spans.iter()
                    .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
                    .collect();
                Value::str_array(lines)
            }
            AtomPipeline::Empty => Value::Array(Array::new()),
        }
    }

    /// Collect a Stream variant into an Atom (no-op for other variants).
    pub fn collect_stream(self) -> Self {
        match self {
            AtomPipeline::Stream(s) => AtomPipeline::Atom(s.into_atom_list()),
            other => other,
        }
    }

    // ── Streaming line iterator ──────────────────────────

    /// Consume the pipeline and return an iterator over its lines.
    ///
    /// For `ExternalStream`, reads lines incrementally (no buffering).
    /// For `Text`, splits the string by newlines.
    /// For `Atom`/`Stream`, converts to text first, then splits.
    /// For `Empty`, yields nothing.
    pub fn into_lines(self) -> Box<dyn Iterator<Item = String>> {
        match self {
            AtomPipeline::ExternalStream(es) => {
                Box::new(es.lines().filter_map(|r| r.ok()))
            }
            AtomPipeline::Text(s) => {
                let lines: Vec<String> = s.lines().map(|l| l.to_string()).collect();
                Box::new(lines.into_iter())
            }
            AtomPipeline::Atom(a) => {
                let text = a.into_text();
                let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
                Box::new(lines.into_iter())
            }
            AtomPipeline::Stream(mut s) => {
                let items: Vec<String> = s
                    .collect_remaining()
                    .iter()
                    .map(|a| a.as_text())
                    .collect();
                Box::new(items.into_iter())
            }
            AtomPipeline::Code { spans, .. } => {
                let lines: Vec<String> = spans.into_iter()
                    .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
                    .collect();
                Box::new(lines.into_iter())
            }
            AtomPipeline::Empty => Box::new(std::iter::empty()),
        }
    }

    // ── Batom binary serialization ─────────────────────

    /// Serialize this pipeline to Batom binary format.
    pub fn to_batom(&self) -> Result<Vec<u8>, super::batom::BatomError> {
        super::batom::encode_pipeline(self)
    }

    /// Deserialize a Batom binary blob into an AtomPipeline.
    pub fn from_batom(data: &[u8]) -> Result<Self, super::batom::BatomError> {
        super::batom::decode_pipeline(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atom_pipeline_atom() {
        let p = AtomPipeline::atom(Value::Int(42), AtomType::CountResult);
        assert!(p.is_atom());
        assert!(p.is_structured());
        assert!(!p.is_empty());
        assert_eq!(p.atom_type(), AtomType::CountResult);
    }

    #[test]
    fn test_atom_pipeline_text() {
        let p = AtomPipeline::text("hello");
        assert!(p.is_text());
        assert!(!p.is_structured());
        assert_eq!(p.as_text(), "hello");
    }

    #[test]
    fn test_atom_pipeline_empty() {
        let p = AtomPipeline::empty();
        assert!(p.is_empty());
        assert_eq!(p.as_text(), "");
    }

    #[test]
    fn test_atom_pipeline_file_list() {
        let p = AtomPipeline::file_list(Value::Void);
        assert_eq!(p.atom_type(), AtomType::FileList);
        assert!(p.is_structured());
    }

    #[test]
    fn test_atom_pipeline_from_atom() {
        let atom = Atom::path("/tmp");
        let p = AtomPipeline::from_atom(atom);
        assert_eq!(p.atom_type(), AtomType::Path);
    }

    #[test]
    fn test_atom_pipeline_into_text() {
        let p = AtomPipeline::text("world");
        assert_eq!(p.into_text(), "world");
    }

    #[test]
    fn test_atom_pipeline_into_value() {
        let p = AtomPipeline::atom(Value::Int(99), AtomType::Nothing);
        let v = p.into_value();
        assert!(v.is_some());
    }

    #[test]
    fn test_atom_pipeline_empty_into_value() {
        let p = AtomPipeline::empty();
        assert!(p.into_value().is_none());
    }

    // ── Plan 031 M0.1: into_dsl_input (Stream bug fix) ──
    //
    // `into_dsl_input` extracts the `Value` a DSL stage (filter/sort/count/...)
    // should operate on. The critical regression it fixes: an `ExternalStream`
    // (output of an external command like `git ls-files | sort .field`) used to
    // be silently dropped to an empty array by the DSL dispatch in
    // `Shell::execute_pipeline_with_auto`, losing all data. It must instead be
    // split into lines as a `Value::Array` so operators (count/sort/uniq/...)
    // see the rows.

    #[test]
    fn dsl_input_external_stream_becomes_line_array() {
        // `sort` is available cross-platform (System32\sort.exe on Windows,
        // coreutils on Unix) and already used by external_stream.rs tests.
        // It reads N lines from stdin and emits N sorted lines, giving a
        // reliable multi-line ExternalStream without shell-specific commands.
        use std::process::{Command, Stdio};
        let child = Command::new("sort")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("sort should spawn");
        let es = ExternalStream::new_with_stdin(child, "cherry\napple\nbanana\n".to_string());
        let p = AtomPipeline::ExternalStream(es);

        let value = p.into_dsl_input();

        // Must be an Array whose length is the number of emitted lines — NOT
        // an empty array (the bug) nor a single Value::Str.
        match value {
            Value::Array(a) => {
                assert_eq!(a.len(), 3, "external stream lines must be preserved");
            }
            other => panic!("expected Value::Array, got {other:?}"),
        }
    }

    #[test]
    fn dsl_input_atom_passes_value_through() {
        let p = AtomPipeline::atom(Value::Int(42), AtomType::Nothing);
        let value = p.into_dsl_input();
        assert!(matches!(value, Value::Int(42)));
    }

    #[test]
    fn dsl_input_text_stays_string() {
        let p = AtomPipeline::text("hello");
        let value = p.into_dsl_input();
        assert!(matches!(value, Value::Str(_)));
    }

    #[test]
    fn dsl_input_empty_yields_empty_array() {
        let p = AtomPipeline::empty();
        let value = p.into_dsl_input();
        match value {
            Value::Array(a) => assert_eq!(a.len(), 0),
            other => panic!("expected empty Value::Array, got {other:?}"),
        }
    }
}
