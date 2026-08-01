//! Data module for shell
//!
//! Provides structured data types and table rendering.
//!
//! Pure logic types (ShellValue, AshFileEntry, etc.) live in `core::data`.
//! Terminal-dependent rendering (Table with ANSI styles) stays here.

// Table rendering uses nu-ansi-term — stays in frontend layer, but the type
// itself (Table/Column/Align/FileEntry) is consumed by cmd/fs (used by ls/builtin).
// Plan 030 M0: the module stays compiled; nu-ansi-term styling inside it is
// gated (see table.rs).
pub mod table;

// Re-export core data types for backward compatibility
pub use crate::core::data::convert;
pub use crate::core::data::value;
pub use crate::core::data::types;

pub use table::{Table, Column, Align, FileEntry};
pub use value::ShellValue;
pub use types::{
    AshFileEntry, AshProcessEntry, AshDiskEntry,
    AshCpuInfo, AshMemoryInfo, FileType,
};
pub use convert::{metadata_to_entry, file_entry_to_value, file_entries_to_value};
