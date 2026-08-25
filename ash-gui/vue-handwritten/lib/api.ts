// Type Definitions

export interface CodeSpan {
    text: string;
    r: number;
    g: number;
    b: number;
    bold: boolean;
    italic: boolean;
}

export interface TaggedCell {
    text: string;
    tag: string;
    kind: string;
}

export interface RenderedCell {
    Text?: string | null;
    Tagged?: TaggedCell | null;
}

export interface TableOutput {
    columns: string[];
    rows: RenderedCell[][];
    atom_type: string;
}

export interface CodeOutput {
    lines: CodeSpan[][];
    language: string;
}

export interface ErrorOutput {
    message: string;
    kind: string;
}

export interface RecordOutput {
    fields: [string, RenderedCell][];
    atom_type: string;
}

export interface AiSuggestionOutput {
    question: string;
    cmd: string;
    notice: string;
    multi: boolean;
    steps: string[];
}

export interface RenderedOutput {
    Table?: TableOutput | null;
    Text?: string | null;
    Code?: CodeOutput | null;
    Error?: ErrorOutput | null;
    Record?: RecordOutput | null;
    AiSuggestion?: AiSuggestionOutput | null;
}

export interface BlockStatus {
    kind: string;
    message: string;
}

export interface Block {
    id: number;
    command: string;
    cwd: string;
    status: BlockStatus;
    output: RenderedOutput;
    streamed_text: string;
    duration_ms: number;
    exit_code: number;
    collapsed: boolean;
    table_sort_col: number;
    table_sort_dir: number;
    table_filter_q: string;
    steps: string[];
    step_styles: string[];
}

export interface GitStatusInfo {
    staged: number;
    unstaged: number;
    untracked: number;
    conflicted: number;
    ahead: number;
    behind: number;
}

export interface PromptContext {
    git_branch: string;
    git_status: GitStatusInfo;
}

export interface CompletionItem {
    replacement: string;
    display: string;
    description: string;
    kind: string;
}

export interface CommandResult {
    block_id: number;
    cwd: string;
    status: string;
    output: RenderedOutput;
    duration_ms: number;
}

export interface ShellEvent {
    event: string;
    block_id: number;
    chunk: string;
    result: CommandResult;
}

export interface JobInfo {
    id: number;
    command: string;
    state: string;
    exit_code: number;
}

export interface ToolEntry {
    name: string;
    description: string;
    usage: string;
}

export interface SmartCommandEntry {
    name: string;
    description: string;
    args: string[];
}

export interface BootSnapshot {
    cwd: string;
    home: string;
    commands: ToolEntry[];
    smart_commands: SmartCommandEntry[];
}

// API Functions

export async function command_list(): Promise<BootSnapshot> {
    const response = await fetch(`/api/command_list`, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json' },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

export async function history(): Promise<string[]> {
    const response = await fetch(`/api/history`, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json' },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

export async function complete(line: string, cursor: number): Promise<CompletionItem[]> {
    const response = await fetch(`/api/complete`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ line, cursor }),
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

export async function prompt_context(): Promise<PromptContext> {
    const response = await fetch(`/api/prompt_context`, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json' },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

export async function run_command(block_id: number, cmd: string, cwd: string): Promise<void> {
    const response = await fetch(`/api/run_command`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ block_id, cmd, cwd }),
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
}

export async function run_smart(block_id: number, name: string, args: string[]): Promise<string> {
    const response = await fetch(`/api/run_smart`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ block_id, name, args }),
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

export async function cancel(): Promise<void> {
    const response = await fetch(`/api/cancel`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
}

export async function open_path(path: string): Promise<void> {
    const response = await fetch(`/api/open_path`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path }),
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
}

export async function jobs(): Promise<JobInfo[]> {
    const response = await fetch(`/api/jobs`, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json' },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

export async function kill_job(job_id: number): Promise<void> {
    const response = await fetch(`/api/kill_job`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ job_id }),
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
}

export async function nl2cmd(nl: string): Promise<string> {
    const response = await fetch(`/api/nl2cmd`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ nl }),
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

export async function ai_pending(): Promise<string> {
    const response = await fetch(`/api/ai_pending`, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json' },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

// Plan 063 T1: suggest-next chips (JSON array string, "[]" = none, take-once).
export async function ai_next(): Promise<string> {
    const response = await fetch(`/api/ai_next`, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json' },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

// Plan 063 T2: last translation's split steps (newline-joined string, "" = none,
// take-once).
export async function ai_steps(): Promise<string> {
    const response = await fetch(`/api/ai_steps`, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json' },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

// Plan 064: boot-script command string ("" = none; submitted whole by Init).
export async function boot_script(): Promise<string> {
    const response = await fetch(`/api/boot_script`, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json' },
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
}

// stream() is consumed via SSE (EventSource) in the store composable; no fetch client. (path: /api/stream)
