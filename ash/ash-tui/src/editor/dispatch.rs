//! `EditCommand` → `LineBuffer` mutation dispatch (Plan 038 M1).
//!
//! This is the minimal port of reedline's `pub(crate)` `Editor::run_edit_command`
//! (`core_editor/editor.rs:54`). reedline dispatches all 88 `EditCommand`
//! variants with heavy selection + undo machinery; M1 implements only the 9
//! needed for "type + move + basic edit", skipping selection and undo.
//!
//! ## How this maps to reedline
//! Each arm here calls a **public** `LineBuffer` method (reedline's `LineBuffer`
//! API is fully `pub`). The reedline original wraps each call in a private
//! helper that also touches `selection_anchor` / `edit_stack`; we omit those
//! side effects entirely (no selection / undo state exists in M1's `Editor`).
//!
//! ## Cut buffer
//! `CutWordLeft` (C-w) and `KillLine` (C-k) write to `editor.cut_buffer`. M1
//! has no paste/yank command yet (that needs the `PasteCutBufferAfter` variant,
//! deferred to M2 along with the `Clipboard` trait abstraction).

use reedline::EditCommand;

use super::Editor;

/// Apply one `EditCommand` to the editor's `LineBuffer`.
///
/// Variants not in M1's minimal set fall through to a no-op `_ => {}` arm.
/// This includes `Undo`/`Redo` (no undo state), `SelectAll`/`CutSelection`/
/// `CopySelection` (no selection state), and all vi text-object / pair variants
/// (need `pub(crate)` LineBuffer methods). They are harmless: M1 binds no keys
/// that would emit them.
pub fn dispatch_edit_command(editor: &mut Editor, cmd: EditCommand) {
    let lb = &mut editor.line_buffer;
    match cmd {
        // ── Insertion ────────────────────────────────────────────────
        EditCommand::InsertChar(c) => lb.insert_char(c),
        EditCommand::InsertString(s) => lb.insert_str(&s),
        EditCommand::InsertNewline => lb.insert_newline(),

        // ── Deletion (no selection: direct grapheme ops) ────────────
        EditCommand::Backspace => lb.delete_left_grapheme(),
        EditCommand::Delete => lb.delete_right_grapheme(),
        // Word deletes — LineBuffer has direct public methods for these.
        EditCommand::BackspaceWord => lb.delete_word_left(),
        EditCommand::DeleteWord => lb.delete_word_right(),
        EditCommand::Clear => lb.clear(),
        EditCommand::ClearToLineEnd => lb.clear_to_line_end(),

        // ── Cursor movement (M1 ignores the `select` flag) ──────────
        // reedline's Move*{select} variants set a selection anchor when
        // select=true. M1 has no selection state, so we treat every move as
        // a plain cursor move (select=false semantics). This is correct as
        // long as M1 binds no shift+arrow-style selection keys.
        EditCommand::MoveToStart { .. } => lb.move_to_start(),
        EditCommand::MoveToLineStart { .. } => lb.move_to_line_start(),
        EditCommand::MoveToEnd { .. } => lb.move_to_end(),
        EditCommand::MoveToLineEnd { .. } => lb.move_to_line_end(),
        EditCommand::MoveLeft { .. } => lb.move_left(),
        EditCommand::MoveRight { .. } => lb.move_right(),
        EditCommand::MoveWordLeft { .. } => lb.move_word_left(),
        EditCommand::MoveBigWordLeft { .. } => lb.move_big_word_left(),
        EditCommand::MoveWordRight { .. } => lb.move_word_right(),
        EditCommand::MoveWordRightStart { .. } => lb.move_word_right_start(),
        EditCommand::MoveBigWordRightStart { .. } => lb.move_big_word_right_start(),
        EditCommand::MoveWordRightEnd { .. } => lb.move_word_right_end(),
        EditCommand::MoveBigWordRightEnd { .. } => lb.move_big_word_right_end(),
        EditCommand::MoveToPosition { position, .. } => lb.set_insertion_point(position),

        // ── Cut to the kill buffer (C-w / C-k) ───────────────────────
        // reedline routes these through cut_range(&cut_buffer, range). M1 has
        // a plain String cut_buffer, so we snapshot the about-to-be-deleted
        // text then delete it via the public LineBuffer API.
        EditCommand::CutWordLeft => {
            let start = lb.insertion_point();
            let cut_start = lb.word_left_index();
            if cut_start < start {
                let text = lb.get_buffer()[cut_start..start].to_string();
                editor.cut_buffer = text;
                lb.clear_range_safe(cut_start..start);
                lb.set_insertion_point(cut_start);
            }
        }
        EditCommand::CutBigWordLeft => {
            let start = lb.insertion_point();
            let cut_start = lb.big_word_left_index();
            if cut_start < start {
                let text = lb.get_buffer()[cut_start..start].to_string();
                editor.cut_buffer = text;
                lb.clear_range_safe(cut_start..start);
                lb.set_insertion_point(cut_start);
            }
        }
        // KillLine (Emacs C-k): cut from cursor to line end; if already at
        // line end, cut the newline. reedline's kill_line reuses cut_char for
        // the at-end case; M1 just clears to line end.
        EditCommand::KillLine => {
            let ip = lb.insertion_point();
            let line_end = lb.find_current_line_end();
            if ip < line_end {
                let text = lb.get_buffer()[ip..line_end].to_string();
                editor.cut_buffer = text;
                lb.clear_to_line_end();
            } else {
                // At line end: cut the following newline (if any) into the
                // kill buffer, joining lines.
                let right = lb.grapheme_right_index();
                if right > ip {
                    editor.cut_buffer = lb.get_buffer()[ip..right].to_string();
                    lb.clear_range_safe(ip..right);
                }
            }
        }
        EditCommand::CutFromStart => {
            let ip = lb.insertion_point();
            let text = lb.get_buffer()[..ip].to_string();
            editor.cut_buffer = text;
            lb.clear_to_insertion_point();
        }
        EditCommand::CutToEnd => {
            let ip = lb.insertion_point();
            let text = lb.get_buffer()[ip..].to_string();
            editor.cut_buffer = text;
            lb.clear_to_end();
        }
        EditCommand::CutToLineEnd => {
            let ip = lb.insertion_point();
            let line_end = lb.find_current_line_end();
            if ip < line_end {
                let text = lb.get_buffer()[ip..line_end].to_string();
                editor.cut_buffer = text;
                lb.clear_to_line_end();
            }
        }
        EditCommand::CutFromLineStart => {
            lb.move_to_line_start();
            let cut_start = lb.insertion_point();
            let line_end = lb.find_current_line_end();
            if cut_start < line_end {
                let text = lb.get_buffer()[cut_start..line_end].to_string();
                editor.cut_buffer = text;
                lb.clear_range_safe(cut_start..line_end);
                lb.set_insertion_point(cut_start);
            }
        }

        // ── Case operations (direct LineBuffer methods) ──────────────
        EditCommand::UppercaseWord => lb.uppercase_word(),
        EditCommand::LowercaseWord => lb.lowercase_word(),
        EditCommand::CapitalizeChar => lb.capitalize_char(),
        EditCommand::SwitchcaseChar => lb.switchcase_char(),
        EditCommand::SwapWords => lb.swap_words(),
        EditCommand::SwapGraphemes => lb.swap_graphemes(),

        // ── M1 no-ops (variants exist but M1 has no state / no binding) ─
        // Undo/Redo need EditStack; selection family need selection_anchor;
        // Complete is handled at the Reedline menu layer (M2); pair/textobject
        // variants need pub(crate) LineBuffer methods. All fall through here.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reedline::{default_emacs_keybindings, Emacs, LineBuffer};

    fn editor_with(text: &str) -> Editor {
        let mut e = Editor::new(Box::new(Emacs::new(default_emacs_keybindings())));
        e.line_buffer = LineBuffer::from(text);
        e
    }

    #[test]
    fn backspace_word_deletes_left_word() {
        let mut e = editor_with("hello world");
        e.line_buffer.set_insertion_point(11); // end
        dispatch_edit_command(&mut e, EditCommand::BackspaceWord);
        assert_eq!(e.text(), "hello ");
    }

    #[test]
    fn cut_word_left_populates_cut_buffer() {
        let mut e = editor_with("foo bar baz");
        e.line_buffer.set_insertion_point(11); // end
        dispatch_edit_command(&mut e, EditCommand::CutWordLeft);
        assert_eq!(e.text(), "foo bar ");
        assert_eq!(e.cut_buffer, "baz");
    }

    #[test]
    fn kill_line_to_end() {
        let mut e = editor_with("hello world");
        e.line_buffer.set_insertion_point(5); // after "hello"
        dispatch_edit_command(&mut e, EditCommand::KillLine);
        assert_eq!(e.text(), "hello");
        assert_eq!(e.cut_buffer, " world");
    }

    #[test]
    fn clear_wipes_buffer() {
        let mut e = editor_with("abc");
        dispatch_edit_command(&mut e, EditCommand::Clear);
        assert!(e.text().is_empty());
    }

    #[test]
    fn move_word_left_advances_cursor() {
        let mut e = editor_with("hello world");
        e.line_buffer.set_insertion_point(11);
        dispatch_edit_command(&mut e, EditCommand::MoveWordLeft { select: false });
        assert_eq!(e.insertion_point(), 6);
    }

    #[test]
    fn move_word_right_advances_cursor() {
        let mut e = editor_with("hello world");
        e.line_buffer.set_insertion_point(0);
        // MoveWordRight (M-f) lands at the END of the current word ("hello"→5),
        // per reedline's word_right_index semantics. MoveWordRightStart lands
        // at the START of the next word ("world"→6).
        dispatch_edit_command(&mut e, EditCommand::MoveWordRight { select: false });
        assert_eq!(e.insertion_point(), 5);
        dispatch_edit_command(&mut e, EditCommand::MoveWordRightStart { select: false });
        assert_eq!(e.insertion_point(), 6);
    }

    #[test]
    fn unknown_variants_are_noop() {
        let mut e = editor_with("abc");
        // Undo has no state in M1 — must be a harmless no-op.
        dispatch_edit_command(&mut e, EditCommand::Undo);
        assert_eq!(e.text(), "abc");
        // SelectAll needs selection state — no-op.
        dispatch_edit_command(&mut e, EditCommand::SelectAll);
        assert_eq!(e.text(), "abc");
    }

    #[test]
    fn uppercase_word_transforms() {
        let mut e = editor_with("hello world");
        e.line_buffer.set_insertion_point(0);
        dispatch_edit_command(&mut e, EditCommand::UppercaseWord);
        assert_eq!(e.text(), "HELLO world");
    }
}
