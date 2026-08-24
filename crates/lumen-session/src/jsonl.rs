use std::io::BufRead;

/// One outcome from [`jsonl_lines`]: either a real, trimmed, non-empty line ready for
/// `serde_json::from_str`, or an unreadable line's diagnostic (`session_id` deliberately absent
/// -- only the caller's own in-progress parse state knows it at this point in the stream, since
/// it's often discovered from a *later* line than the one that failed).
pub enum JsonlLine {
    Line { line_number: usize, byte_offset: usize, text: String },
    Unreadable { line_number: usize, byte_offset: usize, error: String },
}

/// Iterates `reader` line-by-line with CRIT-LUMEN-025's byte-offset tracking and
/// io::Error-as-bad-line handling, shared by every JSONL-format adapter (`ClaudeCodeAdapter`,
/// `CodexAdapter`, `AgyAdapter`) instead of each maintaining its own near-identical copy
/// (CRIT-LUMEN-173) -- previously all three implemented this nearly verbatim, self-acknowledged
/// in their own comments ("same...rationale as claude.rs").
///
/// `byte_offset` is the offset at the START of each line: an LF-based `+1`-per-line
/// approximation (undercounts by 1 byte per line for CRLF-terminated input) that also does NOT
/// advance for an `Unreadable` line (its true byte length is unknown -- it never became a
/// `String`), so the next successfully-read line's reported offset undercounts by that line's
/// real length too. A documented limitation of an already-approximate diagnostic field, not a
/// byte-exact file-seek guarantee -- unchanged from each adapter's prior individual behavior.
///
/// Blank lines (after `.trim()`) are silently skipped -- never yielded as a `Line` or
/// `Unreadable`. Does NOT strip a UTF-8 BOM or parse JSON -- `ClaudeCodeAdapter` needs BOM
/// stripping before parsing and the other two don't, and turning a line's text into a
/// `serde_json::Value` (and recording a parse-failure on error) is left to each adapter's own
/// loop, since it stays a matter of a few lines and forcing it into this shared iterator would
/// only replace one small duplication with a differently-shaped one.
pub fn jsonl_lines<'a>(reader: Box<dyn BufRead + 'a>) -> impl Iterator<Item = JsonlLine> + 'a {
    let mut byte_offset: usize = 0;

    reader.lines().enumerate().filter_map(move |(idx, line_res)| {
        let line_number = idx + 1;
        match line_res {
            Ok(line) => {
                let line_start_offset = byte_offset;
                byte_offset += line.len() + 1;

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(JsonlLine::Line { line_number, byte_offset: line_start_offset, text: trimmed.to_string() })
                }
            }
            Err(e) => Some(JsonlLine::Unreadable { line_number, byte_offset, error: e.to_string() }),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn collect(input: &[u8]) -> Vec<JsonlLine> {
        jsonl_lines(Box::new(Cursor::new(input.to_vec()))).collect()
    }

    #[test]
    fn yields_line_number_byte_offset_and_trimmed_text_for_each_real_line() {
        let lines = collect(b"{\"a\":1}\n{\"b\":2}\n");
        assert_eq!(lines.len(), 2);
        match &lines[0] {
            JsonlLine::Line { line_number, byte_offset, text } => {
                assert_eq!(*line_number, 1);
                assert_eq!(*byte_offset, 0);
                assert_eq!(text, "{\"a\":1}");
            }
            JsonlLine::Unreadable { .. } => panic!("expected a Line"),
        }
        match &lines[1] {
            JsonlLine::Line { line_number, byte_offset, text } => {
                assert_eq!(*line_number, 2);
                assert_eq!(*byte_offset, 8); // len("{\"a\":1}") + 1 for the LF
                assert_eq!(text, "{\"b\":2}");
            }
            JsonlLine::Unreadable { .. } => panic!("expected a Line"),
        }
    }

    #[test]
    fn skips_blank_and_whitespace_only_lines_silently() {
        let lines = collect(b"{\"a\":1}\n\n   \n{\"b\":2}\n");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn trims_leading_and_trailing_whitespace_from_a_real_line() {
        let lines = collect(b"   {\"a\":1}   \n");
        match &lines[0] {
            JsonlLine::Line { text, .. } => assert_eq!(text, "{\"a\":1}"),
            JsonlLine::Unreadable { .. } => panic!("expected a Line"),
        }
    }

    #[test]
    fn surfaces_a_non_utf8_line_as_unreadable_without_stopping_the_stream() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(b"{\"a\":1}\n");
        input.extend_from_slice(b"bad utf8: \xFF\xFE\n");
        input.extend_from_slice(b"{\"b\":2}\n");

        let lines = collect(&input);
        assert_eq!(lines.len(), 3);
        assert!(matches!(&lines[0], JsonlLine::Line { .. }));
        assert!(matches!(&lines[1], JsonlLine::Unreadable { line_number: 2, .. }));
        assert!(matches!(&lines[2], JsonlLine::Line { .. }));
    }

    #[test]
    fn does_not_advance_byte_offset_for_an_unreadable_line() {
        // The unreadable line's true byte length is unknown (it never became a String), so the
        // next real line's reported offset undercounts by that line's real length -- documented,
        // deliberate behavior, not a bug.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(b"bad utf8: \xFF\xFE\n"); // 13 real bytes, never counted
        input.extend_from_slice(b"{\"a\":1}\n");

        let lines = collect(&input);
        match &lines[1] {
            JsonlLine::Line { byte_offset, .. } => assert_eq!(*byte_offset, 0),
            JsonlLine::Unreadable { .. } => panic!("expected a Line"),
        }
    }

    #[test]
    fn empty_input_yields_no_lines() {
        assert!(collect(b"").is_empty());
    }
}
