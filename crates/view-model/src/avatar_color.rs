//! The colour of a call card for someone without video: the dominant colour
//! of their profile picture (Color Thief's MMCQ palette, scored for
//! colourfulness), kept vivid but dark enough for white text on it.

/// Most samples looked at: a strided pick over the picture (about 32x32).
const MAX_SAMPLES: usize = 1024;
/// Swatches asked of Color Thief.
const PALETTE: u8 = 6;

/// HSV saturation of an RGB colour, 0 to 1.
fn hsv_saturation([r, g, b]: [u8; 3]) -> f32 {
    let max = r.max(g).max(b);
    if max == 0 {
        return 0.0;
    }
    f32::from(max - r.min(g).min(b)) / f32::from(max)
}

fn dist2(a: [u8; 3], b: [u8; 3]) -> i32 {
    (0..3)
        .map(|i| {
            let d = i32::from(a[i]) - i32::from(b[i]);
            d * d
        })
        .sum()
}

/// The dominant colour of RGBA pixels (4 bytes each): a strided sample of
/// the opaque pixels, without the near-white and near-black ones (unless
/// that leaves almost nothing, so a black-and-white picture still gets a
/// colour), goes through Color Thief for a palette of swatches. Each swatch
/// scores population times saturation squared (plus a small epsilon, so an
/// all-grey picture picks its most common grey) and the winner's pixels'
/// mean is returned. `None` for no pixels or only transparent ones.
pub fn dominant_rgba(pixels: &[u8]) -> Option<[u8; 3]> {
    let count = pixels.len() / 4;
    let stride = count.div_ceil(MAX_SAMPLES).max(1);
    let opaque: Vec<[u8; 3]> = pixels
        .chunks_exact(4)
        .step_by(stride)
        .filter(|p| p[3] >= 128)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    if opaque.is_empty() {
        return None;
    }
    let coloured: Vec<[u8; 3]> = opaque
        .iter()
        .copied()
        .filter(|c| !c.iter().all(|&v| v > 240) && !c.iter().all(|&v| v < 15))
        .collect();
    let samples = if coloured.len() * 20 < opaque.len() {
        opaque
    } else {
        coloured
    };
    let buffer: Vec<u8> = samples.iter().flatten().copied().collect();
    let mut swatches: Vec<[u8; 3]> =
        color_thief::get_palette(&buffer, color_thief::ColorFormat::Rgb, 1, PALETTE)
            .map(|p| p.iter().map(|c| [c.r, c.g, c.b]).collect())
            .unwrap_or_default();
    if swatches.is_empty() {
        swatches.push(samples[0]);
    }
    // Populations: Color Thief does not return them, so assign each sample
    // to its nearest swatch and keep the mean of what each one gets.
    let mut sums = vec![([0u32; 3], 0u32); swatches.len()];
    for c in &samples {
        let k = (0..swatches.len())
            .min_by_key(|&k| dist2(swatches[k], *c))
            .unwrap_or(0);
        (0..3).for_each(|i| sums[k].0[i] += u32::from(c[i]));
        sums[k].1 += 1;
    }
    let mut best: Option<(f32, [u8; 3])> = None;
    for (sum, n) in sums.into_iter().filter(|(_, n)| *n > 0) {
        let mean = sum.map(|c| ((c + n / 2) / n) as u8);
        let saturation = hsv_saturation(mean);
        let score = n as f32 * (saturation * saturation + 0.01);
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, mean));
        }
    }
    best.map(|(_, c)| c)
}

/// The card colour (RRGGBB) for a picture whose dominant colour is
/// `average`: the same hue and (up to 85%) the same saturation, so a
/// colourful picture gets a vivid card, at a lightness of 28% to 40% (a
/// lighter picture a lighter card), darkened until white text on it has a
/// WCAG contrast of at least 4.5 (the AA level for text).
pub fn card_color(average: [u8; 3]) -> u32 {
    let [r, g, b] = average.map(|c| f64::from(c) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    let saturation = if delta == 0.0 {
        0.0
    } else {
        delta / (1.0 - (2.0 * lightness - 1.0).abs())
    };
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    let saturation = saturation.min(0.85);
    let mut lightness = 0.28 + 0.12 * lightness;
    loop {
        let rgb = hsl_to_rgb(hue, saturation, lightness);
        if lightness <= 0.0 || contrast_with_white(rgb) >= 4.5 {
            return rgb;
        }
        lightness -= 0.005;
    }
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> u32 {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    let byte = |c: f64| ((c + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// WCAG contrast of white text on `rgb`.
fn contrast_with_white(rgb: u32) -> f64 {
    let lin = |c: u32| {
        let c = f64::from(c as u8) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = 0.2126 * lin(rgb >> 16) + 0.7152 * lin(rgb >> 8) + 0.0722 * lin(rgb);
    1.05 / (l + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels(rgb: u32) -> [f64; 3] {
        [(rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff].map(|c| f64::from(c as u8))
    }

    /// WCAG relative luminance.
    fn luminance(rgb: u32) -> f64 {
        let [r, g, b] = channels(rgb).map(|c| {
            let c = c / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn contrast_with_white(rgb: u32) -> f64 {
        1.05 / (luminance(rgb) + 0.05)
    }

    fn solid(rgb: [u8; 3], n: usize) -> Vec<u8> {
        (0..n).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect()
    }

    fn is_red(c: [u8; 3]) -> bool {
        c[0] > 200 && c[1] < 60 && c[2] < 60
    }

    #[test]
    fn a_white_background_does_not_wash_out_the_subject() {
        let mut pixels = solid([255, 255, 255], 60);
        pixels.extend(solid([220, 20, 20], 40));
        assert!(is_red(dominant_rgba(&pixels).unwrap()));
    }

    #[test]
    fn half_red_half_blue_is_one_of_them_not_purple() {
        let mut pixels = solid([220, 20, 20], 50);
        pixels.extend(solid([20, 20, 220], 50));
        let c = dominant_rgba(&pixels).unwrap();
        assert!(is_red(c) || (c[2] > 200 && c[0] < 60), "{c:?}");
    }

    #[test]
    fn a_mostly_blue_picture_with_a_few_dark_pixels_is_blue() {
        let mut pixels = solid([20, 60, 220], 85);
        pixels.extend(solid([10, 10, 10], 15));
        let c = dominant_rgba(&pixels).unwrap();
        assert!(c[2] > 200 && c[0] < 60, "{c:?}");
    }

    #[test]
    fn the_same_picture_gives_the_same_colour() {
        let pixels: Vec<u8> = (0..4000u32)
            .flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 255])
            .collect();
        assert_eq!(dominant_rgba(&pixels), dominant_rgba(&pixels));
    }

    #[test]
    fn grey_pictures_give_that_grey() {
        assert_eq!(
            dominant_rgba(&solid([120, 120, 120], 50)),
            Some([120, 120, 120])
        );
        assert_eq!(dominant_rgba(&[10, 20, 30, 255]), Some([10, 20, 30]));
    }

    #[test]
    fn transparent_pixels_do_not_count() {
        assert_eq!(
            dominant_rgba(&[200, 0, 0, 255, 255, 255, 255, 0]),
            Some([200, 0, 0])
        );
        let mut pixels = solid([0, 0, 255], 5);
        pixels.extend([255, 0, 0, 0].repeat(50));
        assert_eq!(dominant_rgba(&pixels), Some([0, 0, 255]));
        assert_eq!(dominant_rgba(&[255, 255, 255, 0]), None);
        assert_eq!(dominant_rgba(&[]), None);
    }

    #[test]
    fn white_text_reads_on_any_card_colour() {
        for average in [
            [255, 255, 255],
            [255, 255, 0],
            [0, 255, 255],
            [255, 0, 0],
            [0, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 128, 0],
            [128, 128, 128],
        ] {
            let card = card_color(average);
            assert!(
                contrast_with_white(card) >= 4.5,
                "{average:?} -> {card:06x}"
            );
        }
    }

    fn hsl_saturation(rgb: u32) -> f64 {
        let [r, g, b] = channels(rgb).map(|c| c / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        if max == min {
            return 0.0;
        }
        (max - min) / (1.0 - (max + min - 1.0).abs())
    }

    #[test]
    fn saturated_pictures_give_vivid_cards() {
        for (average, dominant) in [
            ([220, 20, 20], 0usize),
            ([20, 200, 40], 1),
            ([20, 40, 230], 2),
        ] {
            let pixels = solid(average, 100);
            let card = card_color(dominant_rgba(&pixels).unwrap());
            assert!(hsl_saturation(card) >= 0.6, "{average:?} -> {card:06x}");
            let ch = channels(card);
            assert!(
                ch[dominant] > ch[(dominant + 1) % 3] && ch[dominant] > ch[(dominant + 2) % 3],
                "{card:06x}"
            );
        }
    }

    #[test]
    fn a_white_background_with_a_red_subject_gives_a_red_card() {
        let mut pixels = solid([255, 255, 255], 70);
        pixels.extend(solid([220, 20, 20], 30));
        let ch = channels(card_color(dominant_rgba(&pixels).unwrap()));
        assert!(ch[0] > ch[1] + 60.0 && ch[0] > ch[2] + 60.0, "{ch:?}");
    }

    #[test]
    fn a_greyscale_picture_gives_a_grey_card() {
        let mut pixels = solid([200, 200, 200], 40);
        pixels.extend(solid([90, 90, 90], 60));
        let card = card_color(dominant_rgba(&pixels).unwrap());
        assert!(hsl_saturation(card) < 0.05, "{card:06x}");
    }

    #[test]
    fn a_black_and_white_picture_still_gets_a_colour() {
        let mut pixels = solid([255, 255, 255], 50);
        pixels.extend(solid([0, 0, 0], 50));
        assert!(dominant_rgba(&pixels).is_some());
    }

    #[test]
    fn the_card_keeps_the_pictures_hue() {
        let red = channels(card_color([230, 30, 30]));
        assert!(red[0] > red[1] + 15.0 && red[0] > red[2] + 15.0, "{red:?}");
        let blue = channels(card_color([30, 40, 230]));
        assert!(blue[2] > blue[0] + 15.0 && blue[2] > blue[1] + 15.0, "{blue:?}");
    }

    #[test]
    fn a_picture_brighter_than_its_card_stays_distinct() {
        // A light picture is drawn on a clearly darker card, so it shows.
        let card = card_color([240, 240, 240]);
        assert!(luminance(card) < 0.15, "{card:06x}");
        assert_ne!(card_color([240, 240, 240]), card_color([20, 20, 20]));
    }
}
