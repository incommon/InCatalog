// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Where a logo's artwork sits in its image, so the site can center it and fill the
//! tile with it (see [`LogoBox`]).
//!
//! The site shows logos on a white tile, so an image is measured as it looks there:
//! composited over white, the artwork is every pixel that isn't near white. That
//! crops away transparent and white margins alike, and also white artwork drawn for
//! a dark page, which is invisible on the tile anyway.
//!
//! - **Built-in margins** and an SVG `viewBox` larger than its artwork are cropped.
//! - **An SVG whose `width` and `height` don't match its `viewBox`** is drawn the way
//!   a browser draws it, `preserveAspectRatio` included, so artwork pinned to one side
//!   is found where it lands.
//! - **A square mark beside or above its words** (a shield, then the university's
//!   name) is cropped to the mark: in a square tile the words would only shrink it.
//!   A wordmark alone is kept whole.
//! - **Artwork opaque to the image's edges** (a colored square, a photo) fills the
//!   tile with no margin.
//!
//! SVG comes from thousands of registrants. It is drawn with no fonts and with every
//! reference to another file or URL refused, so measuring reads nothing but the
//! downloaded bytes. An SVG with text isn't measured, since its text can't be drawn.

use std::io::Cursor;
use std::ops::Range;

use catalog_core::model::LogoBox;
use resvg::{tiny_skia, usvg};

/// Images are measured at no more than this many pixels on their longer side.
const SIDE: u32 = 512;
/// A raster larger than this on either side isn't decoded.
const MAX_DECODE_SIDE: u32 = 4096;
const MAX_DECODE_BYTES: u64 = 128 << 20;
/// How far from white a pixel must be, in its furthest channel (of 255), to count as
/// artwork. Faint JPEG noise and anti-aliasing fringes stay below it.
const INK: u8 = 24;
/// The share of an image's border that must be artwork for it to fill the tile.
const BLEED: f32 = 0.9;

/// Measure an image's artwork. `None` when the bytes can't be decoded or drawn, the
/// image is all background, or it is an SVG with text.
pub fn measure(bytes: &[u8]) -> Option<LogoBox> {
    let ink = if is_svg(bytes) {
        draw_svg(bytes)?
    } else {
        decode_raster(bytes)?
    };
    ink.measure()
}

fn is_svg(b: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&b[..b.len().min(512)]).to_lowercase();
    head.contains("<svg") || head.starts_with("<?xml")
}

fn decode_raster(bytes: &[u8]) -> Option<Ink> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.into_rgba8();
    let (w, h) = rgba.dimensions();
    // Artwork is found at full size, then each cell of a grid at most SIDE across is
    // artwork if any pixel in it is. Scaling the image down first would blend the
    // color of transparent pixels (often black) into white artwork.
    let k = w.max(h).div_ceil(SIDE).max(1);
    let (gw, gh) = (w.div_ceil(k) as usize, h.div_ceil(k) as usize);
    let mut on = vec![false; gw * gh];
    for (x, y, p) in rgba.enumerate_pixels() {
        let [r, g, b, a] = p.0;
        // Over white: c·α + 255·(1 − α), so its distance from white is (255 − c)·α.
        let dist = |c: u8| ((255 - c) as u32 * a as u32 / 255) as u8;
        if dist(r).max(dist(g)).max(dist(b)) > INK {
            on[(y / k) as usize * gw + (x / k) as usize] = true;
        }
    }
    Some(Ink {
        w: gw,
        h: gh,
        aspect: w as f32 / h.max(1) as f32,
        on,
    })
}

fn draw_svg(bytes: &[u8]) -> Option<Ink> {
    if bytes.windows(5).any(|w| w.eq_ignore_ascii_case(b"<text")) {
        return None;
    }
    let opt = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            // Embedded data: URLs are part of the file; anything else would be a read.
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(bytes, &opt).ok()?;
    let size = tree.size();
    let scale = SIDE as f32 / size.width().max(size.height());
    let w = ((size.width() * scale).round() as u32).max(1);
    let h = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height()),
        &mut pixmap.as_mut(),
    );
    let on = pixmap
        .pixels()
        .iter()
        .map(|p| {
            // Premultiplied, so over white a channel is c + 255 − α.
            let dist = |c: u8| p.alpha() - c;
            dist(p.red()).max(dist(p.green())).max(dist(p.blue())) > INK
        })
        .collect();
    Some(Ink {
        w: w as usize,
        h: h as usize,
        aspect: size.width() / size.height(),
        on,
    })
}

/// Which pixels of an image are artwork.
struct Ink {
    w: usize,
    h: usize,
    /// The image's own width over its height (the grid may be scaled down).
    aspect: f32,
    on: Vec<bool>,
}

/// A half-open rectangle of pixels.
#[derive(Debug, Clone, PartialEq)]
struct Rect {
    x: Range<usize>,
    y: Range<usize>,
}

impl Rect {
    fn w(&self) -> f32 {
        self.x.len() as f32
    }
    fn h(&self) -> f32 {
        self.y.len() as f32
    }
    /// Width over height.
    fn shape(&self) -> f32 {
        self.w() / self.h()
    }
    fn squarish(&self) -> bool {
        (0.6..=1.0 / 0.6).contains(&self.shape())
    }
}

impl Ink {
    fn at(&self, x: usize, y: usize) -> bool {
        self.on[y * self.w + x]
    }

    fn measure(&self) -> Option<LogoBox> {
        let all = self.bounds(&Rect {
            x: 0..self.w,
            y: 0..self.h,
        })?;
        let full = |bleed| LogoBox {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            aspect: self.aspect,
            bleed,
            mark: false,
        };
        if self.border_ink() >= BLEED {
            return Some(full(true));
        }
        let (r, mark) = match self
            .mark_beside_words(&all)
            .or_else(|| self.mark_above_words(&all))
        {
            Some(m) => (m, true),
            None => (all, false),
        };
        Some(LogoBox {
            x: r.x.start as f32 / self.w as f32,
            y: r.y.start as f32 / self.h as f32,
            w: r.w() / self.w as f32,
            h: r.h() / self.h as f32,
            aspect: self.aspect,
            bleed: false,
            mark,
        })
    }

    /// The smallest rectangle holding all the artwork within `r`.
    fn bounds(&self, r: &Rect) -> Option<Rect> {
        let cols = runs(r.x.clone(), |x| r.y.clone().any(|y| self.at(x, y)));
        let rows = runs(r.y.clone(), |y| r.x.clone().any(|x| self.at(x, y)));
        Some(Rect {
            x: cols.first()?.start..cols.last()?.end,
            y: rows.first()?.start..rows.last()?.end,
        })
    }

    fn border_ink(&self) -> f32 {
        let (w, h) = (self.w, self.h);
        let edge = (0..w)
            .flat_map(|x| [(x, 0), (x, h - 1)])
            .chain((0..h).flat_map(|y| [(0, y), (w - 1, y)]));
        let (mut on, mut all) = (0, 0);
        for (x, y) in edge {
            all += 1;
            on += self.at(x, y) as usize;
        }
        on as f32 / all as f32
    }

    /// A square mark with words to its right, as in most horizontal lockups.
    ///
    /// The mark is the artwork before the first clear gap. It must be about square
    /// and at least as tall as the words, and what follows must read as words: one
    /// or more lines, each clearly shorter than the mark, spread across separate
    /// letters. A wordmark alone fails that test, since its first letter is as tall
    /// as the rest.
    fn mark_beside_words(&self, all: &Rect) -> Option<Rect> {
        let cols = runs(all.x.clone(), |x| all.y.clone().any(|y| self.at(x, y)));
        let split = first_gap(&cols, gap(all))?;
        let mark = self.bounds(&Rect {
            x: all.x.start..cols[split - 1].end,
            y: all.y.clone(),
        })?;
        let words = self.bounds(&Rect {
            x: cols[split].start..all.x.end,
            y: all.y.clone(),
        })?;
        let lines = runs(words.y.clone(), |y| words.x.clone().any(|x| self.at(x, y)));
        let letters = cols.len() - split;
        let ok = mark.squarish()
            && mark.h() >= 0.8 * words.h()
            && words.w() >= 0.5 * mark.w()
            && letters >= 2
            && lines.iter().all(|l| l.len() as f32 <= 0.75 * mark.h());
        ok.then_some(mark)
    }

    /// A square mark with words below it, as in stacked lockups. The same tests as
    /// [`Ink::mark_beside_words`], turned on their side: the words below are one or
    /// more lines, each clearly shorter than the mark, and no taller in all than it.
    fn mark_above_words(&self, all: &Rect) -> Option<Rect> {
        let rows = runs(all.y.clone(), |y| all.x.clone().any(|x| self.at(x, y)));
        let split = first_gap(&rows, gap(all))?;
        let mark = self.bounds(&Rect {
            x: all.x.clone(),
            y: all.y.start..rows[split - 1].end,
        })?;
        let words = self.bounds(&Rect {
            x: all.x.clone(),
            y: rows[split].start..all.y.end,
        })?;
        let lines = &rows[split..];
        let letters = runs(words.x.clone(), |x| words.y.clone().any(|y| self.at(x, y)));
        let ok = mark.squarish()
            && words.h() <= mark.h()
            && words.w() >= 0.5 * mark.w()
            && letters.len() >= 2
            && lines.iter().all(|l| l.len() as f32 <= 0.6 * mark.h());
        ok.then_some(mark)
    }
}

/// The least gap, in pixels, between a mark and its words: a tenth of the artwork's
/// shorter side, so the space between letters or words doesn't count.
fn gap(all: &Rect) -> usize {
    ((0.1 * all.w().min(all.h())).round() as usize).max(2)
}

/// The index of the first run that starts at least `gap` after the one before it.
fn first_gap(runs: &[Range<usize>], gap: usize) -> Option<usize> {
    (1..runs.len()).find(|&i| runs[i].start - runs[i - 1].end >= gap)
}

/// The runs of indices in `range` where `filled` holds.
fn runs(range: Range<usize>, filled: impl Fn(usize) -> bool) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for i in range {
        if !filled(i) {
            continue;
        }
        match out.last_mut() {
            Some(r) if r.end == i => r.end = i + 1,
            _ => out.push(i..i + 1),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    const CLEAR: Rgba<u8> = Rgba([0, 0, 0, 0]);
    const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
    const NAVY: Rgba<u8> = Rgba([30, 64, 124, 255]);

    /// A PNG of `w`×`h` filled with `bg`, with each `(x, y, w, h)` rectangle in `fg`.
    fn png(w: u32, h: u32, bg: Rgba<u8>, fg: Rgba<u8>, rects: &[(u32, u32, u32, u32)]) -> Vec<u8> {
        let mut img = RgbaImage::from_pixel(w, h, bg);
        for &(x, y, rw, rh) in rects {
            for yy in y..y + rh {
                for xx in x..x + rw {
                    img.put_pixel(xx, yy, fg);
                }
            }
        }
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// The box in pixels of a `w`×`h` image, and whether it is a mark. Pixels within
    /// one of `want` (images over 512 pixels are measured scaled down) read as `want`.
    fn px(b: LogoBox, w: u32, h: u32, want: (u32, u32, u32, u32)) -> ((u32, u32, u32, u32), bool) {
        let r = |v: f32, s: u32, want: u32| {
            let got = (v * s as f32).round() as u32;
            if got.abs_diff(want) <= 1 {
                want
            } else {
                got
            }
        };
        let got = (
            r(b.x, w, want.0),
            r(b.y, h, want.1),
            r(b.w, w, want.2),
            r(b.h, h, want.3),
        );
        (got, b.mark)
    }

    /// Letters: `n` blocks `lw` wide and `lh` tall from `(x, y)`, `gap` apart.
    fn letters(x: u32, y: u32, n: u32, lw: u32, lh: u32, gap: u32) -> Vec<(u32, u32, u32, u32)> {
        (0..n).map(|i| (x + i * (lw + gap), y, lw, lh)).collect()
    }

    #[test]
    fn crops_transparent_and_white_margins() {
        for bg in [CLEAR, WHITE] {
            let b = measure(&png(200, 100, bg, NAVY, &[(70, 20, 60, 60)])).unwrap();
            assert_eq!(px(b, 200, 100, (70, 20, 60, 60)), ((70, 20, 60, 60), false));
            assert!((b.aspect - 2.0).abs() < 1e-6);
            assert!(!b.bleed);
        }
    }

    #[test]
    fn finds_artwork_off_to_one_side() {
        let b = measure(&png(300, 100, CLEAR, NAVY, &[(5, 30, 40, 40)])).unwrap();
        assert_eq!(px(b, 300, 100, (5, 30, 40, 40)), ((5, 30, 40, 40), false));
    }

    #[test]
    fn white_artwork_is_invisible_on_the_tile() {
        // DIRISA's: an orange mark in a corner, white words for a dark page.
        let mut bytes_img = RgbaImage::from_pixel(649, 240, CLEAR);
        for y in 10..70 {
            for x in 570..630 {
                bytes_img.put_pixel(x, y, Rgba([247, 148, 62, 255]));
            }
        }
        for y in 100..200 {
            for x in 20..600 {
                bytes_img.put_pixel(x, y, WHITE);
            }
        }
        let mut out = Cursor::new(Vec::new());
        bytes_img.write_to(&mut out, ImageFormat::Png).unwrap();
        let b = measure(&out.into_inner()).unwrap();
        assert_eq!(
            px(b, 649, 240, (570, 10, 60, 60)),
            ((570, 10, 60, 60), false)
        );
    }

    #[test]
    fn keeps_a_square_mark_and_drops_the_words_beside_it() {
        let mut rects = vec![(10, 10, 80, 80)];
        rects.extend(letters(120, 30, 8, 18, 36, 6));
        let b = measure(&png(330, 100, CLEAR, NAVY, &rects)).unwrap();
        assert_eq!(px(b, 330, 100, (10, 10, 80, 80)), ((10, 10, 80, 80), true));

        // Two lines of words, together as tall as the mark.
        let mut rects = vec![(10, 10, 80, 80)];
        rects.extend(letters(120, 10, 8, 18, 34, 6));
        rects.extend(letters(120, 56, 6, 18, 34, 6));
        let b = measure(&png(330, 100, WHITE, NAVY, &rects)).unwrap();
        assert_eq!(px(b, 330, 100, (10, 10, 80, 80)), ((10, 10, 80, 80), true));
    }

    #[test]
    fn keeps_a_square_mark_and_drops_the_words_below_it() {
        let mut rects = vec![(60, 5, 80, 80)];
        rects.extend(letters(20, 100, 8, 16, 24, 4));
        let b = measure(&png(200, 130, CLEAR, NAVY, &rects)).unwrap();
        assert_eq!(px(b, 200, 130, (60, 5, 80, 80)), ((60, 5, 80, 80), true));
    }

    #[test]
    fn keeps_a_wordmark_whole() {
        // Letters all the same height: the first one isn't a mark, however square.
        let rects = letters(10, 20, 6, 50, 60, 20);
        let b = measure(&png(430, 100, CLEAR, NAVY, &rects)).unwrap();
        assert_eq!(
            px(b, 430, 100, (10, 20, 400, 60)),
            ((10, 20, 400, 60), false)
        );

        // A wide banner with no gap in it.
        let b = measure(&png(456, 150, CLEAR, NAVY, &[(8, 40, 440, 70)])).unwrap();
        assert_eq!(px(b, 456, 150, (8, 40, 440, 70)), ((8, 40, 440, 70), false));

        // A square mark followed by another mark of the same size, not words.
        let b = measure(&png(
            220,
            100,
            CLEAR,
            NAVY,
            &[(10, 10, 80, 80), (130, 10, 80, 80)],
        ))
        .unwrap();
        assert_eq!(
            px(b, 220, 100, (10, 10, 200, 80)),
            ((10, 10, 200, 80), false)
        );
    }

    #[test]
    fn opaque_to_the_edges_fills_the_tile() {
        let b = measure(&png(120, 120, NAVY, WHITE, &[(30, 30, 60, 60)])).unwrap();
        assert!(b.bleed);
        assert_eq!(px(b, 120, 120, (0, 0, 120, 120)), ((0, 0, 120, 120), false));
    }

    #[test]
    fn all_background_has_no_box() {
        assert_eq!(measure(&png(64, 64, CLEAR, NAVY, &[])), None);
        assert_eq!(measure(&png(64, 64, WHITE, NAVY, &[])), None);
        assert_eq!(measure(b"<!doctype html><html>Not found</html>"), None);
    }

    #[test]
    fn crops_an_svg_with_a_padded_view_box() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 128"><rect x="128" y="32" width="64" height="64" fill="#1e407c"/></svg>"##;
        let b = measure(svg).unwrap();
        assert_eq!(
            px(b, 256, 128, (128, 32, 64, 64)),
            ((128, 32, 64, 64), false)
        );
        assert!((b.aspect - 2.0).abs() < 1e-6);
    }

    #[test]
    fn places_an_svg_as_a_browser_does_with_mismatched_dimensions() {
        // A square viewBox in a 4:1 image, pinned to the left by xMinYMid.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="128" viewBox="0 0 128 128" preserveAspectRatio="xMinYMid meet"><rect x="16" y="16" width="96" height="96" fill="#1e407c"/></svg>"##;
        let b = measure(svg).unwrap();
        assert_eq!(px(b, 512, 128, (16, 16, 96, 96)), ((16, 16, 96, 96), false));
        assert!((b.aspect - 4.0).abs() < 1e-6);
    }

    #[test]
    fn svg_references_are_never_followed() {
        // Only the rectangle is drawn: neither the local file nor the URL is read.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 128 128">
            <image href="file:///etc/hosts" x="0" y="0" width="128" height="128"/>
            <image xlink:href="https://example.org/big.png" x="0" y="0" width="128" height="128"/>
            <image href="/etc/hosts" x="0" y="0" width="128" height="128"/>
            <rect x="48" y="48" width="32" height="32" fill="#000"/></svg>"##;
        let b = measure(svg).unwrap();
        assert_eq!(px(b, 128, 128, (48, 48, 32, 32)), ((48, 48, 32, 32), false));
    }

    #[test]
    fn refuses_what_it_cannot_measure_safely() {
        // Text can't be drawn without fonts, so its box would be wrong.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><text x="0" y="40">Example</text></svg>"#;
        assert_eq!(measure(svg), None);
        // Cut off at the download limit.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><rect x="0" y="0" wid"#;
        assert_eq!(measure(svg), None);
        // A PNG that declares itself 20000 pixels wide is not decoded.
        let mut huge = png(4, 4, CLEAR, NAVY, &[(0, 0, 2, 2)]);
        huge[16..20].copy_from_slice(&20000u32.to_be_bytes());
        assert_eq!(measure(&huge), None);
    }
}
