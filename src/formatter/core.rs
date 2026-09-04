//! Core formatting entry point.
//!
//! Implements the full formatting pipeline:
//!
//! ```text
//! Source → strip shebang → epoch prescan → lex (separate tokens + comments)
//!       → parse to Syntax → collect trivia → attach trivia
//!       → generate Doc → render → prepend shebang + trailing newline
//! ```

use super::comments::{lex_for_format, strip_shebang};
use super::config::FormatterConfig;
use super::format::format_forms;
use super::render::render;
use super::trivia::{collect_trivia, AnnotatedSyntax, CommentInfo};
use crate::reader::{lexicon_for, SyntaxReader};

/// Format Elle source code with the given configuration.
///
/// Returns the formatted string, or an error if parsing fails.
pub fn format_code(source: &str, config: &FormatterConfig) -> Result<String, String> {
    // 1. Strip shebang (single strip point for consistent byte offsets)
    let (stripped, shebang) = strip_shebang(source);

    // 2. Lex: separate regular tokens from comment tokens, under the rules
    //    this source's own epoch declares (docs/impl/lexicon.md)
    let lexed = lex_for_format(stripped, "<format>", lexicon_for(stripped)?)?;

    // 3. Parse regular tokens to Syntax tree. `elle fmt` runs without a
    //    runtime, so the tree gets its own heap, freed when this returns.
    let mut home = crate::syntax::SyntaxHeap::new();
    let forms = if lexed.tokens.is_empty() {
        Vec::new()
    } else {
        let mut parser = SyntaxReader::with_byte_offsets(
            lexed.tokens,
            lexed.locations,
            lexed.lengths,
            lexed.byte_offsets,
            home.arena(),
        );
        parser.read_all()?
    };

    // 4. Collect trivia: merge comments from lexer with blank lines from source
    let comment_data: Vec<CommentInfo> = lexed
        .comment_map
        .comments()
        .iter()
        .map(|c| CommentInfo {
            text: c.text.clone(),
            offset: c.byte_offset,
            line: c.line,
        })
        .collect();
    let trivia = collect_trivia(stripped, &comment_data);

    // 5. Attach trivia to Syntax nodes
    let (annotated, dangling) = AnnotatedSyntax::build_toplevel(forms, &trivia, stripped);

    // 6. Generate Doc tree from annotated syntax
    let doc = format_forms(&annotated, &dangling, stripped, config);

    // 7. Render Doc to string
    let rendered = render(&doc, config);

    // 8. Assemble output: shebang + rendered + trailing newline
    //    Strip leading newline from rendered output — format_annotated
    //    emits HardBreak before leading comments, which produces a
    //    spurious newline at the document start.
    let rendered = rendered.trim_start_matches('\n');

    let mut output = String::new();
    if !shebang.is_empty() {
        output.push_str(shebang);
    }
    output.push_str(rendered);

    // 9. Strip trailing whitespace from every line. The renderer emits
    //    indent spaces before HardBreaks (blank separator lines), producing
    //    lines with only whitespace. Strip these so editors and linters
    //    don't complain.
    let output: String = output
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    let mut output = output;
    if !output.ends_with('\n') {
        output.push('\n');
    }

    Ok(output)
}

#[cfg(test)]
mod tests;
