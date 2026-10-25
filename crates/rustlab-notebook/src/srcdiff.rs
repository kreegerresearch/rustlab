//! The Source view of the `watch` git diff: a unified diff of the
//! notebook's `.md`, read-only, with old and new line numbers, word
//! marks inside changed lines, and each hunk named after the nearest
//! heading above it.

use crate::render::{escape_html, highlight_marked};
use crate::worddiff::{self, Mode, MIN_SURVIVAL};

/// Lines of context around each change.
const CONTEXT: usize = 3;
/// Edit distance at which the line diff stops searching and treats the
/// rest of the middle as replaced. Keeps memory and time bounded.
const MAX_D: usize = 2000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Line {
    /// Old index, new index.
    Same(usize, usize),
    Del(usize),
    Ins(usize),
}

fn lines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let body = s.strip_suffix('\n').unwrap_or(s);
    body.split('\n').collect()
}

/// Line diff: lines that occur exactly once on each side are matched
/// first (patience anchoring, as `git diff --patience` does), then Myers'
/// O((N+M)D) diff fills the gaps between anchors. Without the anchors a
/// minimal diff likes to match blank lines, which pairs an unrelated
/// heading with a removed one.
pub(crate) fn line_diff(a: &[&str], b: &[&str]) -> Vec<Line> {
    let mut out = Vec::with_capacity(a.len().max(b.len()));
    diff_range(a, b, 0, a.len(), 0, b.len(), &mut out, 0);
    out
}

#[allow(clippy::too_many_arguments)]
fn diff_range(
    a: &[&str],
    b: &[&str],
    mut a0: usize,
    mut a1: usize,
    mut b0: usize,
    mut b1: usize,
    out: &mut Vec<Line>,
    depth: usize,
) {
    while a0 < a1 && b0 < b1 && a[a0] == b[b0] {
        out.push(Line::Same(a0, b0));
        a0 += 1;
        b0 += 1;
    }
    let mut tail = Vec::new();
    while a1 > a0 && b1 > b0 && a[a1 - 1] == b[b1 - 1] {
        a1 -= 1;
        b1 -= 1;
        tail.push(Line::Same(a1, b1));
    }
    let anchors = if depth < 64 {
        unique_anchors(a, b, a0, a1, b0, b1)
    } else {
        Vec::new()
    };
    if anchors.is_empty() {
        let (ma, mb) = (&a[a0..a1], &b[b0..b1]);
        match myers(ma, mb) {
            Some(mid) => out.extend(mid.into_iter().map(|l| match l {
                Line::Same(i, j) => Line::Same(i + a0, j + b0),
                Line::Del(i) => Line::Del(i + a0),
                Line::Ins(j) => Line::Ins(j + b0),
            })),
            None => {
                out.extend((a0..a1).map(Line::Del));
                out.extend((b0..b1).map(Line::Ins));
            }
        }
    } else {
        let (mut pa, mut pb) = (a0, b0);
        for (i, j) in anchors {
            diff_range(a, b, pa, i, pb, j, out, depth + 1);
            out.push(Line::Same(i, j));
            pa = i + 1;
            pb = j + 1;
        }
        diff_range(a, b, pa, a1, pb, b1, out, depth + 1);
    }
    out.extend(tail.into_iter().rev());
}

/// Lines unique to both ranges, as the longest run that keeps their order
/// on both sides (patience sorting). Blank lines never anchor.
fn unique_anchors(
    a: &[&str],
    b: &[&str],
    a0: usize,
    a1: usize,
    b0: usize,
    b1: usize,
) -> Vec<(usize, usize)> {
    use std::collections::HashMap;
    let mut count: HashMap<&str, (usize, usize, usize, usize)> = HashMap::new();
    for (i, l) in a.iter().enumerate().take(a1).skip(a0) {
        let e = count.entry(l).or_insert((0, 0, 0, 0));
        e.0 += 1;
        e.2 = i;
    }
    for (j, l) in b.iter().enumerate().take(b1).skip(b0) {
        if let Some(e) = count.get_mut(l) {
            e.1 += 1;
            e.3 = j;
        }
    }
    let mut pairs: Vec<(usize, usize)> = count
        .iter()
        .filter(|(l, c)| c.0 == 1 && c.1 == 1 && !l.trim().is_empty())
        .map(|(_, c)| (c.2, c.3))
        .collect();
    pairs.sort_unstable();
    // Longest increasing subsequence on the new-side index.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; pairs.len()];
    for (k, &(_, j)) in pairs.iter().enumerate() {
        let pos = tails.partition_point(|&t| pairs[t].1 < j);
        if pos > 0 {
            prev[k] = Some(tails[pos - 1]);
        }
        if pos == tails.len() {
            tails.push(k);
        } else {
            tails[pos] = k;
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied();
    while let Some(k) = at {
        out.push(pairs[k]);
        at = prev[k];
    }
    out.reverse();
    out
}

fn myers(a: &[&str], b: &[&str]) -> Option<Vec<Line>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    if max == 0 {
        return Some(Vec::new());
    }
    let off = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    // Each step keeps only the frontier it can reach (`-d..=d`), so the
    // trace is O(D²), not O(D·(N+M)).
    'outer: for d in 0..=max.min(MAX_D) as isize {
        trace.push(v[(off - d) as usize..=(off + d) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let idx = (k + off) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                v[idx + 1]
            } else {
                v[idx - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[idx] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'outer;
            }
            k += 2;
        }
    }
    found?;
    // Walk the saved frontiers back from (n, m).
    let mut out = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..trace.len() as isize).rev() {
        if d == 0 {
            // The start: only the snake from (0, 0) is left.
            while x > 0 && y > 0 {
                x -= 1;
                y -= 1;
                out.push(Line::Same(x as usize, y as usize));
            }
            break;
        }
        let row = &trace[d as usize];
        let at = |k: isize| row[(k + d) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = at(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            out.push(Line::Same(x as usize, y as usize));
        }
        if d > 0 {
            if x == prev_x {
                y -= 1;
                out.push(Line::Ins(y as usize));
            } else {
                x -= 1;
                out.push(Line::Del(x as usize));
            }
        }
    }
    out.reverse();
    Some(out)
}

/// The nearest heading at or above each line, skipping fenced code.
fn headings(src: &[&str]) -> Vec<Option<String>> {
    let mut out = Vec::with_capacity(src.len());
    let mut fence: Option<String> = None;
    let mut cur: Option<String> = None;
    for line in src {
        let t = line.trim_start();
        let marker: String = t.chars().take_while(|&c| c == '`' || c == '~').collect();
        if marker.len() >= 3 && (marker.chars().all(|c| c == '`') || marker.chars().all(|c| c == '~')) {
            match &fence {
                Some(open) if marker.starts_with(open.as_str()) => fence = None,
                None => fence = Some(marker),
                _ => {}
            }
        } else if fence.is_none() {
            let hashes = t.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
                let title = t[hashes..].trim().trim_end_matches('#').trim();
                if !title.is_empty() {
                    cur = Some(title.chars().take(60).collect());
                }
            }
        }
        out.push(cur.clone());
    }
    out
}

/// The Source view body for `<main>`: a header line, then each hunk.
/// `label` names the file and comparison (`filters.md · uncommitted
/// changes · 4 changes`).
pub(crate) fn source_view(old: &str, new_src: &str, label: &str) -> String {
    let a = lines(old);
    let b = lines(new_src);
    let ops = line_diff(&a, &b);
    let heads_new = headings(&b);
    let heads_old = headings(&a);
    let mut html = format!(
        "<div class=\"rl-dx-srcview\"><div class=\"rl-dx-fh\">{}</div>\n",
        escape_html(label)
    );
    // Hunk ranges over `ops`: each change plus CONTEXT lines either side,
    // merged when they touch.
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, l)| !matches!(l, Line::Same(..)))
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        html.push_str("<p class=\"rl-dx-note rl-dx-none\">No changes.</p>\n</div>\n");
        return html;
    }
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for &i in &changed {
        let lo = i.saturating_sub(CONTEXT);
        let hi = (i + CONTEXT + 1).min(ops.len());
        match hunks.last_mut() {
            Some(h) if lo <= h.1 => h.1 = hi,
            _ => hunks.push((lo, hi)),
        }
    }
    for (lo, hi) in hunks {
        let slice = &ops[lo..hi];
        // Name the hunk after the heading above its first change.
        let first = slice
            .iter()
            .find(|l| !matches!(l, Line::Same(..)))
            .copied()
            .unwrap_or(slice[0]);
        let head = match first {
            Line::Same(_, j) | Line::Ins(j) => heads_new.get(j).cloned().flatten(),
            Line::Del(i) => heads_old.get(i).cloned().flatten(),
        };
        let place = head.unwrap_or_else(|| "Top of file".into());
        let has_del = slice.iter().any(|l| matches!(l, Line::Del(_)));
        let has_ins = slice.iter().any(|l| matches!(l, Line::Ins(_)));
        let (state, class) = match (has_del, has_ins) {
            (true, false) => ("removed", "rl-dx-del"),
            (false, true) => ("added", "rl-dx-add"),
            _ => ("modified", "rl-dx-mod"),
        };
        html.push_str(&format!(
            "<div class=\"rl-dx-hunk rl-dx {class}\" data-rl-dx=\"{state}\" data-rl-where=\"{p}\">@@ {p} @@</div>\n",
            p = escape_html(&place)
        ));
        let mut k = 0;
        while k < slice.len() {
            if let Line::Same(i, j) = slice[k] {
                html.push_str(&row(Some(i + 1), Some(j + 1), "", "", &escape_html(a[i])));
                k += 1;
                continue;
            }
            let mut dels = Vec::new();
            let mut ins = Vec::new();
            while k < slice.len() && !matches!(slice[k], Line::Same(..)) {
                match slice[k] {
                    Line::Del(i) => dels.push(i),
                    Line::Ins(j) => ins.push(j),
                    Line::Same(..) => {}
                }
                k += 1;
            }
            let mut old_marks: Vec<Vec<std::ops::Range<usize>>> = vec![Vec::new(); dels.len()];
            let mut new_marks: Vec<Vec<std::ops::Range<usize>>> = vec![Vec::new(); ins.len()];
            for p in 0..dels.len().min(ins.len()) {
                if let Some(d) = worddiff::diff(a[dels[p]], b[ins[p]], Mode::Prose) {
                    if d.survival >= MIN_SURVIVAL {
                        old_marks[p] = d.old_marks;
                        new_marks[p] = d.new_marks;
                    }
                }
            }
            for (p, &i) in dels.iter().enumerate() {
                let text = highlight_marked(a[i], false, &old_marks[p], "del");
                html.push_str(&row(Some(i + 1), None, "rl-dx-l-del", "−", &text));
            }
            for (p, &j) in ins.iter().enumerate() {
                let text = highlight_marked(b[j], false, &new_marks[p], "ins");
                html.push_str(&row(None, Some(j + 1), "rl-dx-l-add", "+", &text));
            }
        }
    }
    html.push_str("</div>\n");
    html
}

fn row(old: Option<usize>, new: Option<usize>, class: &str, sign: &str, text: &str) -> String {
    let n = |v: Option<usize>| v.map(|n| n.to_string()).unwrap_or_default();
    format!(
        "<div class=\"rl-dx-l {class}\"><span class=\"rl-dx-n\">{}</span><span class=\"rl-dx-n\">{}</span><span class=\"rl-dx-s\" aria-hidden=\"true\">{sign}</span><span class=\"rl-dx-x\">{text}</span></div>\n",
        n(old),
        n(new)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(a: &[&str], b: &[&str], ops: &[Line]) -> (Vec<String>, Vec<String>) {
        let mut old = Vec::new();
        let mut new = Vec::new();
        for l in ops {
            match *l {
                Line::Same(i, j) => {
                    assert_eq!(a[i], b[j]);
                    old.push(a[i].to_string());
                    new.push(b[j].to_string());
                }
                Line::Del(i) => old.push(a[i].to_string()),
                Line::Ins(j) => new.push(b[j].to_string()),
            }
        }
        (old, new)
    }

    #[test]
    fn line_diff_reproduces_both_sides_and_is_minimal() {
        let a = ["a", "b", "c", "d", "e", "f"];
        let b = ["a", "x", "c", "d", "f", "g"];
        let ops = line_diff(&a, &b);
        let (old, new) = apply(&a, &b, &ops);
        assert_eq!(old, a);
        assert_eq!(new, b);
        let edits = ops.iter().filter(|l| !matches!(l, Line::Same(..))).count();
        assert_eq!(edits, 4, "{ops:?}");
        // A unique heading anchors, so the removed section is one block
        // instead of matching its blank lines.
        let a = ["## Old", "", "gone", "", "## Summary", "", "kept"];
        let b = ["## Summary", "", "new", "", "kept"];
        let ops = line_diff(&a, &b);
        assert!(ops.contains(&Line::Same(4, 0)), "{ops:?}");
        let (old, new) = apply(&a, &b, &ops);
        assert_eq!((old.len(), new.len()), (a.len(), b.len()));
        assert_eq!(line_diff(&[], &["x"]), vec![Line::Ins(0)]);
        assert_eq!(line_diff(&["x"], &[]), vec![Line::Del(0)]);
        assert!(line_diff(&a, &a).iter().all(|l| matches!(l, Line::Same(..))));
    }

    /// Many small random edits: the diff must always rebuild both sides,
    /// and never use more edits than delete-all plus insert-all.
    #[test]
    fn line_diff_round_trips_random_edits() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rnd = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let words = ["a", "b", "c", "d", "e", "f", "g"];
        for _ in 0..300 {
            let a: Vec<&str> = (0..rnd(20)).map(|_| words[rnd(7) as usize]).collect();
            let mut b = a.clone();
            for _ in 0..rnd(6) {
                match rnd(3) {
                    0 if !b.is_empty() => {
                        let at = rnd(b.len() as u64) as usize;
                        b.remove(at);
                    }
                    1 => {
                        let at = rnd(b.len() as u64 + 1) as usize;
                        b.insert(at, words[rnd(7) as usize]);
                    }
                    _ if !b.is_empty() => {
                        let at = rnd(b.len() as u64) as usize;
                        b[at] = words[rnd(7) as usize];
                    }
                    _ => {}
                }
            }
            let ops = line_diff(&a, &b);
            let (old, new) = apply(&a, &b, &ops);
            assert_eq!(old, a);
            assert_eq!(new, b);
            // Myers on its own is minimal: exactly n + m − 2·LCS edits.
            let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
            for i in 0..a.len() {
                for j in 0..b.len() {
                    dp[i + 1][j + 1] = if a[i] == b[j] {
                        dp[i][j] + 1
                    } else {
                        dp[i][j + 1].max(dp[i + 1][j])
                    };
                }
            }
            let raw = myers(&a, &b).unwrap();
            let (old, new) = apply(&a, &b, &raw);
            assert_eq!((old, new), (a.iter().map(|s| s.to_string()).collect::<Vec<_>>(), b.iter().map(|s| s.to_string()).collect::<Vec<_>>()));
            let edits = raw.iter().filter(|l| !matches!(l, Line::Same(..))).count();
            assert_eq!(edits, a.len() + b.len() - 2 * dp[a.len()][b.len()], "{a:?} {b:?}");
        }
    }

    #[test]
    fn source_view_names_hunks_and_marks_words() {
        let old = "# T\n\n## Response\n\nAbout 40 dB.\n\n## Old\n\nGone.\n\n## Summary\n\nKept.\n";
        let new = "# T\n\n## Response\n\nAbout 53 dB.\n\n## Summary\n\nKept.\n";
        let html = source_view(old, new, "n.md · uncommitted changes");
        assert!(html.contains("n.md · uncommitted changes"), "{html}");
        assert!(html.contains("@@ Response @@"), "{html}");
        assert!(html.contains("<del class=\"rl-w\">40</del>"), "{html}");
        assert!(html.contains("<ins class=\"rl-w\">53</ins>"), "{html}");
        assert!(html.contains("data-rl-dx=\"modified\""), "{html}");
        // Old line 5 is gone, new line 5 replaces it.
        assert!(html.contains("<span class=\"rl-dx-n\">5</span><span class=\"rl-dx-n\"></span>"), "{html}");
        assert!(html.contains("Gone."), "{html}");
        assert!(source_view(new, new, "x").contains("No changes."));
    }

    #[test]
    fn headings_skip_fenced_code() {
        let src = ["# A", "```rustlab", "# not a heading", "```", "x"];
        let h = headings(&src);
        assert_eq!(h[4].as_deref(), Some("A"));
    }
}
