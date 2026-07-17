//! Plan 028: Map AtomType → snake_case kind string for the response envelope.
//!
//! We deliberately do NOT add Serialize to AtomType itself (to avoid touching
//! existing pipeline code). Instead this module is the single source of truth
//! for the kind label of each AtomType.

use crate::pipeline::AtomType;

/// Return the stable snake_case kind label for an AtomType.
///
/// These strings appear in the response envelope's `data.kind` field and are
/// part of the Agent-facing contract — do NOT rename without bumping
/// schema_version.
pub fn atom_type_to_kind(t: AtomType) -> &'static str {
    match t {
        AtomType::FileEntry => "file_entry",
        AtomType::FileList => "file_list",
        AtomType::ProcessEntry => "process_entry",
        AtomType::ProcessList => "process_list",
        AtomType::DiskEntry => "disk_entry",
        AtomType::CpuInfo => "cpu_info",
        AtomType::MemoryInfo => "memory_info",
        AtomType::SystemInfo => "system_info",
        AtomType::MatchList => "match_list",
        AtomType::CountResult => "count_result",
        AtomType::Table => "table",
        AtomType::Record => "record",
        AtomType::Text => "text",
        AtomType::Path => "path",
        AtomType::BuildResult => "build_result",
        AtomType::RunResult => "run_result",
        AtomType::HelpInfo => "help_info",
        AtomType::Nothing => "empty",
    }
}

/// The AtomType name as it appears in `data.atom_type` (PascalCase, matches
/// the Rust enum variant). Kept distinct from `kind` (snake_case) so Agents
/// can reference either.
pub fn atom_type_name(t: AtomType) -> &'static str {
    match t {
        AtomType::FileEntry => "FileEntry",
        AtomType::FileList => "FileList",
        AtomType::ProcessEntry => "ProcessEntry",
        AtomType::ProcessList => "ProcessList",
        AtomType::DiskEntry => "DiskEntry",
        AtomType::CpuInfo => "CpuInfo",
        AtomType::MemoryInfo => "MemoryInfo",
        AtomType::SystemInfo => "SystemInfo",
        AtomType::MatchList => "MatchList",
        AtomType::CountResult => "CountResult",
        AtomType::Table => "Table",
        AtomType::Record => "Record",
        AtomType::Text => "Text",
        AtomType::Path => "Path",
        AtomType::BuildResult => "BuildResult",
        AtomType::RunResult => "RunResult",
        AtomType::HelpInfo => "HelpInfo",
        AtomType::Nothing => "Nothing",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_atom_types_have_kind_mappings() {
        // Every variant must map to a non-empty snake_case string.
        let all = [
            AtomType::FileEntry,
            AtomType::FileList,
            AtomType::ProcessEntry,
            AtomType::ProcessList,
            AtomType::DiskEntry,
            AtomType::CpuInfo,
            AtomType::MemoryInfo,
            AtomType::SystemInfo,
            AtomType::MatchList,
            AtomType::CountResult,
            AtomType::Table,
            AtomType::Record,
            AtomType::Text,
            AtomType::Path,
            AtomType::BuildResult,
            AtomType::RunResult,
            AtomType::HelpInfo,
            AtomType::Nothing,
        ];
        for t in all {
            let k = atom_type_to_kind(t);
            assert!(!k.is_empty(), "empty kind for {:?}", t);
            assert!(
                !k.chars().any(|c| c.is_uppercase()),
                "kind {:?} has uppercase (must be snake_case): {}",
                t,
                k
            );
        }
    }

    #[test]
    fn kind_is_stable_string() {
        assert_eq!(atom_type_to_kind(AtomType::FileList), "file_list");
        assert_eq!(atom_type_to_kind(AtomType::ProcessList), "process_list");
        assert_eq!(atom_type_to_kind(AtomType::Table), "table");
        assert_eq!(atom_type_to_kind(AtomType::Nothing), "empty");
    }

    #[test]
    fn atom_type_name_is_pascal() {
        assert_eq!(atom_type_name(AtomType::FileList), "FileList");
        assert_eq!(atom_type_name(AtomType::Nothing), "Nothing");
    }
}
