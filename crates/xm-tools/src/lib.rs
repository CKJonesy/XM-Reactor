//! Cuts a product render out of its white studio background.

use image::{imageops, GrayImage, Luma, RgbImage, Rgba, RgbaImage};

/// Enclosed white regions bigger than this share of the image are background too.
const HOLE_FRACTION: f64 = 0.002;
const WHITE_MIN: u8 = 245;
/// Pixels this close to the cut edge are recoloured from the interior.
const EDGE_BAND: u8 = 3;

const NEIGHBOURS: [(i64, i64); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

fn is_white(rgb: &RgbImage, x: u32, y: u32) -> bool {
    rgb.get_pixel(x, y).0.iter().all(|&c| c >= WHITE_MIN)
}

/// Labels every connected white region; returns (labels, sizes, touches_border) per label.
fn white_regions(rgb: &RgbImage) -> (Vec<u32>, Vec<usize>, Vec<bool>) {
    let (w, h) = rgb.dimensions();
    let mut label = vec![0u32; (w * h) as usize];
    let (mut sizes, mut border) = (vec![0usize], vec![false]);
    for sy in 0..h {
        for sx in 0..w {
            if label[(sy * w + sx) as usize] != 0 || !is_white(rgb, sx, sy) {
                continue;
            }
            let id = sizes.len() as u32;
            let (mut size, mut edge) = (0, false);
            label[(sy * w + sx) as usize] = id;
            let mut stack = vec![(sx, sy)];
            while let Some((x, y)) = stack.pop() {
                size += 1;
                edge |= x == 0 || y == 0 || x == w - 1 || y == h - 1;
                for (dx, dy) in NEIGHBOURS {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let (nx, ny) = (nx as u32, ny as u32);
                    let i = (ny * w + nx) as usize;
                    if label[i] == 0 && is_white(rgb, nx, ny) {
                        label[i] = id;
                        stack.push((nx, ny));
                    }
                }
            }
            sizes.push(size);
            border.push(edge);
        }
    }
    (label, sizes, border)
}

/// Replaces the colour of pixels near the cut edge with the nearest interior colour,
/// so the white studio background cannot show as a light rim once alpha is soft.
fn decontaminate(rgb: &RgbImage, keep: &[bool]) -> RgbImage {
    let (w, h) = rgb.dimensions();
    let step = |i: usize| {
        let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
        NEIGHBOURS.iter().filter_map(move |&(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            (nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64)
                .then(|| (ny as u32 * w + nx as u32) as usize)
        })
    };
    // Distance (capped) from each kept pixel to the removed background.
    let mut dist = vec![u8::MAX; keep.len()];
    let mut queue: Vec<usize> = (0..keep.len()).filter(|&i| !keep[i]).collect();
    queue.iter().for_each(|&i| dist[i] = 0);
    for d in 0..EDGE_BAND {
        let mut next = Vec::new();
        for &i in &queue {
            for n in step(i) {
                if dist[n] == u8::MAX {
                    dist[n] = d + 1;
                    next.push(n);
                }
            }
        }
        queue = next;
    }
    // Spread interior colour outwards over the band and the first pixels of background.
    let mut out = rgb.clone();
    let mut done: Vec<bool> = (0..keep.len())
        .map(|i| keep[i] && dist[i] >= EDGE_BAND)
        .collect();
    let mut queue: Vec<usize> = (0..keep.len()).filter(|&i| done[i]).collect();
    for _ in 0..EDGE_BAND * 2 {
        let mut next = Vec::new();
        for &i in &queue {
            let c = *out.get_pixel(i as u32 % w, i as u32 / w);
            for n in step(i) {
                if !done[n] {
                    done[n] = true;
                    out.put_pixel(n as u32 % w, n as u32 / w, c);
                    next.push(n);
                }
            }
        }
        queue = next;
    }
    out
}
pub fn cut_out(rgb: &RgbImage) -> RgbaImage {
    let (w, h) = rgb.dimensions();
    let (label, sizes, border) = white_regions(rgb);
    let hole_min = (w as f64 * h as f64 * HOLE_FRACTION) as usize;
    let mut keep: Vec<bool> = (0..label.len())
        .map(|i| {
            let l = label[i] as usize;
            l == 0 || !(border[l] || sizes[l] > hole_min)
        })
        .collect();

    // Choke one pixel of light anti-aliasing fringe so no white halo survives.
    let snapshot = keep.clone();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let light = rgb.get_pixel(x, y).0.iter().all(|&c| c > 200);
            if snapshot[i] && light {
                keep[i] = NEIGHBOURS.iter().all(|&(dx, dy)| {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    nx < 0
                        || ny < 0
                        || nx >= w as i64
                        || ny >= h as i64
                        || snapshot[(ny as u32 * w + nx as u32) as usize]
                });
            }
        }
    }

    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    let mut hard = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            if keep[(y * w + x) as usize] {
                hard.put_pixel(x, y, Luma([255]));
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x1 <= x0 {
        return RgbaImage::new(1, 1);
    }
    let soft = imageops::blur(&hard, 1.0);
    let rgb = &decontaminate(rgb, &keep);
    let mut out = RgbaImage::new(x1 - x0, y1 - y0);
    for (x, y, px) in out.enumerate_pixels_mut() {
        let [r, g, b] = rgb.get_pixel(x + x0, y + y0).0;
        *px = Rgba([r, g, b, soft.get_pixel(x + x0, y + y0)[0]]);
    }
    out
}

/// Resizes to `height`, premultiplying alpha so the old white background cannot bleed in.
pub fn resize_to_height(img: &RgbaImage, height: u32) -> RgbaImage {
    let width = ((img.width() as f64 * height as f64 / img.height() as f64).round() as u32).max(1);
    let mut pre = img.clone();
    for px in pre.pixels_mut() {
        let a = px[3] as u32;
        for c in &mut px.0[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    let mut out = imageops::resize(&pre, width, height, imageops::FilterType::Lanczos3);
    for px in out.pixels_mut() {
        let a = px[3] as u32;
        for c in &mut px.0[..3] {
            if let Some(v) = (*c as u32 * 255 + a / 2).checked_div(a) {
                *c = v.min(255) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: image::Rgb<u8> = image::Rgb([255, 255, 255]);
    const GREY: image::Rgb<u8> = image::Rgb([60, 60, 70]);

    /// 100x100 white canvas with a filled rectangle.
    fn blob(x0: u32, y0: u32, x1: u32, y1: u32) -> RgbImage {
        let mut img = RgbImage::from_pixel(100, 100, WHITE);
        for y in y0..y1 {
            for x in x0..x1 {
                img.put_pixel(x, y, GREY);
            }
        }
        img
    }

    fn alpha_at(img: &RgbaImage, x: u32, y: u32) -> u8 {
        img.get_pixel(x, y)[3]
    }

    #[test]
    fn border_white_removed() {
        // A notch cut in from the left edge stays connected to the border.
        let mut img = blob(20, 20, 80, 80);
        for y in 45..55 {
            for x in 0..40 {
                img.put_pixel(x, y, WHITE);
            }
        }
        let out = cut_out(&img);
        assert_eq!(alpha_at(&out, 5, 30), 0);
        assert_eq!(alpha_at(&out, 40, 30), 255);
    }

    #[test]
    fn small_border_white_removed() {
        // 3x3 white patch (0.09 % of the image) touching the left edge of a full-frame body.
        let mut img = RgbImage::from_pixel(100, 100, GREY);
        for y in 40..43 {
            for x in 0..3 {
                img.put_pixel(x, y, WHITE);
            }
        }
        let out = cut_out(&img);
        // The hole is only 3 px wide, so the soft edge leaves it partly transparent.
        assert!(alpha_at(&out, 0, 41) < 128);
        assert_eq!(alpha_at(&out, 50, 50), 255);
    }

    #[test]
    fn enclosed_large_white_removed() {
        // 60x60 ring with a 20x20 white hole (4 % of the image).
        let mut img = blob(20, 20, 80, 80);
        for y in 40..60 {
            for x in 40..60 {
                img.put_pixel(x, y, WHITE);
            }
        }
        let out = cut_out(&img);
        // Image trims to the ring (offset 20), so the hole centre is at 30,30.
        assert_eq!(alpha_at(&out, 30, 30), 0);
        assert_eq!(alpha_at(&out, 5, 5), 255);
    }

    #[test]
    fn small_interior_highlight_kept() {
        // 4x4 white highlight (0.16 % of the image) inside the body.
        let mut img = blob(20, 20, 80, 80);
        for y in 48..52 {
            for x in 48..52 {
                img.put_pixel(x, y, WHITE);
            }
        }
        let out = cut_out(&img);
        assert_eq!(alpha_at(&out, 29, 29), 255);
    }

    #[test]
    fn trimmed_to_content() {
        let out = cut_out(&blob(30, 40, 70, 90));
        assert_eq!((out.width(), out.height()), (40, 50));
    }
}
