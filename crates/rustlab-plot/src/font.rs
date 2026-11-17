//! The one font plotters draws and measures text with.
//!
//! plotters' `ab_glyph` backend has no system lookup: a family name resolves
//! only if it was registered here. Every SVG/PNG/GIF/viewer-raster path calls
//! [`ensure_registered`] before it builds a `TextStyle`, so `"sans-serif"`
//! is the bundled Liberation Sans on every OS, with no fontconfig, FreeType,
//! or C++ toolchain at build time. License: `fonts/OFL.txt`.
//!
//! Size convention: the SVG backend writes `font-size = size / 1.24`, and
//! plotters' old `ttf` backend measured and drew with that em. `ab_glyph`
//! instead scales so that `ascender - descender == size`, which for this
//! face is an em about 11% larger — text measured wider than the SVG draws
//! it (fewer ticks) and PNG glyphs larger than before. The registered copy
//! has its vertical metrics rescaled so `ascender - descender` is
//! `EM_DIVISOR × unitsPerEm`; outlines and advances are untouched.

use std::sync::OnceLock;

/// Liberation Sans 2.1.5 Regular, unmodified (SIL Open Font License 1.1).
pub static SANS_REGULAR: &[u8] = include_bytes!("../fonts/LiberationSans-Regular.ttf");

/// The family name plot text uses.
pub const SANS: &str = "sans-serif";

/// plotters' size-to-em ratio (`ttf` backend and SVG `font-size`).
pub const EM_DIVISOR: f64 = 1.24;

static REGISTERED: OnceLock<&'static [u8]> = OnceLock::new();

/// Register the bundled face as `"sans-serif"`. Idempotent and cheap after
/// the first call. Bold and italic requests fall back to this face.
pub fn ensure_registered() {
    REGISTERED.get_or_init(|| {
        let bytes: &'static [u8] = Box::leak(
            with_em_height(SANS_REGULAR, EM_DIVISOR)
                .expect("bundled font has head/hhea tables")
                .into_boxed_slice(),
        );
        if plotters::style::register_font(SANS, plotters::style::FontStyle::Normal, bytes).is_err()
        {
            panic!("bundled LiberationSans-Regular.ttf failed to parse");
        }
        bytes
    });
}

fn be_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn table(b: &[u8], tag: &[u8; 4]) -> Option<usize> {
    let n = be_u16(b, 4)? as usize;
    (0..n).find_map(|i| {
        let rec = 12 + 16 * i;
        (b.get(rec..rec + 4)? == tag).then(|| be_u32(b, rec + 8).map(|o| o as usize))?
    })
}

/// Copy of `font` whose `hhea` and `OS/2` typographic ascender/descender are
/// scaled together so that `ascender - descender == ratio × unitsPerEm`.
/// Table checksums are not updated; ttf-parser does not verify them.
fn with_em_height(font: &[u8], ratio: f64) -> Option<Vec<u8>> {
    let mut out = font.to_vec();
    let upm = be_u16(font, table(font, b"head")? + 18)? as f64;
    let target = ratio * upm;
    let mut rescale = |asc_at: usize| -> Option<()> {
        let asc = be_u16(font, asc_at)? as i16 as f64;
        let desc = be_u16(font, asc_at + 2)? as i16 as f64;
        if asc - desc <= 0.0 {
            return Some(());
        }
        let k = target / (asc - desc);
        let new_asc = (asc * k).round();
        let new_desc = new_asc - target.round();
        out[asc_at..asc_at + 2].copy_from_slice(&(new_asc as i16).to_be_bytes());
        out[asc_at + 2..asc_at + 4].copy_from_slice(&(new_desc as i16).to_be_bytes());
        Some(())
    };
    rescale(table(font, b"hhea")? + 4)?;
    if let Some(os2) = table(font, b"OS/2") {
        rescale(os2 + 68)?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotters::style::{FontDesc, FontFamily, FontStyle};

    #[test]
    fn sans_serif_resolves_and_measures() {
        ensure_registered();
        let font = FontDesc::new(FontFamily::SansSerif, 20.0, FontStyle::Normal);
        let (w_short, h) = font.box_size("x").expect("sans-serif registered");
        let (w_long, _) = font.box_size("xxxxxxxxxx").unwrap();
        assert!(h > 0 && w_short > 0);
        assert!(w_long > 5 * w_short, "{w_long} vs {w_short}");
    }

    #[test]
    fn measured_em_matches_svg_font_size() {
        // Liberation Sans digits advance 1139/2048 em (Arial metrics). At
        // plotters size 124 the em is 100 px, so ten digits are ~556 px.
        ensure_registered();
        let font = FontDesc::new(FontFamily::SansSerif, 124.0, FontStyle::Normal);
        let (w, _) = font.box_size("0123456789").unwrap();
        let expected = 10.0 * 1139.0 / 2048.0 * 100.0;
        assert!(
            (w as f64 - expected).abs() <= 2.0,
            "width {w}, expected {expected:.1}"
        );
    }

    #[test]
    fn rescale_keeps_outlines_and_hits_ratio() {
        let patched = with_em_height(SANS_REGULAR, EM_DIVISOR).unwrap();
        assert_eq!(patched.len(), SANS_REGULAR.len());
        let hhea = table(&patched, b"hhea").unwrap();
        let asc = be_u16(&patched, hhea + 4).unwrap() as i16 as i32;
        let desc = be_u16(&patched, hhea + 6).unwrap() as i16 as i32;
        assert_eq!(asc - desc, (EM_DIVISOR * 2048.0).round() as i32);
        let glyf = table(SANS_REGULAR, b"glyf").unwrap();
        assert_eq!(patched[glyf..glyf + 4096], SANS_REGULAR[glyf..glyf + 4096]);
    }

    #[test]
    fn bold_falls_back_to_regular() {
        ensure_registered();
        let normal = FontDesc::new(FontFamily::SansSerif, 14.0, FontStyle::Normal);
        let bold = FontDesc::new(FontFamily::SansSerif, 14.0, FontStyle::Bold);
        assert_eq!(
            normal.box_size("Hz 10³").unwrap(),
            bold.box_size("Hz 10³").unwrap()
        );
    }
}
