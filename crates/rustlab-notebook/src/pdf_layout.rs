//! Geometric check for notebook PDFs.
//!
//! pdflatex is the layout engine. This module reads the page content streams
//! (uncompressed — tests set `\pdfcompresslevel=0`, which does not move any
//! glyph) and rebuilds the boxes TeX actually drew:
//!
//! * a word box for each run of glyphs
//! * an image box for each image XObject
//! * a paint box for each filled or stroked path (table rules, row colours,
//!   code-panel frames)
//!
//! Two word or image boxes overlap when their intersection is more than
//! [`OVERLAP_PT`] on both axes. A fill may contain the words it backs; it is
//! a problem only when it cuts into a word whose centre lies outside the
//! fill. Anything except the centred page number must stay inside the 1 in
//! article margin ([`MARGIN_PT`]), with a few points of slack for a final
//! line's depth (`\maxdepth`) and for stroke width.
//!
//! The same idea as `rustlab-plot`'s label-overlap test: parse the drawn
//! boxes and assert they do not occupy the same region. No PDF library —
//! the tokenizer covers the operators pdfTeX emits.

use std::collections::BTreeMap;

/// Intersection deeper than this on both axes is an overlap.
/// Adjacent lines with a normal baseline skip stay under it; a glyph
/// that sits on another glyph does not.
const OVERLAP_PT: f64 = 1.5;
/// `\geometry{margin=1in}` in the notebook preamble.
const MARGIN_PT: f64 = 72.0;
/// Stroke width and rounding.
const EDGE_SLOP_PT: f64 = 2.0;
/// `\maxdepth` lets the last line's descenders enter the bottom margin.
const BOTTOM_SLOP_PT: f64 = 5.0;

#[derive(Debug, Clone)]
struct Rect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Rect {
    fn from_points(pts: &[(f64, f64)]) -> Option<Self> {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in pts {
            x0 = x0.min(*x);
            y0 = y0.min(*y);
            x1 = x1.max(*x);
            y1 = y1.max(*y);
        }
        if x0 > x1 || y0 > y1 {
            None
        } else {
            Some(Self { x0, y0, x1, y1 })
        }
    }

    fn normalize(mut self) -> Self {
        if self.x0 > self.x1 {
            std::mem::swap(&mut self.x0, &mut self.x1);
        }
        if self.y0 > self.y1 {
            std::mem::swap(&mut self.y0, &mut self.y1);
        }
        self
    }

    fn width(&self) -> f64 {
        self.x1 - self.x0
    }
    fn height(&self) -> f64 {
        self.y1 - self.y0
    }
    fn cx(&self) -> f64 {
        (self.x0 + self.x1) / 2.0
    }
    fn cy(&self) -> f64 {
        (self.y0 + self.y1) / 2.0
    }

    fn intersects(&self, o: &Rect, tol: f64) -> bool {
        let ix = self.x1.min(o.x1) - self.x0.max(o.x0);
        let iy = self.y1.min(o.y1) - self.y0.max(o.y0);
        ix > tol && iy > tol
    }

    fn expand(&self, pad: f64) -> Self {
        Self {
            x0: self.x0 - pad,
            y0: self.y0 - pad,
            x1: self.x1 + pad,
            y1: self.y1 + pad,
        }
    }
}

#[derive(Debug)]
struct Word {
    rect: Rect,
    text: String,
}

#[derive(Debug)]
struct Paint {
    rect: Rect,
    fill: bool,
}

struct PageGeom {
    width: f64,
    height: f64,
    words: Vec<Word>,
    images: Vec<Rect>,
    paints: Vec<Paint>,
}

#[derive(Debug, Clone)]
pub struct LayoutProblem {
    pub page: usize,
    pub detail: String,
}

/// Boxes that collide, fills that cut into another row's words, and
/// anything that leaves the text block. `pdf` must be uncompressed
/// (`\pdfcompresslevel=0`); a FlateDecode content stream is an error.
pub fn pdf_layout_problems(pdf: &[u8]) -> Result<Vec<LayoutProblem>, String> {
    let objs = parse_objects(pdf)?;
    let pages = page_ids(&objs)?;
    let mut problems = Vec::new();
    for (idx, id) in pages.iter().enumerate() {
        let page_no = idx + 1;
        let geom = layout_page(&objs, *id)?;
        problems.extend(problems_on_page(page_no, &geom));
    }
    Ok(problems)
}

fn problems_on_page(page: usize, g: &PageGeom) -> Vec<LayoutProblem> {
    let mut out = Vec::new();
    let n = g.words.len();
    for i in 0..n {
        for j in i + 1..n {
            if g.words[i].rect.intersects(&g.words[j].rect, OVERLAP_PT) {
                out.push(LayoutProblem {
                    page,
                    detail: format!(
                        "text overlaps {:?} and {:?} ({:.1}×{:.1} pt at y {:.0})",
                        snippet(&g.words[i].text),
                        snippet(&g.words[j].text),
                        g.words[i].rect.x1.min(g.words[j].rect.x1)
                            - g.words[i].rect.x0.max(g.words[j].rect.x0),
                        g.words[i].rect.y1.min(g.words[j].rect.y1)
                            - g.words[i].rect.y0.max(g.words[j].rect.y0),
                        g.words[i].rect.cy()
                    ),
                });
            }
        }
        for im in &g.images {
            if g.words[i].rect.intersects(im, OVERLAP_PT) {
                out.push(LayoutProblem {
                    page,
                    detail: format!(
                        "text {:?} overlaps an image ({:.0},{:.0})-({:.0},{:.0})",
                        snippet(&g.words[i].text),
                        im.x0,
                        im.y0,
                        im.x1,
                        im.y1
                    ),
                });
            }
        }
        for p in &g.paints {
            if !p.fill {
                continue;
            }
            if !g.words[i].rect.intersects(&p.rect, OVERLAP_PT) {
                continue;
            }
            let cy = g.words[i].rect.cy();
            // The word belongs to this fill when its centre sits inside.
            // A fill that only clips the next row fails that test.
            if cy < p.rect.y0 - 0.4 || cy > p.rect.y1 + 0.4 {
                out.push(LayoutProblem {
                    page,
                    detail: format!(
                        "fill cuts into {:?} (word centre {:.1}, fill y {:.1}..{:.1})",
                        snippet(&g.words[i].text),
                        cy,
                        p.rect.y0,
                        p.rect.y1
                    ),
                });
            }
        }
    }
    for i in 0..g.images.len() {
        for j in i + 1..g.images.len() {
            if g.images[i].intersects(&g.images[j], OVERLAP_PT) {
                out.push(LayoutProblem {
                    page,
                    detail: format!("images overlap on page {page}"),
                });
            }
        }
    }

    let left = MARGIN_PT - EDGE_SLOP_PT;
    let right = g.width - MARGIN_PT + EDGE_SLOP_PT;
    let top = g.height - MARGIN_PT + EDGE_SLOP_PT;
    let bottom = MARGIN_PT - BOTTOM_SLOP_PT;
    let mut past = |label: &str, r: &Rect| {
        let mut why = Vec::new();
        if r.x0 < left {
            why.push(format!("left by {:.1} pt", left - r.x0));
        }
        if r.x1 > right {
            why.push(format!("right by {:.1} pt", r.x1 - right));
        }
        if r.y1 > top {
            why.push(format!("top by {:.1} pt", r.y1 - top));
        }
        if r.y0 < bottom {
            why.push(format!("bottom by {:.1} pt", bottom - r.y0));
        }
        if !why.is_empty() {
            out.push(LayoutProblem {
                page,
                detail: format!("{label} past the margin ({})", why.join(", ")),
            });
        }
    };
    for (i, w) in g.words.iter().enumerate() {
        if is_page_number(i, &g.words, g.width) {
            continue;
        }
        past(&format!("text {:?}", snippet(&w.text)), &w.rect);
    }
    for im in &g.images {
        past("image", im);
    }
    for p in &g.paints {
        // Hairlines and swatches smaller than a word are not a text block.
        if p.rect.width() < 1.0 && p.rect.height() < 1.0 {
            continue;
        }
        past(if p.fill { "fill" } else { "rule" }, &p.rect);
    }
    out
}

fn snippet(s: &str) -> String {
    let t: String = s.chars().take(42).collect();
    if s.chars().count() > 42 {
        format!("{t}…")
    } else {
        t
    }
}

/// The article class centres a decimal page number in the footer. A table
/// row that has fallen into that band has other words on the same baseline.
fn is_page_number(i: usize, words: &[Word], page_w: f64) -> bool {
    let w = &words[i];
    let t = w.text.trim();
    if t.is_empty() || t.len() > 4 || !t.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if w.rect.y1 > MARGIN_PT - 1.0 {
        return false;
    }
    if (w.rect.cx() - page_w / 2.0).abs() > 40.0 {
        return false;
    }
    words
        .iter()
        .enumerate()
        .all(|(j, o)| j == i || (o.rect.cy() - w.rect.cy()).abs() > 2.0)
}

// --- PDF objects -----------------------------------------------------------

struct RawObj {
    /// Object body. For a stream object this is the dictionary only.
    body: String,
    stream: Option<Vec<u8>>,
}

fn parse_objects(pdf: &[u8]) -> Result<BTreeMap<u32, RawObj>, String> {
    let mut objs = BTreeMap::new();
    let mut i = 0;
    while i < pdf.len() {
        let Some(at) = find_obj(pdf, i) else {
            break;
        };
        let (id, header_at) = at;
        let after = end_of_header_keyword(pdf, header_at);
        let (body, stream, next) = split_object(pdf, after)?;
        objs.insert(
            id,
            RawObj {
                body: String::from_utf8_lossy(&body).into_owned(),
                stream,
            },
        );
        i = next;
    }
    if objs.is_empty() {
        return Err("PDF has no objects".into());
    }
    Ok(objs)
}

/// Byte index of `N N obj`, and the id. `from` is where the search starts.
fn find_obj(pdf: &[u8], from: usize) -> Option<(u32, usize)> {
    let mut i = from;
    while i + 6 < pdf.len() {
        if pdf[i] == b'o' && pdf[i + 1] == b'b' && pdf[i + 2] == b'j' && is_boundary(pdf, i + 3) {
            // Walk back over "N N ".
            let mut k = i;
            while k > from && pdf[k - 1].is_ascii_whitespace() {
                k -= 1;
            }
            let end_gen = k;
            while k > from && pdf[k - 1].is_ascii_digit() {
                k -= 1;
            }
            let gen_ok = k < end_gen;
            while k > from && pdf[k - 1].is_ascii_whitespace() {
                k -= 1;
            }
            let end_id = k;
            while k > from && pdf[k - 1].is_ascii_digit() {
                k -= 1;
            }
            if gen_ok && k < end_id && (k == 0 || pdf[k - 1].is_ascii_whitespace()) {
                let id: u32 = std::str::from_utf8(&pdf[k..end_id]).ok()?.parse().ok()?;
                return Some((id, i));
            }
        }
        i += 1;
    }
    None
}

fn is_boundary(pdf: &[u8], i: usize) -> bool {
    i >= pdf.len() || pdf[i].is_ascii_whitespace() || matches!(pdf[i], b'<' | b'[' | b'/' | b'%')
}

fn end_of_header_keyword(pdf: &[u8], obj_kw: usize) -> usize {
    let mut i = obj_kw + 3;
    while i < pdf.len() && pdf[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn split_object(pdf: &[u8], start: usize) -> Result<(Vec<u8>, Option<Vec<u8>>, usize), String> {
    let rest = &pdf[start..];
    let stream_at = find_keyword(rest, b"stream");
    let endobj_at = find_keyword(rest, b"endobj");
    let Some(endobj_at) = endobj_at else {
        return Err("object is missing endobj".into());
    };
    if stream_at.is_some_and(|s| s < endobj_at) {
        let stream_at = stream_at.unwrap();
        let dict = rest[..stream_at].to_vec();
        let dict_s = String::from_utf8_lossy(&dict);
        let len = stream_length(&dict_s, pdf)?;
        let mut data_at = start + stream_at + b"stream".len();
        if pdf.get(data_at) == Some(&b'\r') {
            data_at += 1;
        }
        if pdf.get(data_at) == Some(&b'\n') {
            data_at += 1;
        }
        let data_end = data_at + len;
        if data_end > pdf.len() {
            return Err(format!("stream length {len} runs past the file"));
        }
        let stream = pdf[data_at..data_end].to_vec();
        let after = find_keyword(&pdf[data_end..], b"endobj")
            .map(|p| data_end + p + b"endobj".len())
            .unwrap_or(data_end);
        Ok((dict, Some(stream), after))
    } else {
        let body = rest[..endobj_at].to_vec();
        Ok((body, None, start + endobj_at + b"endobj".len()))
    }
}

fn find_keyword(hay: &[u8], kw: &[u8]) -> Option<usize> {
    hay.windows(kw.len()).position(|w| w == kw).and_then(|p| {
        let before_ok = p == 0 || hay[p - 1].is_ascii_whitespace();
        let after = p + kw.len();
        let after_ok = after >= hay.len() || hay[after].is_ascii_whitespace();
        if before_ok && after_ok {
            Some(p)
        } else {
            // `stream` can sit at the start; a later real hit is found by
            // scanning again only when the first match is a false friend
            // inside a longer word. One keyword check is enough for pdfTeX.
            None
        }
    })
}

fn stream_length(dict: &str, pdf: &[u8]) -> Result<usize, String> {
    let Some(at) = dict.find("/Length") else {
        return Err("stream has no /Length".into());
    };
    let rest = dict[at + "/Length".len()..].trim_start();
    let mut toks = rest.split_whitespace();
    let a = toks.next().unwrap_or("");
    let b = toks.next().unwrap_or("");
    let c = toks.next().unwrap_or("");
    if b == "0" && c == "R" {
        let id: u32 = a.parse().map_err(|_| format!("bad /Length ref {a}"))?;
        bare_int(pdf, id).ok_or_else(|| format!("/Length object {id} not found"))
    } else {
        a.trim_end_matches(|ch: char| !ch.is_ascii_digit() && ch != '.')
            .parse::<f64>()
            .map(|n| n as usize)
            .map_err(|_| format!("bad /Length {a}"))
    }
}

fn bare_int(pdf: &[u8], id: u32) -> Option<usize> {
    let marker = format!("\n{id} 0 obj");
    let text = String::from_utf8_lossy(pdf);
    let at = text.find(&marker)? + marker.len();
    let rest = text[at..].trim_start();
    let num = rest.split_whitespace().next()?;
    num.parse::<usize>().ok()
}

fn page_ids(objs: &BTreeMap<u32, RawObj>) -> Result<Vec<u32>, String> {
    let catalog = objs.iter().find(|(_, o)| has_type(&o.body, "Catalog"));
    if let Some((_, cat)) = catalog {
        if let Some(pages) = first_ref_after(&cat.body, "/Pages") {
            let mut out = Vec::new();
            walk_pages(objs, pages, &mut out, 0)?;
            if !out.is_empty() {
                return Ok(out);
            }
        }
    }
    let mut pages: Vec<u32> = objs
        .iter()
        .filter(|(_, o)| has_type(&o.body, "Page"))
        .map(|(id, _)| *id)
        .collect();
    pages.sort_unstable();
    if pages.is_empty() {
        Err("PDF has no pages".into())
    } else {
        Ok(pages)
    }
}

fn walk_pages(
    objs: &BTreeMap<u32, RawObj>,
    id: u32,
    out: &mut Vec<u32>,
    depth: usize,
) -> Result<(), String> {
    if depth > 12 {
        return Err("page tree is too deep".into());
    }
    let obj = objs.get(&id).ok_or_else(|| format!("missing page {id}"))?;
    if has_type(&obj.body, "Pages") {
        for kid in refs_after(&obj.body, "/Kids") {
            walk_pages(objs, kid, out, depth + 1)?;
        }
    } else if has_type(&obj.body, "Page") {
        out.push(id);
    }
    Ok(())
}

fn has_type(body: &str, name: &str) -> bool {
    let marker = format!("/Type /{name}");
    let Some(at) = body.find(&marker) else {
        return false;
    };
    let after = at + marker.len();
    !body[after..].starts_with(|c: char| c.is_ascii_alphanumeric())
}

fn first_ref_after(body: &str, key: &str) -> Option<u32> {
    refs_after(body, key).into_iter().next()
}

fn refs_after(body: &str, key: &str) -> Vec<u32> {
    let Some(at) = body.find(key) else {
        return Vec::new();
    };
    let rest = &body[at + key.len()..];
    let slice = if rest.trim_start().starts_with('[') {
        let t = rest.trim_start();
        take_bracket(t).unwrap_or(t)
    } else {
        let t = rest.trim_start();
        // one "N 0 R"
        let mut n = 0;
        let b = t.as_bytes();
        let mut i = 0;
        while i < b.len() && n < 3 {
            if b[i].is_ascii_whitespace() {
                if i > 0 && !b[i - 1].is_ascii_whitespace() {
                    n += 1;
                }
            }
            i += 1;
            if n >= 3 {
                break;
            }
        }
        &t[..i.min(t.len())]
    };
    let mut out = Vec::new();
    let toks: Vec<&str> = slice.split_whitespace().collect();
    let mut i = 0;
    while i + 2 < toks.len() {
        if toks[i + 1] == "0" && toks[i + 2].starts_with('R') {
            if let Ok(id) = toks[i].parse() {
                out.push(id);
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}

fn take_bracket(s: &str) -> Option<&str> {
    if !s.starts_with('[') {
        return None;
    }
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn take_dict(s: &str) -> Option<&str> {
    let s = s.trim_start();
    if !s.starts_with("<<") {
        return None;
    }
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'<' && b[i + 1] == b'<' {
            depth += 1;
            i += 2;
            continue;
        }
        if b[i] == b'>' && b[i + 1] == b'>' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Some(&s[..i]);
            }
            continue;
        }
        i += 1;
    }
    None
}

fn resolve_dict<'a>(objs: &'a BTreeMap<u32, RawObj>, body: &'a str, key: &str) -> Option<&'a str> {
    let at = body.find(key)?;
    let rest = body[at + key.len()..].trim_start();
    if rest.starts_with("<<") {
        return take_dict(rest);
    }
    let toks: Vec<&str> = rest.split_whitespace().take(3).collect();
    if toks.len() >= 3 && toks[1] == "0" && toks[2].starts_with('R') {
        let id = toks[0].parse().ok()?;
        objs.get(&id).map(|o| o.body.as_str())
    } else {
        None
    }
}

#[derive(Clone)]
struct FontInfo {
    first: i32,
    widths: Vec<f64>,
}

struct Resources {
    fonts: BTreeMap<String, FontInfo>,
    xobjects: BTreeMap<String, u32>,
}

fn resources_of(objs: &BTreeMap<u32, RawObj>, page_body: &str) -> Resources {
    let mut body = page_body.to_string();
    // Inherit /Resources from /Parent when the page does not carry them.
    if !body.contains("/Resources") {
        let mut guard = 0;
        let mut current = page_body.to_string();
        while guard < 8 {
            guard += 1;
            let Some(parent) = first_ref_after(&current, "/Parent") else {
                break;
            };
            let Some(p) = objs.get(&parent) else { break };
            if p.body.contains("/Resources") {
                body = p.body.clone();
                break;
            }
            current = p.body.clone();
        }
    }
    let res_body = resolve_dict(objs, &body, "/Resources")
        .unwrap_or("")
        .to_string();
    let fonts = font_map(objs, &res_body);
    let xobjects = name_ref_map(resolve_dict(objs, &res_body, "/XObject").unwrap_or(""))
        .into_iter()
        .collect();
    Resources { fonts, xobjects }
}

fn font_map(objs: &BTreeMap<u32, RawObj>, res: &str) -> BTreeMap<String, FontInfo> {
    let mut out = BTreeMap::new();
    let Some(font_dict) = resolve_dict(objs, res, "/Font") else {
        return out;
    };
    for (name, id) in name_ref_map(font_dict) {
        let Some(font) = objs.get(&id) else { continue };
        let first = int_after(&font.body, "/FirstChar").unwrap_or(0);
        let widths = if let Some(id) = first_ref_after(&font.body, "/Widths") {
            objs.get(&id)
                .map(|o| number_list(&o.body))
                .unwrap_or_default()
        } else if let Some(at) = font.body.find("/Widths") {
            let rest = font.body[at + "/Widths".len()..].trim_start();
            take_bracket(rest).map(number_list).unwrap_or_default()
        } else {
            Vec::new()
        };
        out.insert(name, FontInfo { first, widths });
    }
    out
}

fn name_ref_map(dict: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let toks: Vec<&str> = dict.split_whitespace().collect();
    let mut i = 0;
    while i + 3 < toks.len() {
        if let Some(name) = toks[i].strip_prefix('/') {
            if toks[i + 2] == "0" && toks[i + 3].starts_with('R') {
                if let Ok(id) = toks[i + 1].parse() {
                    // Skip nested-dict markers that are not names of objects.
                    if !name.is_empty() && name != "Font" && name != "XObject" {
                        out.push((name.to_string(), id));
                    }
                }
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn int_after(body: &str, key: &str) -> Option<i32> {
    let at = body.find(key)?;
    let rest = body[at + key.len()..].trim_start();
    let tok = rest.split_whitespace().next()?;
    tok.parse().ok()
}

fn number_list(body: &str) -> Vec<f64> {
    let start = body.find('[').map(|i| i + 1).unwrap_or(0);
    let end = body[start..]
        .find(']')
        .map(|i| start + i)
        .unwrap_or(body.len());
    body[start..end]
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect()
}

fn layout_page(objs: &BTreeMap<u32, RawObj>, page_id: u32) -> Result<PageGeom, String> {
    let page = objs
        .get(&page_id)
        .ok_or_else(|| format!("missing page {page_id}"))?;
    let mb = media_box(objs, &page.body).unwrap_or([0.0, 0.0, 595.276, 841.89]);
    let res = resources_of(objs, &page.body);
    let contents = content_ids(&page.body);
    let mut words = Vec::new();
    let mut images = Vec::new();
    let mut paints = Vec::new();
    for cid in contents {
        let obj = objs
            .get(&cid)
            .ok_or_else(|| format!("missing contents {cid}"))?;
        let stream = obj
            .stream
            .as_deref()
            .ok_or_else(|| format!("contents {cid} is not a stream"))?;
        if obj.body.contains("/FlateDecode") {
            return Err("PDF content is compressed; compile with \\pdfcompresslevel=0".into());
        }
        interpret(stream, &res, objs, &mut words, &mut images, &mut paints, 0)?;
    }
    Ok(PageGeom {
        width: mb[2] - mb[0],
        height: mb[3] - mb[1],
        words,
        images,
        paints,
    })
}

fn media_box(objs: &BTreeMap<u32, RawObj>, body: &str) -> Option<[f64; 4]> {
    if let Some(at) = body.find("/MediaBox") {
        let rest = body[at + "/MediaBox".len()..].trim_start();
        if let Some(br) = take_bracket(rest) {
            let n = number_list(br);
            if n.len() == 4 {
                return Some([n[0], n[1], n[2], n[3]]);
            }
        }
    }
    let parent = first_ref_after(body, "/Parent")?;
    let p = objs.get(&parent)?;
    media_box(objs, &p.body)
}

fn content_ids(body: &str) -> Vec<u32> {
    refs_after(body, "/Contents")
}

// --- content stream --------------------------------------------------------

#[derive(Clone)]
enum Opnd {
    Num(f64),
    Name(String),
    Str(Vec<u8>),
    Arr(Vec<Opnd>),
}

struct GState {
    ctm: [f64; 6],
    line_width: f64,
    font: String,
    font_size: f64,
    /// Text matrix. Glyphs live in this space, then the CTM.
    tm: [f64; 6],
    pen_x: f64,
    char_spacing: f64,
    word_spacing: f64,
    hscale: f64,
    leading: f64,
    path: Vec<(f64, f64)>,
}

impl GState {
    fn new() -> Self {
        Self {
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            line_width: 1.0,
            font: String::new(),
            font_size: 10.0,
            tm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            pen_x: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            hscale: 100.0,
            leading: 0.0,
            path: Vec::new(),
        }
    }
}

struct WordBuild {
    rect: Rect,
    text: String,
}

struct Interp<'a> {
    gs: GState,
    stack_gs: Vec<GState>,
    ops: Vec<Opnd>,
    word: Option<WordBuild>,
    fonts: &'a BTreeMap<String, FontInfo>,
    xobjects: &'a BTreeMap<String, u32>,
    objs: &'a BTreeMap<u32, RawObj>,
    words: &'a mut Vec<Word>,
    images: &'a mut Vec<Rect>,
    paints: &'a mut Vec<Paint>,
    depth: usize,
}

fn interpret(
    stream: &[u8],
    res: &Resources,
    objs: &BTreeMap<u32, RawObj>,
    words: &mut Vec<Word>,
    images: &mut Vec<Rect>,
    paints: &mut Vec<Paint>,
    depth: usize,
) -> Result<(), String> {
    if depth > 6 {
        return Ok(());
    }
    let toks = tokenize(stream);
    let mut ip = Interp {
        gs: GState::new(),
        stack_gs: Vec::new(),
        ops: Vec::new(),
        word: None,
        fonts: &res.fonts,
        xobjects: &res.xobjects,
        objs,
        words,
        images,
        paints,
        depth,
    };
    for tok in toks {
        match tok {
            Tok::Num(n) => ip.ops.push(Opnd::Num(n)),
            Tok::Name(n) => ip.ops.push(Opnd::Name(n)),
            Tok::Str(s) => ip.ops.push(Opnd::Str(s)),
            Tok::Arr(a) => ip.ops.push(Opnd::Arr(a)),
            Tok::Op(op) => ip.op(&op)?,
        }
    }
    ip.flush();
    Ok(())
}

enum Tok {
    Num(f64),
    Name(String),
    Str(Vec<u8>),
    Arr(Vec<Opnd>),
    Op(String),
}

fn tokenize(data: &[u8]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'%' {
            while i < data.len() && data[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'(' {
            let (s, n) = parse_literal(data, i);
            out.push(Tok::Str(s));
            i = n;
            continue;
        }
        if b == b'<' && data.get(i + 1) != Some(&b'<') {
            let (s, n) = parse_hex(data, i);
            out.push(Tok::Str(s));
            i = n;
            continue;
        }
        if b == b'<' && data.get(i + 1) == Some(&b'<') {
            // Inline dict in a content stream (image dictionary). Skip it.
            let s = String::from_utf8_lossy(&data[i..]);
            if let Some(d) = take_dict(&s) {
                i += d.len();
                continue;
            }
        }
        if b == b'/' {
            let start = i + 1;
            i += 1;
            while i < data.len() && !is_delim(data[i]) {
                i += 1;
            }
            let name = String::from_utf8_lossy(&data[start..i]).into_owned();
            out.push(Tok::Name(name));
            continue;
        }
        if b == b'[' {
            let (arr, n) = parse_array(data, i);
            out.push(Tok::Arr(arr));
            i = n;
            continue;
        }
        if b == b']' {
            i += 1;
            continue;
        }
        if is_num_start(data, i) {
            if let Some((n, j)) = parse_num(data, i) {
                out.push(Tok::Num(n));
                i = j;
                continue;
            }
        }
        if b.is_ascii_alphabetic() || b == b'\'' || b == b'"' {
            let start = i;
            i += 1;
            if b.is_ascii_alphabetic() {
                while i < data.len() && data[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if data.get(i) == Some(&b'*') {
                    i += 1;
                }
            }
            let op = String::from_utf8_lossy(&data[start..i]).into_owned();
            out.push(Tok::Op(op));
            continue;
        }
        i += 1;
    }
    out
}

fn is_delim(b: u8) -> bool {
    b.is_ascii_whitespace()
        || matches!(
            b,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
}

fn is_num_start(data: &[u8], i: usize) -> bool {
    let b = data[i];
    if b.is_ascii_digit() || b == b'.' {
        return true;
    }
    if (b == b'+' || b == b'-') && i + 1 < data.len() {
        let n = data[i + 1];
        return n.is_ascii_digit() || n == b'.';
    }
    false
}

fn parse_num(data: &[u8], mut i: usize) -> Option<(f64, usize)> {
    let start = i;
    if data[i] == b'+' || data[i] == b'-' {
        i += 1;
    }
    let mut saw = false;
    while i < data.len() && data[i].is_ascii_digit() {
        saw = true;
        i += 1;
    }
    if i < data.len() && data[i] == b'.' {
        i += 1;
        while i < data.len() && data[i].is_ascii_digit() {
            saw = true;
            i += 1;
        }
    }
    if !saw {
        return None;
    }
    let s = std::str::from_utf8(&data[start..i]).ok()?;
    Some((s.parse().ok()?, i))
}

fn parse_literal(data: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    i += 1; // (
    let mut out = Vec::new();
    let mut depth = 1i32;
    while i < data.len() && depth > 0 {
        match data[i] {
            b'\\' => {
                i += 1;
                if i >= data.len() {
                    break;
                }
                match data[i] {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'(' | b')' | b'\\' => out.push(data[i]),
                    b'\n' => {}
                    b'\r' => {
                        if data.get(i + 1) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    d if d.is_ascii_digit() => {
                        let mut v = 0u16;
                        let mut k = 0;
                        while k < 3 && i < data.len() && data[i].is_ascii_digit() && data[i] < b'8'
                        {
                            v = v * 8 + (data[i] - b'0') as u16;
                            i += 1;
                            k += 1;
                        }
                        out.push(v as u8);
                        continue;
                    }
                    other => out.push(other),
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                out.push(b'(');
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth > 0 {
                    out.push(b')');
                }
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    (out, i)
}

fn parse_hex(data: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    i += 1;
    let mut hex = Vec::new();
    while i < data.len() && data[i] != b'>' {
        if !data[i].is_ascii_whitespace() {
            hex.push(data[i]);
        }
        i += 1;
    }
    if i < data.len() {
        i += 1;
    }
    if hex.len() % 2 == 1 {
        hex.push(b'0');
    }
    let mut out = Vec::new();
    let mut k = 0;
    while k + 1 < hex.len() {
        let h = |c: u8| -> u8 {
            match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => 0,
            }
        };
        out.push((h(hex[k]) << 4) | h(hex[k + 1]));
        k += 2;
    }
    (out, i)
}

fn parse_array(data: &[u8], mut i: usize) -> (Vec<Opnd>, usize) {
    i += 1; // [
    let mut items = Vec::new();
    while i < data.len() && data[i] != b']' {
        if data[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if data[i] == b'(' {
            let (s, n) = parse_literal(data, i);
            items.push(Opnd::Str(s));
            i = n;
            continue;
        }
        if data[i] == b'<' && data.get(i + 1) != Some(&b'<') {
            let (s, n) = parse_hex(data, i);
            items.push(Opnd::Str(s));
            i = n;
            continue;
        }
        if data[i] == b'/' {
            let start = i + 1;
            i += 1;
            while i < data.len() && !is_delim(data[i]) {
                i += 1;
            }
            items.push(Opnd::Name(
                String::from_utf8_lossy(&data[start..i]).into_owned(),
            ));
            continue;
        }
        if is_num_start(data, i) {
            if let Some((n, j)) = parse_num(data, i) {
                items.push(Opnd::Num(n));
                i = j;
                continue;
            }
        }
        if data[i] == b'[' {
            let (a, n) = parse_array(data, i);
            items.push(Opnd::Arr(a));
            i = n;
            continue;
        }
        i += 1;
    }
    if i < data.len() && data[i] == b']' {
        i += 1;
    }
    (items, i)
}

fn apply(m: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// `cm` concatenates: the new matrix is applied to the point first.
fn concat(ctm: [f64; 6], m: [f64; 6]) -> [f64; 6] {
    let (a, b, c, d, e, f) = (m[0], m[1], m[2], m[3], m[4], m[5]);
    let (a2, b2, c2, d2, e2, f2) = (ctm[0], ctm[1], ctm[2], ctm[3], ctm[4], ctm[5]);
    [
        a * a2 + b * c2,
        a * b2 + b * d2,
        c * a2 + d * c2,
        c * b2 + d * d2,
        e * a2 + f * c2 + e2,
        e * b2 + f * d2 + f2,
    ]
}

impl Interp<'_> {
    fn op(&mut self, op: &str) -> Result<(), String> {
        match op {
            "q" => self.stack_gs.push(self.clone_gs()),
            "Q" => {
                self.flush();
                if let Some(gs) = self.stack_gs.pop() {
                    self.gs = gs;
                }
            }
            "cm" => {
                let f = self.pop_num();
                let e = self.pop_num();
                let d = self.pop_num();
                let c = self.pop_num();
                let b = self.pop_num();
                let a = self.pop_num();
                self.gs.ctm = concat(self.gs.ctm, [a, b, c, d, e, f]);
            }
            "w" => self.gs.line_width = self.pop_num(),
            "g" | "G" | "i" | "j" | "J" | "M" | "ri" | "gs" | "cs" | "CS" | "Tc" | "Tw" | "Tz"
            | "TL" | "Tr" | "Ts" => match op {
                "Tc" => self.gs.char_spacing = self.pop_num(),
                "Tw" => self.gs.word_spacing = self.pop_num(),
                "Tz" => self.gs.hscale = self.pop_num(),
                "TL" => self.gs.leading = self.pop_num(),
                "gs" | "cs" | "CS" | "ri" => {
                    self.pop_any();
                }
                _ => {
                    self.pop_num();
                }
            },
            "rg" | "RG" => {
                let _ = (self.pop_num(), self.pop_num(), self.pop_num());
            }
            "k" | "K" => {
                let _ = (
                    self.pop_num(),
                    self.pop_num(),
                    self.pop_num(),
                    self.pop_num(),
                );
            }
            "d" => {
                self.pop_any();
                self.pop_any();
            }
            "m" => {
                let y = self.pop_num();
                let x = self.pop_num();
                self.gs.path.push(apply(&self.gs.ctm, x, y));
            }
            "l" => {
                let y = self.pop_num();
                let x = self.pop_num();
                self.gs.path.push(apply(&self.gs.ctm, x, y));
            }
            "c" => {
                let y3 = self.pop_num();
                let x3 = self.pop_num();
                let y2 = self.pop_num();
                let x2 = self.pop_num();
                let y1 = self.pop_num();
                let x1 = self.pop_num();
                for (x, y) in [(x1, y1), (x2, y2), (x3, y3)] {
                    self.gs.path.push(apply(&self.gs.ctm, x, y));
                }
            }
            "v" | "y" => {
                let y2 = self.pop_num();
                let x2 = self.pop_num();
                let y1 = self.pop_num();
                let x1 = self.pop_num();
                for (x, y) in [(x1, y1), (x2, y2)] {
                    self.gs.path.push(apply(&self.gs.ctm, x, y));
                }
            }
            "h" => {}
            "re" => {
                let h = self.pop_num();
                let w = self.pop_num();
                let y = self.pop_num();
                let x = self.pop_num();
                for (px, py) in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)] {
                    self.gs.path.push(apply(&self.gs.ctm, px, py));
                }
            }
            "S" | "s" => self.paint(false, true),
            "f" | "f*" => self.paint(true, false),
            "B" | "B*" | "b" | "b*" => self.paint(true, true),
            "n" | "W" | "W*" => {
                if op == "n" {
                    self.gs.path.clear();
                }
            }
            "BT" => {
                self.flush();
                self.gs.tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                self.gs.pen_x = 0.0;
            }
            "ET" => self.flush(),
            "Tf" => {
                let size = self.pop_num();
                let name = self.pop_name();
                self.gs.font = name;
                self.gs.font_size = size;
            }
            "Td" => {
                let ty = self.pop_num();
                let tx = self.pop_num();
                self.td(tx, ty);
            }
            "TD" => {
                let ty = self.pop_num();
                let tx = self.pop_num();
                self.gs.leading = -ty;
                self.td(tx, ty);
            }
            "Tm" => {
                self.flush();
                let f = self.pop_num();
                let e = self.pop_num();
                let d = self.pop_num();
                let c = self.pop_num();
                let b = self.pop_num();
                let a = self.pop_num();
                self.gs.tm = [a, b, c, d, e, f];
                self.gs.pen_x = 0.0;
            }
            "T*" => {
                let lead = self.gs.leading;
                self.td(0.0, -lead);
            }
            "Tj" => {
                if let Some(Opnd::Str(s)) = self.ops.pop() {
                    self.draw_bytes(&s);
                }
            }
            "TJ" => {
                if let Some(Opnd::Arr(arr)) = self.ops.pop() {
                    for item in arr {
                        match item {
                            Opnd::Num(n) => {
                                // Thousandths of an em, subtracted from x.
                                let dx = -n / 1000.0 * self.gs.font_size * (self.gs.hscale / 100.0);
                                self.gs.pen_x += dx;
                            }
                            Opnd::Str(s) => self.draw_bytes(&s),
                            _ => {}
                        }
                    }
                }
            }
            "'" => {
                if let Some(Opnd::Str(s)) = self.ops.pop() {
                    let lead = self.gs.leading;
                    self.td(0.0, -lead);
                    self.draw_bytes(&s);
                }
            }
            "\"" => {
                if let Some(Opnd::Str(s)) = self.ops.pop() {
                    let ac = self.pop_num();
                    let aw = self.pop_num();
                    self.gs.word_spacing = aw;
                    self.gs.char_spacing = ac;
                    let lead = self.gs.leading;
                    self.td(0.0, -lead);
                    self.draw_bytes(&s);
                }
            }
            "Do" => {
                let name = self.pop_name();
                self.do_xobject(&name)?;
            }
            _ => self.ops.clear(),
        }
        Ok(())
    }

    fn clone_gs(&self) -> GState {
        GState {
            ctm: self.gs.ctm,
            line_width: self.gs.line_width,
            font: self.gs.font.clone(),
            font_size: self.gs.font_size,
            tm: self.gs.tm,
            pen_x: self.gs.pen_x,
            char_spacing: self.gs.char_spacing,
            word_spacing: self.gs.word_spacing,
            hscale: self.gs.hscale,
            leading: self.gs.leading,
            path: self.gs.path.clone(),
        }
    }

    fn pop_num(&mut self) -> f64 {
        match self.ops.pop() {
            Some(Opnd::Num(n)) => n,
            _ => 0.0,
        }
    }

    fn pop_name(&mut self) -> String {
        match self.ops.pop() {
            Some(Opnd::Name(n)) => n,
            _ => String::new(),
        }
    }

    fn pop_any(&mut self) {
        let _ = self.ops.pop();
    }

    fn td(&mut self, tx: f64, ty: f64) {
        self.flush();
        let (a, b, c, d, e, f) = (
            self.gs.tm[0],
            self.gs.tm[1],
            self.gs.tm[2],
            self.gs.tm[3],
            self.gs.tm[4],
            self.gs.tm[5],
        );
        self.gs.tm[4] = a * tx + c * ty + e;
        self.gs.tm[5] = b * tx + d * ty + f;
        self.gs.pen_x = 0.0;
    }

    fn text_point(&self, dx: f64, dy: f64) -> (f64, f64) {
        let (x, y) = apply(&self.gs.tm, self.gs.pen_x + dx, dy);
        apply(&self.gs.ctm, x, y)
    }

    fn em_user(&self) -> f64 {
        let a = self.text_point(0.0, 0.0);
        let b = self.text_point(self.gs.font_size.max(0.1), 0.0);
        ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
    }

    fn glyph_width(&self, byte: u8) -> f64 {
        let units = self
            .fonts
            .get(&self.gs.font)
            .map(|f| {
                let i = byte as i32 - f.first;
                if i >= 0 {
                    f.widths.get(i as usize).copied().unwrap_or(500.0)
                } else {
                    500.0
                }
            })
            .unwrap_or(500.0);
        let mut w = units / 1000.0 * self.gs.font_size * (self.gs.hscale / 100.0);
        if byte == b' ' {
            w += self.gs.word_spacing * (self.gs.hscale / 100.0);
        }
        w
    }

    fn draw_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if b == b' ' || b == b'\t' {
                self.flush();
                self.gs.pen_x += self.glyph_width(b' ');
                continue;
            }
            let gw = self.glyph_width(b);
            let origin = self.text_point(0.0, 0.0);
            let right = self.text_point(gw, 0.0);
            let top = self.text_point(0.0, self.gs.font_size * 0.72);
            let bot = self.text_point(0.0, -self.gs.font_size * 0.22);
            let rect = Rect::from_points(&[origin, right, top, bot])
                .unwrap_or(Rect {
                    x0: origin.0,
                    y0: origin.1,
                    x1: origin.0,
                    y1: origin.1,
                })
                .normalize();
            self.push_glyph(rect, b);
            self.gs.pen_x += gw + self.gs.char_spacing * (self.gs.hscale / 100.0);
        }
    }

    fn push_glyph(&mut self, rect: Rect, b: u8) {
        let ch = char::from(b);
        let em = self.em_user().max(0.1);
        let split = if let Some(w) = &self.word {
            let gap = rect.x0 - w.rect.x1;
            let jump = (rect.cy() - w.rect.cy()).abs();
            gap > 0.18 * em || jump > 0.45 * em
        } else {
            false
        };
        if split {
            self.flush();
        }
        if let Some(w) = self.word.as_mut() {
            w.rect.x0 = w.rect.x0.min(rect.x0);
            w.rect.y0 = w.rect.y0.min(rect.y0);
            w.rect.x1 = w.rect.x1.max(rect.x1);
            w.rect.y1 = w.rect.y1.max(rect.y1);
            w.text.push(ch);
        } else {
            self.word = Some(WordBuild {
                rect,
                text: ch.to_string(),
            });
        }
    }

    fn flush(&mut self) {
        if let Some(w) = self.word.take() {
            if w.rect.width() > 0.01 && !w.text.trim().is_empty() {
                self.words.push(Word {
                    rect: w.rect,
                    text: w.text,
                });
            }
        }
    }

    fn paint(&mut self, fill: bool, stroke: bool) {
        if let Some(rect) = Rect::from_points(&self.gs.path) {
            let pad = if stroke {
                0.5 * self.gs.line_width * self.gs.ctm[0].abs().max(self.gs.ctm[3].abs()).max(0.0)
            } else {
                0.0
            };
            let rect = rect.normalize().expand(pad);
            if rect.width() > 0.05 || rect.height() > 0.05 {
                self.paints.push(Paint { rect, fill });
            }
        }
        self.gs.path.clear();
    }

    fn do_xobject(&mut self, name: &str) -> Result<(), String> {
        let Some(id) = self.xobjects.get(name).copied() else {
            return Ok(());
        };
        let Some(obj) = self.objs.get(&id) else {
            return Ok(());
        };
        if obj.body.contains("/Subtype /Form") {
            let matrix = form_matrix(&obj.body);
            let saved = self.gs.ctm;
            self.gs.ctm = concat(saved, matrix);
            if let Some(stream) = obj.stream.as_deref() {
                if !obj.body.contains("/FlateDecode") {
                    let local = form_resources(self.objs, &obj.body, self.fonts, self.xobjects);
                    interpret(
                        stream,
                        &local,
                        self.objs,
                        self.words,
                        self.images,
                        self.paints,
                        self.depth + 1,
                    )?;
                }
            }
            self.gs.ctm = saved;
        } else {
            // Image (or unknown): the unit square through the CTM.
            let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
            let pts: Vec<(f64, f64)> = corners
                .into_iter()
                .map(|(x, y)| apply(&self.gs.ctm, x, y))
                .collect();
            if let Some(rect) = Rect::from_points(&pts) {
                self.images.push(rect.normalize());
            }
        }
        Ok(())
    }
}

fn form_matrix(body: &str) -> [f64; 6] {
    if let Some(at) = body.find("/Matrix") {
        let rest = body[at + "/Matrix".len()..].trim_start();
        if let Some(br) = take_bracket(rest) {
            let n = number_list(br);
            if n.len() == 6 {
                return [n[0], n[1], n[2], n[3], n[4], n[5]];
            }
        }
    }
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

fn form_resources(
    objs: &BTreeMap<u32, RawObj>,
    body: &str,
    parent_fonts: &BTreeMap<String, FontInfo>,
    parent_xo: &BTreeMap<String, u32>,
) -> Resources {
    let mut fonts = parent_fonts.clone();
    let mut xobjects = parent_xo.clone();
    if let Some(res) = resolve_dict(objs, body, "/Resources") {
        fonts.extend(font_map(objs, res));
        for (n, id) in name_ref_map(resolve_dict(objs, res, "/XObject").unwrap_or("")) {
            xobjects.insert(n, id);
        }
    }
    Resources { fonts, xobjects }
}

pub fn format_problems(problems: &[LayoutProblem]) -> String {
    let n = problems.len();
    let shown = problems.iter().take(40);
    let mut s = String::new();
    for p in shown {
        s.push_str(&format!("  page {}: {}\n", p.page, p.detail));
    }
    if n > 40 {
        s.push_str(&format!("  … and {} more\n", n - 40));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mini_pdf(content: &str) -> Vec<u8> {
        let widths: String = std::iter::repeat("600 ").take(96).collect();
        let payload = content.as_bytes();
        format!(
            "%PDF-1.4\n\
             1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
             2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
             3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.276 841.89]\n\
             /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 7 0 R >> >> >>\nendobj\n\
             4 0 obj\n<< /Length {len} >>\nstream\n{content}endstream\nendobj\n\
             5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helv /FirstChar 32 /LastChar 127 /Widths 6 0 R >>\nendobj\n\
             6 0 obj\n[ {widths}]\nendobj\n\
             7 0 obj\n<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Length 0 >>\nstream\nendstream\nendobj\n",
            len = payload.len(),
            content = content,
            widths = widths,
        )
        .into_bytes()
    }

    fn problems(content: &str) -> Vec<LayoutProblem> {
        pdf_layout_problems(&mini_pdf(content)).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn translated_word_lands_where_the_matrix_says() {
        let pdf = mini_pdf("q\n1 0 0 1 50 0 cm\nBT\n/F1 10 Tf 100 400 Td [(Hi)]TJ\nET\nQ\n");
        let objs = parse_objects(&pdf).unwrap();
        let geom = layout_page(&objs, 3).unwrap();
        let w = geom
            .words
            .iter()
            .find(|w| w.text.contains('H'))
            .expect("word");
        assert!(
            (w.rect.x0 - 150.0).abs() < 1.0,
            "x0 {} (CTM + Td)",
            w.rect.x0
        );
        assert!((w.rect.cy() - 400.0).abs() < 8.0, "y {}", w.rect.cy());
    }

    #[test]
    fn overlapping_words_are_reported_and_a_clean_page_is_not() {
        let hit = problems(
            "BT\n/F1 10 Tf 100 400 Td [(Hello)]TJ\nET\nBT\n/F1 10 Tf 110 400 Td [(World)]TJ\nET\n",
        );
        assert!(hit.iter().any(|p| p.detail.contains("overlaps")), "{hit:?}");
        let clean = problems(
            "BT\n/F1 10 Tf 200 400 Td [(OK)]TJ\nET\nBT\n/F1 10 Tf 294 28 Td [(1)]TJ\nET\n",
        );
        assert!(clean.is_empty(), "{clean:?}");
    }

    #[test]
    fn margin_and_fill_bleed_and_image_box() {
        let margin = problems("BT\n/F1 10 Tf 10 400 Td [(X)]TJ\nET\n");
        assert!(
            margin.iter().any(|p| p.detail.contains("past the margin")),
            "{margin:?}"
        );
        // Fill covers only the bottom of the word: the centre is outside.
        let bleed =
            problems("0 0 0 rg\n180 388 80 12 re\nf\nBT\n/F1 10 Tf 200 400 Td [(Word)]TJ\nET\n");
        assert!(
            bleed.iter().any(|p| p.detail.contains("fill cuts")),
            "{bleed:?}"
        );
        let pdf =
            mini_pdf("q\n120 0 0 60 80 500 cm\n/Im1 Do\nQ\nBT\n/F1 10 Tf 80 500 Td [(Z)]TJ\nET\n");
        let objs = parse_objects(&pdf).unwrap();
        let geom = layout_page(&objs, 3).unwrap();
        assert_eq!(geom.images.len(), 1, "image box");
        let im = &geom.images[0];
        assert!(
            (im.x0 - 80.0).abs() < 0.5 && (im.x1 - 200.0).abs() < 0.5,
            "{im:?}"
        );
        assert!(
            (im.y0 - 500.0).abs() < 0.5 && (im.y1 - 560.0).abs() < 0.5,
            "{im:?}"
        );
        // The word sits on the image.
        let hit = pdf_layout_problems(&pdf).unwrap();
        assert!(hit.iter().any(|p| p.detail.contains("image")), "{hit:?}");
    }

    fn fixture(name: &str) -> String {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pdf_overlap")
            .join(name);
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
    }

    fn render_pdf(md: &str) -> Result<Vec<u8>, String> {
        if !crate::pdf_compile::which_exists("pdflatex") {
            return Err("__skip__".into());
        }
        let source = crate::strip_render_artifacts(md);
        let blocks = crate::parse::parse_notebook(&source);
        let rendered = crate::execute::execute_notebook(&blocks);
        let work = tempfile::tempdir().map_err(|e| e.to_string())?;
        let plot_dir = work.path().join("plots").join("notebook");
        let theme = rustlab_plot::theme::Theme::Light.colors();
        let mut tex = crate::render_latex::render_latex(
            "Overlap",
            &rendered,
            &plot_dir,
            "plots/notebook",
            &theme,
            &crate::render::LinkMode::single_file(),
        );
        // Layout is unchanged. The geometry parser reads plain content streams.
        if let Some(pos) = tex.find("\\begin{document}") {
            tex.insert_str(pos, "\\pdfcompresslevel=0\n\\pdfobjcompresslevel=0\n");
        } else {
            return Err("tex has no document body".into());
        }
        let tex_path = work.path().join("notebook.tex");
        std::fs::write(&tex_path, &tex).map_err(|e| e.to_string())?;
        crate::pdf_compile::convert_svgs_to_pdf(work.path())?;
        let pdf_path = work.path().join("out.pdf");
        if let Err(e) = crate::compile_pdf(&tex_path, &pdf_path) {
            let log = std::fs::read_to_string(tex_path.with_extension("log")).unwrap_or_default();
            let tail: String = log
                .lines()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!("{e}\n{tail}"));
        }
        std::fs::read(&pdf_path).map_err(|e| e.to_string())
    }

    fn expect_clean(name: &str) {
        let md = fixture(name);
        match render_pdf(&md) {
            Err(e) if e == "__skip__" => {
                eprintln!("skipping {name}: pdflatex not installed");
            }
            Err(e) => panic!("{name}: PDF render failed:\n{e}"),
            Ok(pdf) => match pdf_layout_problems(&pdf) {
                Err(e) => panic!("{name}: {e}"),
                Ok(problems) if problems.is_empty() => {}
                Ok(problems) => panic!(
                    "{name}: {} layout problem(s)\n{}",
                    problems.len(),
                    format_problems(&problems)
                ),
            },
        }
    }

    #[test]
    fn plain_prose_pdf_has_no_overlap() {
        expect_clean("00_plain.md");
    }

    #[test]
    fn paragraphs_around_a_table_do_not_overlap() {
        expect_clean("01_around_table.md");
    }

    #[test]
    fn wide_table_with_code_and_math_does_not_overlap() {
        expect_clean("02_wide_code.md");
    }

    #[test]
    fn code_output_plot_and_table_do_not_overlap() {
        expect_clean("03_code_plot_table.md");
    }

    #[test]
    fn many_row_table_stays_inside_the_page() {
        expect_clean("04_tall_table.md");
    }

    #[test]
    fn consecutive_tables_do_not_overlap() {
        expect_clean("05_consecutive.md");
    }

    #[test]
    fn table_in_a_list_and_after_a_quote_does_not_overlap() {
        expect_clean("06_list_quote.md");
    }

    #[test]
    fn long_lines_headings_and_output_stay_inside_the_margins() {
        expect_clean("07_long_lines.md");
    }

    #[test]
    fn table_near_a_page_break_does_not_overlap() {
        expect_clean("08_page_break.md");
    }

    /// Full sweep of `examples/notebooks`. Not part of `cargo test` — it
    /// compiles every notebook. Run with `--ignored`.
    #[test]
    #[ignore = "renders every notebook under examples/notebooks to PDF"]
    fn examples_notebooks_pdf_have_no_overlap() {
        if !crate::pdf_compile::which_exists("pdflatex") {
            eprintln!("skipping: pdflatex not installed");
            return;
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/notebooks");
        let mut files = Vec::new();
        walk_md(&root, &mut files);
        files.sort();
        let mut bad = Vec::new();
        for path in &files {
            let md = std::fs::read_to_string(path).unwrap();
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .display()
                .to_string();
            match render_pdf(&md) {
                Err(e) => bad.push(format!("{rel}: render failed: {e}")),
                Ok(pdf) => match pdf_layout_problems(&pdf) {
                    Err(e) => bad.push(format!("{rel}: {e}")),
                    Ok(problems) if problems.is_empty() => {}
                    Ok(problems) => bad.push(format!(
                        "{rel}: {} problem(s)\n{}",
                        problems.len(),
                        format_problems(&problems)
                    )),
                },
            }
        }
        assert!(
            bad.is_empty(),
            "{} notebook(s) failed the overlap check:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    fn walk_md(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                walk_md(&p, out);
            } else if p.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(p);
            }
        }
    }
}
