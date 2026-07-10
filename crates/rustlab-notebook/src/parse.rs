/// Directives that modify how a code block is displayed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CodeDirectives {
    /// Hide the source code (still executed).
    pub hidden: bool,
    /// Wrap output in a collapsible disclosure widget with this title.
    pub details: Option<String>,
    /// Tile figure outputs N-across in a grid layout.
    pub grid_cols: Option<usize>,
    /// Caption text (used by formats that support figure captions, e.g. LaTeX).
    pub caption: Option<String>,
}

/// Directives that modify how a Mermaid diagram is displayed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MermaidDirectives {
    /// Hide the diagram entirely (block is parsed but produces no output).
    pub hidden: bool,
    /// Wrap output in a collapsible disclosure widget with this title.
    pub details: Option<String>,
    /// Caption text (used by formats that support figure captions, e.g. LaTeX).
    pub caption: Option<String>,
}

/// The kind of callout box. Aligned with the GitHub / Obsidian shared
/// set so `> [!NOTE]` source renders identically across all surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl CalloutKind {
    /// Map a `> [!TYPE]` tag to a `CalloutKind`. Aliases:
    /// `INFO` → Note, `HINT` → Tip, `DANGER` → Caution. Case-insensitive.
    pub fn from_gfm_tag(tag: &str) -> Option<Self> {
        match tag.to_ascii_uppercase().as_str() {
            "NOTE" | "INFO" => Some(CalloutKind::Note),
            "TIP" | "HINT" => Some(CalloutKind::Tip),
            "IMPORTANT" => Some(CalloutKind::Important),
            "WARNING" => Some(CalloutKind::Warning),
            "CAUTION" | "DANGER" => Some(CalloutKind::Caution),
            _ => None,
        }
    }

    /// Default human-readable title shown when a callout has no explicit
    /// title (e.g. "Note", "Warning").
    pub(crate) fn default_label(&self) -> &'static str {
        match self {
            CalloutKind::Note => "Note",
            CalloutKind::Tip => "Tip",
            CalloutKind::Important => "Important",
            CalloutKind::Warning => "Warning",
            CalloutKind::Caution => "Caution",
        }
    }
}

/// A block in a parsed notebook.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// Raw markdown text (to be rendered as HTML).
    Markdown(String),
    /// A ```rustlab fenced code block (source to be executed).
    Code {
        source: String,
        directives: CodeDirectives,
    },
    /// A ```mermaid fenced diagram block (rendered to SVG, not executed).
    Mermaid {
        source: String,
        directives: MermaidDirectives,
    },
    /// A ```rustlab-widget fenced block declaring an interactive control.
    /// `source` is the raw TOML body (kept for block hashing / re-render
    /// stability); `decl` is the parsed declaration the renderer and server
    /// consume. Malformed declarations become a `Callout` instead.
    Widget {
        decl: crate::widget::WidgetDecl,
        source: String,
    },
    /// A callout box. `title` is the optional override on `> [!TIP] My title`;
    /// when `None`, the renderer uses the default kind label.
    Callout {
        kind: CalloutKind,
        title: Option<String>,
        content: String,
    },
    /// Start of a numbered exercise.
    ExerciseStart,
    /// Start of a solution (collapsed by default).
    SolutionStart,
}

/// Parse a markdown notebook into a sequence of blocks.
///
/// - Optional YAML frontmatter (`---` delimited) is stripped.
/// - ` ```rustlab ` fenced code blocks become `Block::Code`.
/// - Everything else (including other fenced blocks) becomes `Block::Markdown`.
pub fn parse_notebook(src: &str) -> Vec<Block> {
    let src = strip_frontmatter(src);
    let mut blocks = Vec::new();
    let mut markdown_buf = String::new();
    let mut code_buf = String::new();
    let mut in_rustlab = false;
    let mut in_mermaid = false;
    let mut in_widget = false;
    let mut code_directives = CodeDirectives::default();
    let mut mermaid_directives = MermaidDirectives::default();

    // State for exercise/solution scope tracking
    let mut in_exercise = false;
    let mut in_solution = false;

    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if in_rustlab {
            if trimmed == "```" {
                // End of rustlab code block
                blocks.push(Block::Code {
                    source: code_buf.clone(),
                    directives: code_directives.clone(),
                });
                code_buf.clear();
                in_rustlab = false;
                code_directives = CodeDirectives::default();
            } else {
                if !code_buf.is_empty() {
                    code_buf.push('\n');
                }
                code_buf.push_str(line);
            }
            i += 1;
        } else if in_mermaid {
            if trimmed == "```" {
                blocks.push(Block::Mermaid {
                    source: code_buf.clone(),
                    directives: mermaid_directives.clone(),
                });
                code_buf.clear();
                in_mermaid = false;
                mermaid_directives = MermaidDirectives::default();
            } else {
                if !code_buf.is_empty() {
                    code_buf.push('\n');
                }
                code_buf.push_str(line);
            }
            i += 1;
        } else if in_widget {
            if trimmed == "```" {
                // End of widget block — parse the TOML body. A malformed
                // declaration becomes a Caution callout (the page still
                // renders; downstream `widget("name")` calls then error).
                match crate::widget::parse_widget(&code_buf) {
                    Ok(decl) => blocks.push(Block::Widget {
                        decl,
                        source: code_buf.clone(),
                    }),
                    Err(msg) => blocks.push(Block::Callout {
                        kind: CalloutKind::Caution,
                        title: Some("Widget error".to_string()),
                        content: msg,
                    }),
                }
                code_buf.clear();
                in_widget = false;
            } else {
                if !code_buf.is_empty() {
                    code_buf.push('\n');
                }
                code_buf.push_str(line);
            }
            i += 1;
        } else if trimmed == "```rustlab-widget" || trimmed.starts_with("```rustlab-widget ") {
            // Flush any pending markdown before the widget declaration.
            if !markdown_buf.is_empty() {
                blocks.push(Block::Markdown(markdown_buf.clone()));
                markdown_buf.clear();
            }
            in_widget = true;
            i += 1;
        } else if trimmed == "```rustlab" || trimmed.starts_with("```rustlab ") {
            // Extract stacked code-block directives from the tail of the markdown buffer
            code_directives = extract_code_directives(&mut markdown_buf);
            // Start of rustlab code block — flush markdown buffer
            if !markdown_buf.is_empty() {
                blocks.push(Block::Markdown(markdown_buf.clone()));
                markdown_buf.clear();
            }
            in_rustlab = true;
            i += 1;
        } else if trimmed == "```mermaid" || trimmed.starts_with("```mermaid ") {
            mermaid_directives = extract_mermaid_directives(&mut markdown_buf);
            if !markdown_buf.is_empty() {
                blocks.push(Block::Markdown(markdown_buf.clone()));
                markdown_buf.clear();
            }
            in_mermaid = true;
            i += 1;
        } else if let Some((kind, title)) = parse_gfm_callout_header(trimmed) {
            // GitHub / Obsidian native callout: `> [!NOTE]` opens a
            // contiguous blockquote whose body is the callout content.
            if !markdown_buf.is_empty() {
                blocks.push(Block::Markdown(markdown_buf.clone()));
                markdown_buf.clear();
            }
            i += 1;
            let mut content = String::new();
            while i < lines.len() {
                let cl = lines[i];
                let ct = cl.trim();
                // Strip one level of `> ` prefix; bare `>` becomes blank line.
                let stripped = if let Some(rest) = ct.strip_prefix("> ") {
                    Some(rest)
                } else if ct == ">" {
                    Some("")
                } else {
                    None
                };
                match stripped {
                    Some(body) => {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(body);
                        i += 1;
                    }
                    None => break,
                }
            }
            blocks.push(Block::Callout {
                kind,
                title,
                content,
            });
        } else if let Some(directive) = parse_markdown_directive(trimmed) {
            match directive {
                MarkdownDirective::Callout(kind) => {
                    // Flush markdown buffer
                    if !markdown_buf.is_empty() {
                        blocks.push(Block::Markdown(markdown_buf.clone()));
                        markdown_buf.clear();
                    }
                    // Collect callout content until blank line, heading, or closing tag.
                    // Only the three legacy kinds can reach this branch — `Important`
                    // and `Caution` exist solely for the GFM `> [!IMPORTANT]` form.
                    let closing_tag = match kind {
                        CalloutKind::Note => "<!-- /note -->",
                        CalloutKind::Tip => "<!-- /tip -->",
                        CalloutKind::Warning => "<!-- /warning -->",
                        CalloutKind::Important | CalloutKind::Caution => unreachable!(
                            "legacy `<!-- ... -->` callouts only produce Note/Tip/Warning"
                        ),
                    };
                    let mut content = String::new();
                    i += 1;
                    while i < lines.len() {
                        let cl = lines[i];
                        let ct = cl.trim();
                        if ct == closing_tag {
                            i += 1; // consume closing tag
                            break;
                        }
                        if ct.is_empty() || ct.starts_with('#') {
                            break; // blank line or heading ends single-paragraph callout
                        }
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(cl);
                        i += 1;
                    }
                    if !content.is_empty() {
                        blocks.push(Block::Callout {
                            kind,
                            title: None,
                            content,
                        });
                    }
                }
                MarkdownDirective::ExerciseStart => {
                    // Flush markdown buffer
                    if !markdown_buf.is_empty() {
                        blocks.push(Block::Markdown(markdown_buf.clone()));
                        markdown_buf.clear();
                    }
                    // Auto-close any open solution/exercise
                    if in_solution {
                        blocks.push(Block::SolutionStart); // close marker handled by renderer
                        in_solution = false;
                    }
                    if in_exercise {
                        // previous exercise had no solution — that's ok
                    }
                    blocks.push(Block::ExerciseStart);
                    in_exercise = true;
                    i += 1;
                }
                MarkdownDirective::SolutionStart => {
                    // Flush markdown buffer
                    if !markdown_buf.is_empty() {
                        blocks.push(Block::Markdown(markdown_buf.clone()));
                        markdown_buf.clear();
                    }
                    blocks.push(Block::SolutionStart);
                    in_solution = true;
                    i += 1;
                }
            }
        } else {
            if !markdown_buf.is_empty() {
                markdown_buf.push('\n');
            }
            markdown_buf.push_str(line);
            i += 1;
        }
    }

    // Flush remaining content
    if in_rustlab && !code_buf.is_empty() {
        // Unclosed code block — treat as code anyway
        blocks.push(Block::Code {
            source: code_buf,
            directives: code_directives,
        });
    } else if in_mermaid && !code_buf.is_empty() {
        blocks.push(Block::Mermaid {
            source: code_buf,
            directives: mermaid_directives,
        });
    } else if in_widget && !code_buf.is_empty() {
        // Unclosed widget fence — parse what we have; errors become a callout.
        match crate::widget::parse_widget(&code_buf) {
            Ok(decl) => blocks.push(Block::Widget {
                decl,
                source: code_buf,
            }),
            Err(msg) => blocks.push(Block::Callout {
                kind: CalloutKind::Caution,
                title: Some("Widget error".to_string()),
                content: msg,
            }),
        }
    }
    if !markdown_buf.is_empty() {
        blocks.push(Block::Markdown(markdown_buf));
    }

    blocks
}

/// Directives that appear in the markdown flow (not before code blocks).
enum MarkdownDirective {
    Callout(CalloutKind),
    ExerciseStart,
    SolutionStart,
}

/// Detect a GitHub / Obsidian-native callout opener: `> [!TYPE]`,
/// optionally followed by a `+` (open by default — Obsidian) or `-`
/// (collapsed by default) foldable suffix and an optional title:
///
/// ```text
/// > [!NOTE]
/// > [!TIP] Heads up
/// > [!WARNING]+
/// > [!IMPORTANT]- Custom title
/// ```
///
/// Returns `(kind, optional_title)`. The foldable `+` / `-` suffix is
/// silently consumed — we don't currently expose that to renderers.
fn parse_gfm_callout_header(trimmed: &str) -> Option<(CalloutKind, Option<String>)> {
    let rest = trimmed.strip_prefix("> [!")?;
    let close = rest.find(']')?;
    let tag = &rest[..close];
    let kind = CalloutKind::from_gfm_tag(tag)?;
    let after = rest[close + 1..].trim_start_matches(['+', '-']).trim();
    let title = if after.is_empty() {
        None
    } else {
        Some(after.to_string())
    };
    Some((kind, title))
}

/// Try to parse a trimmed line as a markdown-flow directive.
fn parse_markdown_directive(trimmed: &str) -> Option<MarkdownDirective> {
    match trimmed {
        "<!-- note -->" => Some(MarkdownDirective::Callout(CalloutKind::Note)),
        "<!-- tip -->" => Some(MarkdownDirective::Callout(CalloutKind::Tip)),
        "<!-- warning -->" => Some(MarkdownDirective::Callout(CalloutKind::Warning)),
        "<!-- exercise -->" => Some(MarkdownDirective::ExerciseStart),
        "<!-- solution -->" => Some(MarkdownDirective::SolutionStart),
        _ => None,
    }
}

/// Try to parse a trimmed line as a code-block directive (appears before a ```rustlab fence).
fn parse_code_directive(trimmed: &str) -> Option<CodeDirectiveKind> {
    if trimmed == "<!-- hide -->" {
        return Some(CodeDirectiveKind::Hide);
    }
    if let Some(rest) = trimmed.strip_prefix("<!-- details:") {
        if let Some(title) = rest.strip_suffix("-->") {
            let title = title.trim();
            if !title.is_empty() {
                return Some(CodeDirectiveKind::Details(title.to_string()));
            }
        }
    }
    if let Some(rest) = trimmed.strip_prefix("<!-- grid:") {
        if let Some(n_str) = rest.strip_suffix("-->") {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                if n > 0 {
                    return Some(CodeDirectiveKind::Grid(n));
                }
            }
        }
    }
    if let Some(rest) = trimmed.strip_prefix("<!-- caption:") {
        if let Some(text) = rest.strip_suffix("-->") {
            let text = text.trim();
            if !text.is_empty() {
                return Some(CodeDirectiveKind::Caption(text.to_string()));
            }
        }
    }
    None
}

enum CodeDirectiveKind {
    Hide,
    Details(String),
    Grid(usize),
    Caption(String),
}

/// Scan backward from the tail of the markdown buffer to collect stacked
/// code-block directives. Matched lines are removed from the buffer.
fn extract_code_directives(markdown_buf: &mut String) -> CodeDirectives {
    let mut directives = CodeDirectives::default();

    // Repeatedly check the last line for a directive
    loop {
        let last_line = match markdown_buf.lines().last() {
            Some(l) => l.trim().to_string(),
            None => break,
        };
        match parse_code_directive(&last_line) {
            Some(CodeDirectiveKind::Hide) => directives.hidden = true,
            Some(CodeDirectiveKind::Details(title)) => directives.details = Some(title),
            Some(CodeDirectiveKind::Grid(n)) => directives.grid_cols = Some(n),
            Some(CodeDirectiveKind::Caption(text)) => directives.caption = Some(text),
            None => break,
        }
        // Remove the directive line from the buffer
        if let Some(pos) = markdown_buf.rfind(&last_line) {
            // Find the start of this line (including preceding newline)
            let start = if pos > 0 && markdown_buf.as_bytes().get(pos - 1) == Some(&b'\n') {
                pos - 1
            } else {
                pos
            };
            markdown_buf.truncate(start);
        }
    }

    // Trim trailing whitespace left behind
    let trimmed_len = markdown_buf.trim_end().len();
    markdown_buf.truncate(trimmed_len);

    directives
}

/// Same as `extract_code_directives` but for Mermaid blocks. Mermaid does
/// not support the `grid` directive (single SVG per block); other directives
/// share the same vocabulary as code blocks.
fn extract_mermaid_directives(markdown_buf: &mut String) -> MermaidDirectives {
    let mut directives = MermaidDirectives::default();

    loop {
        let last_line = match markdown_buf.lines().last() {
            Some(l) => l.trim().to_string(),
            None => break,
        };
        match parse_code_directive(&last_line) {
            Some(CodeDirectiveKind::Hide) => directives.hidden = true,
            Some(CodeDirectiveKind::Details(title)) => directives.details = Some(title),
            Some(CodeDirectiveKind::Caption(text)) => directives.caption = Some(text),
            // Grid is meaningless for Mermaid (single SVG per block) — ignore but consume.
            Some(CodeDirectiveKind::Grid(_)) => {}
            None => break,
        }
        if let Some(pos) = markdown_buf.rfind(&last_line) {
            let start = if pos > 0 && markdown_buf.as_bytes().get(pos - 1) == Some(&b'\n') {
                pos - 1
            } else {
                pos
            };
            markdown_buf.truncate(start);
        }
    }

    let trimmed_len = markdown_buf.trim_end().len();
    markdown_buf.truncate(trimmed_len);

    directives
}

/// Parsed YAML frontmatter.
///
/// Only the keys that the notebook renderer understands are surfaced; unknown
/// keys are ignored silently so future additions don't break existing files.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Frontmatter {
    /// Overrides the title used in rendered output and the index page.
    pub title: Option<String>,
    /// Sort weight for the index page (ascending). Ties fall back to filename.
    pub order: Option<i64>,
}

/// Extract YAML frontmatter and the remaining body from a notebook source.
///
/// If no closed `---` block is present, the returned frontmatter is empty and
/// the body is the original source unchanged.
pub fn extract_frontmatter(src: &str) -> (Frontmatter, &str) {
    let (fm_block, body) = match split_frontmatter(src) {
        Some(pair) => pair,
        None => return (Frontmatter::default(), src),
    };
    (parse_frontmatter(fm_block), body)
}

/// Strip optional YAML frontmatter delimited by `---` lines.
fn strip_frontmatter(src: &str) -> &str {
    split_frontmatter(src).map(|(_, body)| body).unwrap_or(src)
}

/// Byte offset where the parseable body begins — i.e. everything the
/// frontmatter scanner skips (leading whitespace + `---` block). The
/// splice below preserves `src[..offset]` verbatim.
fn body_offset(src: &str) -> usize {
    let body = strip_frontmatter(src);
    body.as_ptr() as usize - src.as_ptr() as usize
}

/// Replace the body of the `code_ordinal`-th ```` ```rustlab ```` fence
/// (0-based, counting only rustlab fences) with `new_source`, leaving
/// every other byte of `src` untouched — frontmatter, prose, other
/// fences, directives, the fence's own opener/closer lines, and line
/// endings all survive verbatim.
///
/// This is the write-back half of the interactive server's inline cell
/// editor. It mirrors [`parse_notebook`]'s exact fence rules (including
/// its quirks — e.g. no generic-fence tracking) so the ordinal maps to
/// the same block the parser sees; callers must still re-parse the
/// result and verify the target block equals `new_source` before writing
/// to disk (a `new_source` containing e.g. a bare ``` ``` ``` line closes
/// the fence early — the re-parse check turns that into a rejected save
/// instead of a corrupted file).
///
/// Returns `None` when `src` has fewer than `code_ordinal + 1` rustlab
/// fences.
pub fn replace_code_block_source(
    src: &str,
    code_ordinal: usize,
    new_source: &str,
) -> Option<String> {
    let split = body_offset(src);
    let (front, body) = src.split_at(split);
    let mut out = String::with_capacity(src.len() + new_source.len());
    out.push_str(front);

    let mut in_rustlab = false;
    let mut in_mermaid = false;
    let mut in_widget = false;
    let mut seen = 0usize; // rustlab fences encountered so far
    let mut replacing = false; // inside the target fence (old body dropped)
    let mut replaced = false;

    // Emit `new_source` with a trailing newline: re-parsing joins body
    // lines with '\n', so `body + "\n"` round-trips both `"a"` and
    // `"a\n"` (trailing blank line) back to the exact `new_source`.
    let emit_new = |out: &mut String| {
        out.push_str(new_source);
        out.push('\n');
    };

    for line in body.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = content.trim();
        if in_rustlab {
            if trimmed == "```" {
                in_rustlab = false;
                if replacing {
                    emit_new(&mut out);
                    replacing = false;
                    replaced = true;
                }
                out.push_str(line);
            } else if !replacing {
                out.push_str(line);
            }
            // (replacing → old body line dropped)
        } else if in_mermaid || in_widget {
            if trimmed == "```" {
                in_mermaid = false;
                in_widget = false;
            }
            out.push_str(line);
        } else if trimmed == "```rustlab-widget" || trimmed.starts_with("```rustlab-widget ") {
            in_widget = true;
            out.push_str(line);
        } else if trimmed == "```rustlab" || trimmed.starts_with("```rustlab ") {
            in_rustlab = true;
            if seen == code_ordinal {
                replacing = true;
            }
            seen += 1;
            out.push_str(line);
        } else if trimmed == "```mermaid" || trimmed.starts_with("```mermaid ") {
            in_mermaid = true;
            out.push_str(line);
        } else {
            out.push_str(line);
        }
    }

    // Unclosed target fence at EOF: parse_notebook still yields it as a
    // Code block, so emit the replacement there too.
    if replacing {
        emit_new(&mut out);
        replaced = true;
    }

    replaced.then_some(out)
}

/// Locate a closed `---`-delimited frontmatter block at the start of `src`.
/// Returns `(frontmatter_body, rest_of_source)` when found.
fn split_frontmatter(src: &str) -> Option<(&str, &str)> {
    let trimmed = src.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_open = &trimmed[3..];
    // The opening `---` line must terminate with a newline (anything after `---`
    // on the same line is rejected so we don't misparse a horizontal rule).
    let first_nl = after_open.find('\n')?;
    if !after_open[..first_nl].trim().is_empty() {
        return None;
    }
    let rest = &after_open[first_nl + 1..];
    let mut consumed = 0;
    for line in rest.lines() {
        if line.trim() == "---" {
            let body_start = consumed + line.len();
            // Skip the trailing newline after the closing `---`, if any.
            let body = rest.get(body_start..).unwrap_or("");
            let body = body.strip_prefix('\n').unwrap_or(body);
            let fm = &rest[..consumed];
            return Some((fm, body));
        }
        consumed += line.len() + 1; // +1 for the newline separator
    }
    None
}

/// Minimal hand-rolled YAML scanner that recognises `key: value` lines.
/// Quoted strings (single or double) are unwrapped; everything else is taken
/// verbatim. Unknown keys are ignored.
fn parse_frontmatter(src: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, raw_val)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let val = unquote(raw_val.trim());
        match key {
            "title" => {
                if !val.is_empty() {
                    fm.title = Some(val);
                }
            }
            "order" | "weight" => {
                if let Ok(n) = val.parse::<i64>() {
                    fm.order = Some(n);
                }
            }
            _ => {}
        }
    }
    fm
}

fn unquote(s: &str) -> String {
    if s.len() >= 2 {
        let b = s.as_bytes();
        let first = b[0];
        let last = b[s.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_code(block: &Block, expected_src: &str, expected_hidden: bool) -> bool {
        matches!(block, Block::Code { source, directives } if source == expected_src && directives.hidden == expected_hidden)
    }

    #[test]
    fn simple_notebook() {
        let src = "# Title\n\nSome text.\n\n```rustlab\nx = 1:10\n```\n\nMore text.";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], Block::Markdown(s) if s.contains("Title")));
        assert!(is_code(&blocks[1], "x = 1:10", false));
        assert!(matches!(&blocks[2], Block::Markdown(s) if s.contains("More text")));
    }

    #[test]
    fn frontmatter_stripped() {
        let src = "---\ntitle: Test\n---\n# Heading\n";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Markdown(s) if s.contains("Heading")));
    }

    #[test]
    fn other_fences_are_markdown() {
        let src = "```python\nprint('hi')\n```\n";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Markdown(_)));
    }

    #[test]
    fn multiple_code_blocks() {
        let src = "Text\n\n```rustlab\na = 1\n```\n\nMiddle\n\n```rustlab\nb = 2\n```\n\nEnd";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 5);
        assert!(matches!(&blocks[0], Block::Markdown(_)));
        assert!(is_code(&blocks[1], "a = 1", false));
        assert!(matches!(&blocks[2], Block::Markdown(_)));
        assert!(is_code(&blocks[3], "b = 2", false));
        assert!(matches!(&blocks[4], Block::Markdown(_)));
    }

    #[test]
    fn hide_directive() {
        let src = "Setup:\n\n<!-- hide -->\n```rustlab\nx = 42\n```\n\nVisible:\n\n```rustlab\ndisp(x)\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 4);
        assert!(matches!(&blocks[0], Block::Markdown(s) if s.contains("Setup:")));
        assert!(is_code(&blocks[1], "x = 42", true));
        assert!(matches!(&blocks[2], Block::Markdown(s) if s.contains("Visible:")));
        assert!(is_code(&blocks[3], "disp(x)", false));
    }

    #[test]
    fn empty_input() {
        let blocks = parse_notebook("");
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn markdown_only() {
        let src = "# Title\n\nJust prose, no code.";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Markdown(s) if s.contains("Just prose")));
    }

    #[test]
    fn code_only() {
        let src = "```rustlab\nx = 1\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        assert!(is_code(&blocks[0], "x = 1", false));
    }

    #[test]
    fn empty_code_block() {
        let src = "```rustlab\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Code { source, .. } if source.is_empty()));
    }

    #[test]
    fn unclosed_code_block() {
        let src = "```rustlab\nx = 1\ny = 2";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], Block::Code { source, .. } if source.contains("x = 1") && source.contains("y = 2"))
        );
    }

    #[test]
    fn consecutive_code_blocks() {
        let src = "```rustlab\na = 1\n```\n```rustlab\nb = 2\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], Block::Code { source, .. } if source == "a = 1"));
        assert!(matches!(&blocks[1], Block::Code { source, .. } if source == "b = 2"));
    }

    #[test]
    fn hide_directive_strips_from_markdown() {
        let src = "Intro\n\n<!-- hide -->\n```rustlab\nx = 1\n```";
        let blocks = parse_notebook(src);
        if let Block::Markdown(md) = &blocks[0] {
            assert!(
                !md.contains("<!-- hide -->"),
                "directive should be stripped: {md:?}"
            );
        }
    }

    #[test]
    fn hide_at_start_of_file() {
        let src = "<!-- hide -->\n```rustlab\nsetup_code\n```\n\nVisible text";
        let blocks = parse_notebook(src);
        assert!(matches!(&blocks[0], Block::Code { directives, .. } if directives.hidden));
    }

    #[test]
    fn multiple_hide_directives() {
        let src = "<!-- hide -->\n```rustlab\na = 1\n```\n\n<!-- hide -->\n```rustlab\nb = 2\n```\n\n```rustlab\nc = 3\n```";
        let blocks = parse_notebook(src);
        assert!(matches!(&blocks[0], Block::Code { directives, .. } if directives.hidden));
        assert!(matches!(&blocks[1], Block::Code { directives, .. } if directives.hidden));
        assert!(matches!(&blocks[2], Block::Code { directives, .. } if !directives.hidden));
    }

    #[test]
    fn rustlab_fence_with_trailing_text() {
        let src = "```rustlab ignore\nx = 1\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Code { source, .. } if source == "x = 1"));
    }

    #[test]
    fn frontmatter_no_closing() {
        let src = "---\ntitle: Test\nno closing\n# Heading";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        if let Block::Markdown(md) = &blocks[0] {
            assert!(
                md.contains("---"),
                "should preserve content when frontmatter unclosed"
            );
        }
    }

    // ── extract_frontmatter / parse_frontmatter ────────────────────────────

    #[test]
    fn extract_frontmatter_parses_title() {
        let src = "---\ntitle: Hello\n---\n# Body\n";
        let (fm, body) = extract_frontmatter(src);
        assert_eq!(fm.title.as_deref(), Some("Hello"));
        assert_eq!(fm.order, None);
        assert_eq!(body, "# Body\n");
    }

    #[test]
    fn extract_frontmatter_parses_order_and_title() {
        let src = "---\ntitle: Third Chapter\norder: 3\n---\nbody\n";
        let (fm, body) = extract_frontmatter(src);
        assert_eq!(fm.title.as_deref(), Some("Third Chapter"));
        assert_eq!(fm.order, Some(3));
        assert_eq!(body, "body\n");
    }

    #[test]
    fn extract_frontmatter_accepts_weight_alias() {
        let src = "---\nweight: -5\n---\n";
        let (fm, _) = extract_frontmatter(src);
        assert_eq!(fm.order, Some(-5));
    }

    #[test]
    fn extract_frontmatter_unquotes_double_quotes() {
        let src = "---\ntitle: \"Quoted: with colon\"\n---\n";
        let (fm, _) = extract_frontmatter(src);
        assert_eq!(fm.title.as_deref(), Some("Quoted: with colon"));
    }

    #[test]
    fn extract_frontmatter_unquotes_single_quotes() {
        let src = "---\ntitle: 'Single Quoted'\n---\n";
        let (fm, _) = extract_frontmatter(src);
        assert_eq!(fm.title.as_deref(), Some("Single Quoted"));
    }

    #[test]
    fn extract_frontmatter_ignores_unknown_keys() {
        let src = "---\nauthor: Alice\ntitle: Known\nfavorite_color: blue\n---\n";
        let (fm, _) = extract_frontmatter(src);
        assert_eq!(fm.title.as_deref(), Some("Known"));
        // No panic / silent drop of unknown keys.
    }

    #[test]
    fn extract_frontmatter_no_block_returns_original_source() {
        let src = "# Just a Heading\n\nNo frontmatter here.\n";
        let (fm, body) = extract_frontmatter(src);
        assert_eq!(fm, Frontmatter::default());
        assert_eq!(body, src);
    }

    #[test]
    fn extract_frontmatter_rejects_horizontal_rule() {
        // A bare `---` followed by content on the same line is NOT frontmatter.
        let src = "--- not frontmatter ---\ntext\n";
        let (fm, body) = extract_frontmatter(src);
        assert_eq!(fm, Frontmatter::default());
        assert_eq!(body, src);
    }

    #[test]
    fn extract_frontmatter_unclosed_is_not_stripped() {
        let src = "---\ntitle: X\n(no closing)\n";
        let (fm, body) = extract_frontmatter(src);
        assert_eq!(
            fm,
            Frontmatter::default(),
            "unclosed block must not be parsed"
        );
        assert_eq!(body, src);
    }

    #[test]
    fn extract_frontmatter_ignores_comments() {
        let src = "---\n# comment line\ntitle: T\n# another\norder: 2\n---\n";
        let (fm, _) = extract_frontmatter(src);
        assert_eq!(fm.title.as_deref(), Some("T"));
        assert_eq!(fm.order, Some(2));
    }

    #[test]
    fn extract_frontmatter_non_integer_order_ignored() {
        let src = "---\norder: abc\n---\n";
        let (fm, _) = extract_frontmatter(src);
        assert_eq!(fm.order, None);
    }

    #[test]
    fn multiline_code_block() {
        let src = "```rustlab\nline1\nline2\nline3\n```";
        let blocks = parse_notebook(src);
        assert!(
            matches!(&blocks[0], Block::Code { source, .. } if source == "line1\nline2\nline3")
        );
    }

    // ── New directive tests ──

    #[test]
    fn details_directive() {
        let src = "<!-- details: Show Plots -->\n```rustlab\nx = 1\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Code { directives, .. }
            if directives.details.as_deref() == Some("Show Plots")));
    }

    #[test]
    fn grid_directive() {
        let src = "<!-- grid: 3 -->\n```rustlab\nx = 1\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Code { directives, .. }
            if directives.grid_cols == Some(3)));
    }

    #[test]
    fn stacked_directives() {
        let src =
            "<!-- hide -->\n<!-- grid: 2 -->\n<!-- details: Gallery -->\n```rustlab\nx = 1\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        if let Block::Code { directives, .. } = &blocks[0] {
            assert!(directives.hidden);
            assert_eq!(directives.grid_cols, Some(2));
            assert_eq!(directives.details.as_deref(), Some("Gallery"));
        } else {
            panic!("expected Code block");
        }
    }

    #[test]
    fn callout_note() {
        let src = "Intro\n\n<!-- note -->\nThis is a note.\n\nMore text.";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], Block::Markdown(s) if s.contains("Intro")));
        assert!(
            matches!(&blocks[1], Block::Callout { kind: CalloutKind::Note, content, .. }
            if content == "This is a note.")
        );
        assert!(matches!(&blocks[2], Block::Markdown(s) if s.contains("More text")));
    }

    #[test]
    fn callout_multiline_with_close() {
        let src = "<!-- tip -->\nFirst line.\nSecond line.\n<!-- /tip -->\n\nAfter.";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 2);
        assert!(
            matches!(&blocks[0], Block::Callout { kind: CalloutKind::Tip, content, .. }
            if content.contains("First line.") && content.contains("Second line."))
        );
        assert!(matches!(&blocks[1], Block::Markdown(s) if s.contains("After")));
    }

    #[test]
    fn callout_ends_at_heading() {
        let src = "<!-- warning -->\nDanger!\n# Next Section";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 2);
        assert!(
            matches!(&blocks[0], Block::Callout { kind: CalloutKind::Warning, content, .. }
            if content == "Danger!")
        );
        assert!(matches!(&blocks[1], Block::Markdown(s) if s.contains("Next Section")));
    }

    #[test]
    fn exercise_and_solution() {
        let src = "<!-- exercise -->\nWhat is 2+2?\n\n<!-- solution -->\nThe answer is 4.\n";
        let blocks = parse_notebook(src);
        assert!(matches!(&blocks[0], Block::ExerciseStart));
        assert!(matches!(&blocks[1], Block::Markdown(s) if s.contains("What is 2+2?")));
        assert!(matches!(&blocks[2], Block::SolutionStart));
        assert!(matches!(&blocks[3], Block::Markdown(s) if s.contains("answer is 4")));
    }

    #[test]
    fn exercise_with_code_in_solution() {
        let src = "<!-- exercise -->\nCompute x.\n\n<!-- solution -->\nHere it is:\n\n```rustlab\nx = 42\n```";
        let blocks = parse_notebook(src);
        assert!(matches!(&blocks[0], Block::ExerciseStart));
        assert!(matches!(&blocks[1], Block::Markdown(s) if s.contains("Compute x")));
        assert!(matches!(&blocks[2], Block::SolutionStart));
        // Solution contains markdown + code
        assert!(matches!(&blocks[3], Block::Markdown(s) if s.contains("Here it is")));
        assert!(matches!(&blocks[4], Block::Code { source, .. } if source == "x = 42"));
    }

    // ── Mermaid blocks ──

    #[test]
    fn mermaid_block_basic() {
        let src = "Diagram:\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\nAfter.";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], Block::Markdown(s) if s.contains("Diagram:")));
        assert!(matches!(&blocks[1], Block::Mermaid { source, .. }
            if source == "flowchart LR\n  A --> B"));
        assert!(matches!(&blocks[2], Block::Markdown(s) if s.contains("After")));
    }

    #[test]
    fn mermaid_with_hide() {
        let src = "<!-- hide -->\n```mermaid\ngraph TD\nA-->B\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Mermaid { directives, .. } => assert!(directives.hidden),
            _ => panic!("expected Mermaid"),
        }
    }

    #[test]
    fn mermaid_with_details() {
        let src = "<!-- details: Architecture -->\n```mermaid\ngraph TD\nA-->B\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Mermaid { directives, .. } => {
                assert_eq!(directives.details.as_deref(), Some("Architecture"));
            }
            _ => panic!("expected Mermaid"),
        }
    }

    #[test]
    fn mermaid_with_caption() {
        let src = "<!-- caption: Signal flow -->\n```mermaid\nflowchart LR\nA-->B\n```";
        let blocks = parse_notebook(src);
        match &blocks[0] {
            Block::Mermaid { directives, .. } => {
                assert_eq!(directives.caption.as_deref(), Some("Signal flow"));
            }
            _ => panic!("expected Mermaid"),
        }
    }

    #[test]
    fn caption_then_rustlab_populates_code_directives() {
        let src = "<!-- caption: My Plot -->\n```rustlab\nplot(1:10)\n```";
        let blocks = parse_notebook(src);
        match &blocks[0] {
            Block::Code { directives, .. } => {
                assert_eq!(directives.caption.as_deref(), Some("My Plot"));
            }
            _ => panic!("expected Code"),
        }
    }

    #[test]
    fn mermaid_after_rustlab_no_fence_leak() {
        let src = "```rustlab\nx = 1\n```\n\n```mermaid\ngraph TD\nA-->B\n```";
        let blocks = parse_notebook(src);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], Block::Code { source, .. } if source == "x = 1"));
        assert!(matches!(&blocks[1], Block::Mermaid { source, .. }
            if source == "graph TD\nA-->B"));
    }

    #[test]
    fn mermaid_special_chars_round_trip() {
        let src = "```mermaid\nA & <B> --> C\n```";
        let blocks = parse_notebook(src);
        match &blocks[0] {
            Block::Mermaid { source, .. } => assert_eq!(source, "A & <B> --> C"),
            _ => panic!("expected Mermaid"),
        }
    }

    // ── GitHub / Obsidian-native callouts (Phase A) ──

    #[test]
    fn parse_callout_github_note_basic() {
        let src = "Intro.\n\n> [!NOTE]\n> Pay attention here.\n\nAfter.";
        let blocks = parse_notebook(src);
        assert!(matches!(
            &blocks[1],
            Block::Callout {
                kind: CalloutKind::Note,
                title: None,
                content,
            } if content == "Pay attention here."
        ));
    }

    #[test]
    fn parse_callout_github_with_inline_title() {
        let src = "> [!TIP] Heads up\n> Increase nfft.";
        let blocks = parse_notebook(src);
        match &blocks[0] {
            Block::Callout {
                kind: CalloutKind::Tip,
                title,
                content,
            } => {
                assert_eq!(title.as_deref(), Some("Heads up"));
                assert_eq!(content, "Increase nfft.");
            }
            _ => panic!("expected Tip callout"),
        }
    }

    #[test]
    fn parse_callout_github_foldable_suffix() {
        // `+` (open by default) and `-` (collapsed) are silently consumed.
        let src = "> [!WARNING]+\n> Body.";
        let blocks = parse_notebook(src);
        assert!(matches!(
            &blocks[0],
            Block::Callout { kind: CalloutKind::Warning, .. }
        ));

        let src2 = "> [!IMPORTANT]- Title\n> Body.";
        let blocks2 = parse_notebook(src2);
        match &blocks2[0] {
            Block::Callout {
                kind: CalloutKind::Important,
                title,
                ..
            } => assert_eq!(title.as_deref(), Some("Title")),
            _ => panic!("expected Important callout with title"),
        }
    }

    #[test]
    fn parse_callout_github_multiline_body() {
        let src = "> [!CAUTION]\n> First line.\n> Second line.\n>\n> Third line.";
        let blocks = parse_notebook(src);
        match &blocks[0] {
            Block::Callout {
                kind: CalloutKind::Caution,
                content,
                ..
            } => {
                assert!(content.contains("First line."));
                assert!(content.contains("Second line."));
                assert!(content.contains("Third line."));
            }
            _ => panic!("expected Caution callout"),
        }
    }

    #[test]
    fn parse_callout_github_aliases() {
        // `INFO` → Note, `HINT` → Tip, `DANGER` → Caution. Case-insensitive.
        let src = "> [!info]\n> a\n\n> [!Hint]\n> b\n\n> [!danger]\n> c";
        let blocks = parse_notebook(src);
        let kinds: Vec<_> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Callout { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            vec![CalloutKind::Note, CalloutKind::Tip, CalloutKind::Caution]
        );
    }

    #[test]
    fn parse_callout_unknown_type_left_as_blockquote() {
        // `> [!FOO]` doesn't match a known kind — falls through as plain
        // markdown (the renderer emits it as a blockquote).
        let src = "> [!FOO]\n> body.";
        let blocks = parse_notebook(src);
        assert!(blocks.iter().all(|b| !matches!(b, Block::Callout { .. })));
        assert!(matches!(&blocks[0], Block::Markdown(s) if s.contains("[!FOO]")));
    }

    #[test]
    fn parse_callout_legacy_html_comment_still_works() {
        // `<!-- note -->` continues to parse — soft-deprecated, not removed.
        let src = "<!-- note -->\nLegacy still works.";
        let blocks = parse_notebook(src);
        assert!(matches!(
            &blocks[0],
            Block::Callout {
                kind: CalloutKind::Note,
                title: None,
                content,
            } if content == "Legacy still works."
        ));
    }

    // ── replace_code_block_source (inline cell write-back) ──

    const SPLICE_SRC: &str = "\
---
title: fence ``` inside frontmatter
---

# Doc

```rustlab
a = 1;
```

prose between

```rustlab
b = 2;
```

```mermaid
A-->B
```

tail prose
";

    #[test]
    fn splice_replaces_only_the_target_block() {
        let out = replace_code_block_source(SPLICE_SRC, 1, "b = 999;").unwrap();
        // Target block replaced…
        assert!(out.contains("b = 999;"));
        assert!(!out.contains("b = 2;"));
        // …and every other byte identical: frontmatter, first block,
        // prose, mermaid, tail.
        assert!(out.contains("title: fence ``` inside frontmatter"));
        assert!(out.contains("a = 1;"));
        assert!(out.contains("prose between"));
        assert!(out.contains("A-->B"));
        assert!(out.contains("tail prose"));
        // Parse-equivalence: same block structure, target updated.
        let before = parse_notebook(SPLICE_SRC);
        let after = parse_notebook(&out);
        assert_eq!(before.len(), after.len());
        for (i, (b, a)) in before.iter().zip(after.iter()).enumerate() {
            if i == 3 {
                // markdown, code, markdown, code — target is index 3.
                assert!(matches!(a, Block::Code { source, .. } if source == "b = 999;"));
            } else {
                assert_eq!(b, a, "non-target block {i} must be untouched");
            }
        }
    }

    #[test]
    fn splice_identity_is_parse_equivalent() {
        let before = parse_notebook(SPLICE_SRC);
        let out = replace_code_block_source(SPLICE_SRC, 0, "a = 1;").unwrap();
        assert_eq!(before, parse_notebook(&out), "same-source splice is a parse no-op");
    }

    #[test]
    fn splice_out_of_range_returns_none() {
        assert!(replace_code_block_source(SPLICE_SRC, 2, "x").is_none());
        assert!(replace_code_block_source("no fences at all", 0, "x").is_none());
    }

    #[test]
    fn splice_ignores_rustlab_lines_inside_other_fences() {
        // The ```rustlab-looking lines inside widget fences are body
        // text, not openers — the fence ordinal must skip them (mirrors
        // parse_notebook, which is in in_widget state there).
        let src = "\
```rustlab-widget
name = \"g\"
type = \"slider\"
min = 0
max = 1
default = 0.5
```

```rustlab
real = 1;
```
";
        let out = replace_code_block_source(src, 0, "real = 2;").unwrap();
        assert!(out.contains("real = 2;"));
        assert!(out.contains("name = \"g\""), "widget body untouched");
        let after = parse_notebook(&out);
        assert!(
            after.iter().any(|b| matches!(b, Block::Code { source, .. } if source == "real = 2;")),
        );
        assert!(after.iter().any(|b| matches!(b, Block::Widget { .. })));
    }

    #[test]
    fn splice_round_trips_trailing_newline_bodies() {
        let src = "```rustlab\nold\n```\n";
        // Body WITH a trailing blank line…
        let out = replace_code_block_source(src, 0, "new\n").unwrap();
        let blocks = parse_notebook(&out);
        assert!(matches!(&blocks[0], Block::Code { source, .. } if source == "new\n"));
        // …and without.
        let out = replace_code_block_source(src, 0, "new").unwrap();
        let blocks = parse_notebook(&out);
        assert!(matches!(&blocks[0], Block::Code { source, .. } if source == "new"));
    }

    #[test]
    fn splice_handles_unclosed_final_fence() {
        let src = "# T\n\n```rustlab\nold body";
        let out = replace_code_block_source(src, 0, "new body").unwrap();
        let blocks = parse_notebook(&out);
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, Block::Code { source, .. } if source == "new body")),
            "unclosed final fence must still be spliceable: {out:?}"
        );
    }

    /// Property check over the real example notebooks: splicing a marker
    /// into every code ordinal of every notebook leaves all other blocks
    /// parse-identical and puts exactly the marker at the target.
    #[test]
    fn splice_round_trips_across_example_notebooks() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/notebooks");
        let marker = "SPLICE_PROBE = 424242;";
        let mut checked_blocks = 0usize;
        for entry in std::fs::read_dir(dir).expect("examples/notebooks missing") {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "md") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            let before = parse_notebook(&src);
            let code_count = before
                .iter()
                .filter(|b| matches!(b, Block::Code { .. }))
                .count();
            for j in 0..code_count {
                let out = replace_code_block_source(&src, j, marker)
                    .unwrap_or_else(|| panic!("splice failed: {} #{j}", path.display()));
                let after = parse_notebook(&out);
                assert_eq!(
                    before.len(),
                    after.len(),
                    "block count changed: {} #{j}",
                    path.display()
                );
                let mut code_seen = 0usize;
                for (b, a) in before.iter().zip(after.iter()) {
                    if matches!(b, Block::Code { .. }) {
                        if code_seen == j {
                            assert!(
                                matches!(a, Block::Code { source, .. } if source == marker),
                                "target not replaced: {} #{j}",
                                path.display()
                            );
                        } else {
                            assert_eq!(b, a, "other code block moved: {} #{j}", path.display());
                        }
                        code_seen += 1;
                    } else {
                        assert_eq!(b, a, "non-code block changed: {} #{j}", path.display());
                    }
                }
                checked_blocks += 1;
            }
        }
        assert!(
            checked_blocks > 50,
            "expected to exercise many example blocks, got {checked_blocks}"
        );
    }
}
