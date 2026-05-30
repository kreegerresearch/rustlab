//! Phase 3: per-block diff between two renders of the same
//! notebook.
//!
//! The renderer wraps every diffable block in
//! `<section class="rl-block" id="b-<hash>">…</section>` (see
//! `render::finalize_block`). This module:
//!
//! 1. [`split_blocks`] scans a full rendered document and returns a
//!    `Vec<Block>` of `(id, html)` pairs in source order. The
//!    `html` field includes the wrapping `<section>`.
//! 2. [`compute_changes`] walks two such vecs *pairwise by
//!    position*. It does not key by `id`, because a content edit
//!    changes the content-hash id and an id-keyed diff would see
//!    "remove old + insert new" instead of "replace at position N"
//!    — making every prose tweak look like a structural change.
//!    Position-based addressing keeps the partial payload tight
//!    for the common edit-in-place case.
//! 3. [`classify`] decides whether the broadcast should be `None`,
//!    a `Partial` (carry per-position replacements), or `Full`
//!    (block count changed → structural edit, or the change ratio
//!    is high enough that a full refresh is cheaper).
//! 4. [`partial_envelope`] frames the per-position replacements
//!    as the Phase-3 `{"kind":"partial","blocks":[…]}` WebSocket
//!    message. The client replaces by
//!    `document.querySelectorAll("section.rl-block")[position]
//!    .outerHTML = html`.

/// One block extracted from a rendered document. `id` is the
/// `b-<8-hex-chars>[-N]` identifier the renderer emitted; `html`
/// is the full `<section class="rl-block" id="…">…</section>`
/// substring (so the client can replace by `outerHTML`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: String,
    pub html: String,
}

/// Marker that opens every block-wrapping `<section>`. Keep in
/// lockstep with `render::finalize_block` — if that helper's
/// wrapper format changes, this prefix must change with it.
const SECTION_OPEN_PREFIX: &str = "<section class=\"rl-block\" id=\"";
const SECTION_CLOSE: &str = "</section>";

/// Scan a rendered HTML document for every
/// `<section class="rl-block" id="b-…">…</section>` block. Each
/// returned `Block` carries the full section (open tag through
/// close tag) as its `html`, so a client can replace by
/// `outerHTML`.
///
/// The scanner tracks `<section>` open/close depth so nested
/// `<section>` elements inside a block's content (very rare —
/// our renderer doesn't emit them, but pulldown-cmark could in
/// principle through raw HTML) don't trip a premature close.
pub fn split_blocks(html: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let bytes = html.as_bytes();
    let mut cursor = 0;

    while let Some(rel) = html[cursor..].find(SECTION_OPEN_PREFIX) {
        let open_at = cursor + rel;
        // Parse the id between `id="` and the next `">`.
        let id_start = open_at + SECTION_OPEN_PREFIX.len();
        let Some(id_end_rel) = html[id_start..].find("\">") else {
            break;
        };
        let id = html[id_start..id_start + id_end_rel].to_string();
        let after_open_tag = id_start + id_end_rel + 2; // past `">`

        // Walk forward, tracking nesting of `<section`. We started
        // depth=1 (the open we just consumed). Find the matching
        // `</section>`.
        let mut depth: usize = 1;
        let mut scan = after_open_tag;
        let close_end;
        loop {
            // Find the next interesting marker.
            let next_open = find_after(bytes, scan, b"<section");
            let next_close = find_after(bytes, scan, SECTION_CLOSE.as_bytes());
            match (next_open, next_close) {
                (_, None) => {
                    // Malformed input — bail out. Don't emit a
                    // partial block (would confuse the client).
                    return out;
                }
                (Some(o), Some(c)) if o < c => {
                    depth += 1;
                    scan = o + b"<section".len();
                }
                (_, Some(c)) => {
                    depth -= 1;
                    if depth == 0 {
                        close_end = c + SECTION_CLOSE.len();
                        break;
                    } else {
                        scan = c + SECTION_CLOSE.len();
                    }
                }
            }
        }
        let block_html = html[open_at..close_end].to_string();
        out.push(Block { id, html: block_html });
        cursor = close_end;
    }
    out
}

fn find_after(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

/// One position-addressed replacement: the existing block at
/// `position` in source order should be swapped for the new HTML
/// (which includes its own `<section>` wrapper carrying the new
/// content-hash id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDiff {
    pub position: usize,
    pub html: String,
}

/// Compare two block lists *pairwise by position*. Returns:
///
/// - `None` if the block counts differ (structural change —
///   caller should fall back to a full refresh).
/// - `Some(vec)` of per-position replacements otherwise. Empty
///   vec means "no content changed".
pub fn compute_changes(prev: &[Block], new: &[Block]) -> Option<Vec<BlockDiff>> {
    if prev.len() != new.len() {
        return None;
    }
    let mut out = Vec::new();
    for (position, (p, n)) in prev.iter().zip(new.iter()).enumerate() {
        if p.html != n.html {
            out.push(BlockDiff {
                position,
                html: n.html.clone(),
            });
        }
    }
    Some(out)
}

/// When the change ratio (changed blocks / total new blocks)
/// exceeds this, the coordinator should broadcast `kind=full`
/// instead — the partial payload would be larger than a full
/// refresh, and we'd rather pay one big swap than many small ones.
pub const FULL_REFRESH_RATIO: f32 = 0.5;

/// Decide which envelope to send. `None` means "no content
/// change" (skip the broadcast — the next page load will pick up
/// the new HTML via `GET /notebook.html` anyway).
#[derive(Debug, PartialEq, Eq)]
pub enum Broadcast {
    None,
    Full,
    Partial(Vec<BlockDiff>),
}

pub fn classify(prev: &[Block], new: &[Block]) -> Broadcast {
    match compute_changes(prev, new) {
        // Structural change (block count differs) — full refresh.
        None => Broadcast::Full,
        Some(diffs) if diffs.is_empty() => Broadcast::None,
        Some(diffs) if (diffs.len() as f32 / new.len() as f32) >= FULL_REFRESH_RATIO => {
            Broadcast::Full
        }
        Some(diffs) => Broadcast::Partial(diffs),
    }
}

/// Wrap a `Vec<BlockDiff>` in the Phase-3 `{"kind":"partial",…}`
/// envelope. Mirrors `ws::full_envelope` for the other variant.
pub fn partial_envelope(diffs: &[BlockDiff]) -> String {
    let payload: Vec<serde_json::Value> = diffs
        .iter()
        .map(|d| serde_json::json!({ "position": d.position, "html": d.html }))
        .collect();
    serde_json::json!({ "kind": "partial", "blocks": payload }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, html: &str) -> Block {
        Block {
            id: id.to_string(),
            html: html.to_string(),
        }
    }

    fn wrap(id: &str, body: &str) -> String {
        format!("<section class=\"rl-block\" id=\"{id}\">\n{body}\n</section>\n")
    }

    #[test]
    fn split_blocks_extracts_two_sections() {
        let doc = format!(
            "<html><body>{a}{b}</body></html>",
            a = wrap("b-aaaa1111", "<p>one</p>"),
            b = wrap("b-bbbb2222", "<p>two</p>"),
        );
        let blocks = split_blocks(&doc);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].id, "b-aaaa1111");
        assert!(blocks[0].html.contains("<p>one</p>"));
        assert!(blocks[0].html.starts_with("<section class=\"rl-block\""));
        assert!(blocks[0].html.ends_with("</section>"));
        assert_eq!(blocks[1].id, "b-bbbb2222");
    }

    #[test]
    fn split_blocks_handles_nested_section_inside_block() {
        let body = "<section class=\"inner\">nested</section>";
        let doc = wrap("b-nest0000", body);
        let blocks = split_blocks(&doc);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].html.contains("nested"));
        // The outer </section> is the closer; the inner one didn't trip us.
        assert_eq!(blocks[0].html.matches("</section>").count(), 2);
    }

    #[test]
    fn split_blocks_returns_empty_for_html_with_no_blocks() {
        let blocks = split_blocks("<html><body><p>no blocks here</p></body></html>");
        assert!(blocks.is_empty());
    }

    #[test]
    fn compute_changes_finds_modified_block_by_position() {
        let prev = vec![
            block("b-old1", "<section ...>old A</section>"),
            block("b-bbb",  "<section ...>B</section>"),
        ];
        let new = vec![
            // Content edit → new content-hash id, same position.
            block("b-new1", "<section ...>NEW A</section>"),
            block("b-bbb",  "<section ...>B</section>"),
        ];
        let changes = compute_changes(&prev, &new).expect("counts match");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].position, 0);
        assert!(changes[0].html.contains("NEW A"));
    }

    #[test]
    fn compute_changes_returns_none_on_count_mismatch() {
        // Inserted blocks change structure → caller falls back to Full.
        let prev = vec![block("b-a", "<section ...>A</section>")];
        let new = vec![
            block("b-a",   "<section ...>A</section>"),
            block("b-new", "<section ...>NEW</section>"),
        ];
        assert!(compute_changes(&prev, &new).is_none());
    }

    #[test]
    fn compute_changes_empty_when_identical() {
        let blocks = vec![
            block("b-a", "<section ...>A</section>"),
            block("b-b", "<section ...>B</section>"),
        ];
        let diffs = compute_changes(&blocks, &blocks).unwrap();
        assert!(diffs.is_empty());
    }

    #[test]
    fn classify_no_change_returns_none() {
        let blocks = vec![block("b-a", "<section ...>A</section>")];
        assert_eq!(classify(&blocks, &blocks), Broadcast::None);
    }

    #[test]
    fn classify_one_of_three_changes_is_partial() {
        let prev = vec![
            block("b-a", "<section ...>A</section>"),
            block("b-b", "<section ...>B</section>"),
            block("b-c", "<section ...>C</section>"),
        ];
        let new = vec![
            block("b-A2", "<section ...>A!</section>"), // changed
            block("b-b",  "<section ...>B</section>"),
            block("b-c",  "<section ...>C</section>"),
        ];
        match classify(&prev, &new) {
            Broadcast::Partial(diffs) => {
                assert_eq!(diffs.len(), 1);
                assert_eq!(diffs[0].position, 0);
                assert!(diffs[0].html.contains("A!"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn classify_count_mismatch_forces_full() {
        let prev = vec![
            block("b-a", "<section ...>A</section>"),
            block("b-b", "<section ...>B</section>"),
        ];
        let new = vec![block("b-a", "<section ...>A</section>")];
        assert_eq!(classify(&prev, &new), Broadcast::Full);
    }

    #[test]
    fn classify_majority_change_is_full() {
        let prev = vec![
            block("b-a", "<section ...>A</section>"),
            block("b-b", "<section ...>B</section>"),
        ];
        let new = vec![
            block("b-x", "<section ...>X</section>"),
            block("b-y", "<section ...>Y</section>"),
        ];
        assert_eq!(classify(&prev, &new), Broadcast::Full);
    }

    #[test]
    fn partial_envelope_round_trips_through_json() {
        let diffs = vec![BlockDiff { position: 3, html: "<section>A</section>".into() }];
        let env = partial_envelope(&diffs);
        let parsed: serde_json::Value = serde_json::from_str(&env).unwrap();
        assert_eq!(parsed["kind"], "partial");
        let arr = parsed["blocks"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["position"], 3);
        assert_eq!(arr[0]["html"], "<section>A</section>");
    }
}
