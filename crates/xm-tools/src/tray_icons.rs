//! Draws the tray icons: a headphone glyph with a mode badge between the ear cups (filled disc
//! for noise cancelling, ring for ambient, none for off), plus a faded plain glyph for
//! disconnected. White for a dark taskbar, near-black for a light one. The badge is sized so the
//! ring's hole is still three clear pixels across at 16 px, where disc and ring must differ.
//!
//! usage: tray-icons <out-dir>   (the app's copy lives in crates/xm-reactor/assets/tray)

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::{ExtendedColorType, Rgba, RgbaImage};
use std::process::ExitCode;

const SIZES: [u32; 6] = [16, 20, 24, 32, 40, 48];
const DISCONNECTED_ALPHA: f64 = 0.45;
/// Sub-samples per pixel side, for anti-aliased edges.
const SS: u32 = 8;
/// The badge's centre and outer radius, and the radius of the ring's hole (unit square, y down).
const BADGE: (f64, f64) = (0.5, 0.64);
const BADGE_R: f64 = 0.2;
const RING_HOLE_R: f64 = 0.105;

#[derive(Clone, Copy, PartialEq)]
enum Badge {
    Disc,
    Ring,
    None,
}

/// (file stem, badge, faded)
const GLYPHS: [(&str, Badge, bool); 4] = [
    ("nc", Badge::Disc, false),
    ("ambient", Badge::Ring, false),
    ("off", Badge::None, false),
    ("disconnected", Badge::None, true),
];

/// (file suffix, glyph colour)
const THEMES: [(&str, [u8; 3]); 2] = [("dark", [255, 255, 255]), ("light", [28, 28, 28])];

fn rounded_rect(x: f64, y: f64, (l, t, r, b): (f64, f64, f64, f64), radius: f64) -> bool {
    let cx = x.clamp(l + radius, r - radius);
    let cy = y.clamp(t + radius, b - radius);
    x >= l && x <= r && y >= t && y <= b && (x - cx).hypot(y - cy) <= radius
}

/// Whether the point (unit square, y down) is inside the glyph.
fn inside(x: f64, y: f64, badge: Badge) -> bool {
    // Headband: the top half of a ring.
    let d = (x - 0.5).hypot(y - 0.52);
    let band = y <= 0.52 && (0.30..=0.40).contains(&d);
    let cups = [(0.08, 0.46, 0.26, 0.88), (0.74, 0.46, 0.92, 0.88)]
        .into_iter()
        .any(|r| rounded_rect(x, y, r, 0.08));
    let b = (x - BADGE.0).hypot(y - BADGE.1);
    let badge = match badge {
        Badge::Disc => b <= BADGE_R,
        Badge::Ring => (RING_HOLE_R..=BADGE_R).contains(&b),
        Badge::None => false,
    };
    band || cups || badge
}

fn draw(size: u32, badge: Badge, rgb: [u8; 3], alpha: f64) -> RgbaImage {
    RgbaImage::from_fn(size, size, |px, py| {
        let mut hits = 0;
        for sy in 0..SS {
            for sx in 0..SS {
                let x = (f64::from(px * SS + sx) + 0.5) / f64::from(size * SS);
                let y = (f64::from(py * SS + sy) + 0.5) / f64::from(size * SS);
                hits += u32::from(inside(x, y, badge));
            }
        }
        let a = f64::from(hits) / f64::from(SS * SS) * alpha * 255.0;
        Rgba([rgb[0], rgb[1], rgb[2], a.round() as u8])
    })
}

fn run(args: &[String]) -> Result<(), String> {
    let [out] = args else {
        return Err("usage: tray-icons <out-dir>".into());
    };
    std::fs::create_dir_all(out).map_err(|e| format!("{out}: {e}"))?;
    for (stem, badge, faded) in GLYPHS {
        for (theme, rgb) in THEMES {
            let alpha = if faded { DISCONNECTED_ALPHA } else { 1.0 };
            let images: Vec<RgbaImage> =
                SIZES.iter().map(|&s| draw(s, badge, rgb, alpha)).collect();
            let frames = images
                .iter()
                .map(|img| {
                    let (w, h) = img.dimensions();
                    IcoFrame::as_png(img.as_raw(), w, h, ExtendedColorType::Rgba8)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let path = std::path::Path::new(out).join(format!("{stem}-{theme}.ico"));
            let file =
                std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            IcoEncoder::new(std::io::BufWriter::new(file))
                .encode_images(&frames)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 3] = [255, 255, 255];

    /// Alpha at the badge centre and on the ring's stroke, at `size` px.
    fn badge_alpha(size: u32, badge: Badge) -> (u8, u8) {
        let img = draw(size, badge, WHITE, 1.0);
        let px = |v: f64| (v * f64::from(size)) as u32;
        let at = |x: f64, y: f64| img.get_pixel(px(x), px(y)).0[3];
        let stroke = (BADGE_R + RING_HOLE_R) / 2.0;
        (at(BADGE.0, BADGE.1), at(BADGE.0, BADGE.1 + stroke))
    }

    #[test]
    fn badges_tell_the_modes_apart() {
        assert_eq!(badge_alpha(48, Badge::Disc), (255, 255));
        assert_eq!(badge_alpha(48, Badge::Ring), (0, 255));
        assert_eq!(badge_alpha(48, Badge::None), (0, 0));
    }

    #[test]
    fn disc_and_ring_differ_at_16_px() {
        let (disc, _) = badge_alpha(16, Badge::Disc);
        let (hole, stroke) = badge_alpha(16, Badge::Ring);
        assert_eq!(disc, 255);
        assert!(hole < 40, "the ring's hole fills in at 16 px: {hole}");
        assert!(stroke > 200, "the ring's stroke fades at 16 px: {stroke}");
    }

    #[test]
    fn disconnected_is_faded_and_themes_set_the_colour() {
        let img = draw(48, Badge::None, [28, 28, 28], DISCONNECTED_ALPHA);
        let cup = img.get_pixel((0.18 * 48.0) as u32, (0.7 * 48.0) as u32).0;
        assert_eq!(cup, [28, 28, 28, 115]);
        // The corners stay clear.
        assert_eq!(img.get_pixel(0, 0).0[3], 0);
    }
}
