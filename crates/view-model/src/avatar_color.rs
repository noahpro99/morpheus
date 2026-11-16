//! The colour of a call card for someone without video: the dominant colour
//! of their profile picture, darkened and desaturated so white text and the
//! picture itself stay readable on it.

/// Most samples looked at: a strided pick over the picture (about 32x32).
const MAX_SAMPLES: usize = 1024;
const CLUSTERS: usize = 4;
const ITERATIONS: usize = 6;

fn to_oklab([r, g, b]: [u8; 3]) -> [f32; 3] {
    let lin = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

/// The dominant colour of RGBA pixels (4 bytes each): a strided sample of
/// the opaque pixels is clustered (k-means in Oklab, seeded
/// deterministically), and the cluster that is big, colourful and neither
/// near-white nor near-black wins; its pixels' mean is returned. `None`
/// for no pixels or only transparent ones.
pub fn dominant_rgba(pixels: &[u8]) -> Option<[u8; 3]> {
    let count = pixels.len() / 4;
    let stride = count.div_ceil(MAX_SAMPLES).max(1);
    let samples: Vec<([u8; 3], [f32; 3])> = pixels
        .chunks_exact(4)
        .step_by(stride)
        .filter(|p| p[3] >= 128)
        .map(|p| {
            let rgb = [p[0], p[1], p[2]];
            (rgb, to_oklab(rgb))
        })
        .collect();
    if samples.is_empty() {
        return None;
    }
    // Farthest-point seeding from the first sample: deterministic.
    let mut centres = vec![samples[0].1];
    while centres.len() < CLUSTERS {
        let mut far = (0.0f32, None);
        for (_, lab) in &samples {
            let d = centres
                .iter()
                .map(|c| dist2(*c, *lab))
                .fold(f32::MAX, f32::min);
            if d > far.0 {
                far = (d, Some(*lab));
            }
        }
        match far {
            (d, Some(c)) if d > 1e-4 => centres.push(c),
            _ => break,
        }
    }
    let mut assign = vec![0usize; samples.len()];
    for _ in 0..ITERATIONS {
        for (slot, (_, lab)) in assign.iter_mut().zip(&samples) {
            *slot = (0..centres.len())
                .min_by(|&a, &b| dist2(centres[a], *lab).total_cmp(&dist2(centres[b], *lab)))
                .unwrap_or(0);
        }
        for (k, centre) in centres.iter_mut().enumerate() {
            let (mut sum, mut n) = ([0.0f32; 3], 0.0f32);
            for ((_, lab), _) in samples.iter().zip(&assign).filter(|(_, a)| **a == k) {
                (0..3).for_each(|i| sum[i] += lab[i]);
                n += 1.0;
            }
            if n > 0.0 {
                *centre = sum.map(|v| v / n);
            }
        }
    }
    let total = samples.len() as f32;
    let mut best: Option<(f32, [u8; 3])> = None;
    for (k, centre) in centres.iter().enumerate() {
        let (mut rgb, mut n) = ([0u32; 3], 0u32);
        for ((c, _), _) in samples.iter().zip(&assign).filter(|(_, a)| **a == k) {
            (0..3).for_each(|i| rgb[i] += u32::from(c[i]));
            n += 1;
        }
        if n == 0 {
            continue;
        }
        let chroma = centre[1].hypot(centre[2]);
        let mut score = n as f32 / total * (1.0 + 3.0 * chroma);
        if centre[0] > 0.92 || centre[0] < 0.15 {
            score *= 0.3;
        }
        let mean = rgb.map(|c| ((c + n / 2) / n) as u8);
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, mean));
        }
    }
    best.map(|(_, c)| c)
}

/// The card colour (RRGGBB) for a picture whose dominant colour is `average`: the
/// same hue, less saturated, and dark whatever the picture's brightness
/// (lightness 12% to 30%, so white text has a contrast of 7 or more and a
/// picture lighter than its card shows against it).
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
    let saturation = (saturation * 0.6).min(0.5);
    let lightness = 0.12 + 0.18 * lightness;
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
        ] {
            let card = card_color(average);
            assert!(
                contrast_with_white(card) >= 7.0,
                "{average:?} -> {card:06x}"
            );
        }
    }

    #[test]
    fn the_card_keeps_the_pictures_hue_but_not_all_its_colour() {
        let red = channels(card_color([230, 30, 30]));
        assert!(
            red[0] > red[1] + 15.0 && red[0] > red[2] + 15.0,
            "reddish: {red:?}"
        );
        let blue = channels(card_color([30, 40, 230]));
        assert!(
            blue[2] > blue[0] + 15.0 && blue[2] > blue[1] + 15.0,
            "bluish: {blue:?}"
        );
        // Less saturated than the picture's own colour.
        let spread = |c: [f64; 3]| {
            c.iter().cloned().fold(0.0, f64::max) - c.iter().cloned().fold(255.0, f64::min)
        };
        assert!(spread(red) < 230.0 - 30.0);
    }

    #[test]
    fn a_picture_brighter_than_its_card_stays_distinct() {
        // A light picture is drawn on a clearly darker card, so it shows.
        let card = card_color([240, 240, 240]);
        assert!(luminance(card) < 0.1, "{card:06x}");
        assert_ne!(card_color([240, 240, 240]), card_color([20, 20, 20]));
    }
}
