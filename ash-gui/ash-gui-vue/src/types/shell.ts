/**
 * TypeScript mirror of ash-core's `RenderedOutput` family (renderer.rs).
 *
 * These are produced by Rust with `#[derive(serde::Serialize)]` and sent to the
 * frontend as JSON over Tauri events/commands. The discriminant keys (e.g.
 * `"Table"`, `"Record"`) come from serde's default externally-tagged enum
 * representation.
 *
 * Keep these in sync with:
 *   ash-core/src/renderer.rs     (RenderedOutput, RenderedCell, CellTag, ...)
 *   ash-core/src/pipeline/atom.rs (AtomType)
 */

/** Semantic type tag — the `atom_type` field on Table/Record. See AtomType in atom.rs. */
export type AtomType =
  | 'FileEntry' | 'FileList'
  | 'ProcessEntry' | 'ProcessList'
  | 'DiskEntry' | 'CpuInfo' | 'MemoryInfo' | 'SystemInfo'
  | 'MatchList' | 'CountResult'
  | 'Table' | 'Record'
  | 'Text' | 'Path'
  | 'BuildResult' | 'RunResult'
  | 'HelpInfo' | 'Nothing'

/** Sub-kind for FileName cells. */
export type FileNameKind = 'Dir' | 'CodeAtRs' | 'Executable' | 'Config' | 'Plain'

/** Semantic tag for a cell — what the cell *is*, not how to color it. */
export type CellTag =
  | { FileName: FileNameKind }
  | 'Dir'
  | 'Permission'
  | 'Plain'

/** One cell of a rendered table. */
export type RenderedCell =
  | { Text: string }
  | { Tagged: { text: string; tag: CellTag } }

/** A single record's fields (key → cell). */
export type RecordField = [string, RenderedCell]

/** The frontend-agnostic output description. Discriminated by the variant key. */
export type RenderedOutput =
  | { Table: { columns: string[]; rows: RenderedCell[][]; atom_type: AtomType } }
  | { Record: { fields: RecordField[]; atom_type: AtomType } }
  | { Text: string }
  | 'Empty'
  | { Error: { message: string; kind: RenderErrorKind } }

export type RenderErrorKind = 'NotFound' | 'PermissionDenied' | 'NonzeroExit' | 'Other'

// ── Block lifecycle (frontend-side; the Rust worker only emits results) ──────

/** Status of a block currently shown in the UI. */
export type BlockStatus =
  | { kind: 'Running' }
  | { kind: 'Success' }
  | { kind: 'Failed'; message: string }
  | { kind: 'Cancelled' }

/** One block as tracked by the frontend. */
export interface Block {
  id: number
  command: string
  cwd: string
  status: BlockStatus
  output: RenderedOutput | null
  /**
   * Incremental text streamed from a long external command (Plan 040 M4).
   * Shown while status is Running; the final `command-result` replaces this
   * with the block's `output` (Text).
   */
  streamedText: string
  /** Wall-clock ms the command took (filled when status leaves Running). */
  durationMs: number | null
}

// ── Payloads for the Tauri `command-result` event ────────────────────────────

export interface CommandResultPayload {
  block_id: number
  cwd: string
  /** serde externally-tagged: unit variant → "Success", tuple variant → { Failed: msg }. */
  status:
    | 'Success'
    | { Failed: string }
  output: RenderedOutput
  duration_ms: number
}

/** Payload for the `command-output` streaming event (Plan 040 M4). One chunk of
 * streamed text from a long external command, attributed to a Running block. */
export interface CommandOutputPayload {
  block_id: number
  chunk: string
}

// ── Payloads for boot-time commands ──────────────────────────────────────────

export interface ToolEntry {
  name: string
  description: string
}

export interface SmartCommandEntry {
  name: string
  description: string
}

export interface CommandListResult {
  cwd: string
  home: string
  commands: ToolEntry[]
  smart_commands: SmartCommandEntry[]
}

// ── Plan 041 M7: completion (shared engine, serialized for the frontend) ────

/** One completion candidate from the shared backend engine. Mirrors the Rust
 * `CompletionItem` (shell_worker.rs) — itself a serialization of the core
 * `auto_shell::completions::Completion` type. */
export interface CompletionItem {
  /** What to insert into the input. */
  replacement: string
  /** What to show in the completion menu (may differ from replacement). */
  display: string
  /** Optional one-line description (e.g. "Reverse sort order" for -r). */
  description: string | null
  /** Semantic kind: command / file / flag / directory / variable / ... */
  kind: string
}

// ── Plan 041 M5: prompt context (git branch/status) ─────────────────────────

export interface GitStatusInfo {
  staged: number
  unstaged: number
  untracked: number
  conflicted: number
  ahead: number
  behind: number
}

export interface PromptContext {
  git_branch: string | null
  git_status: GitStatusInfo | null
}
