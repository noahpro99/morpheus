//! The colour of a call card for someone without video: the average colour
//! of their profile picture, darkened and desaturated so white text and the
//! picture itself stay readable on it.

/// The average colour of RGBA pixels (4 bytes each), weighted by their
/// alpha; `None` for no pixels or only transparent ones.
pub fn average_rgba(pixels: &[u8]) -> Option<[u8; 3]> {
    let mut sum = [0u64; 3];
    let mut weight = 0u64;
    for pixel in pixels.chunks_exact(4) {
        let alpha = u64::from(pixel[3]);
        for (total, channel) in sum.iter_mut().zip(pixel) {
            *total += u64::from(*channel) * alpha;
        }
        weight += alpha;
    }
    if weight == 0 {
        return None;
    }
    Some(sum.map(|total| ((total + weight / 2) / weight) as u8))
}

/// The card colour (RRGGBB) for a picture whose average is `average`: the
/// same hue, less saturated, and dark whatever the picture's brightness
/// (lightness 12% to 30%, so white text has a contrast of 7 or more and a
/// picture lighter than its card shows against it).
pub fn card_color(average: [u8; 3]) -> u32 {
    let [r, g, b] = average.map(|c| f64::from(c) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    let saturation = if delta == 0.0 { 0.0 } else { delta / (1.0 - (2.0 * lightness - 1.0).abs()) };
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
            if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        });
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn contrast_with_white(rgb: u32) -> f64 {
        1.05 / (luminance(rgb) + 0.05)
    }

    #[test]
    fn the_average_is_the_mean_of_the_pixels() {
        let pixels = [255, 0, 0, 255, 0, 0, 255, 255];
        assert_eq!(average_rgba(&pixels), Some([128, 0, 128]));
        assert_eq!(average_rgba(&[10, 20, 30, 255]), Some([10, 20, 30]));
    }

    #[test]
    fn transparent_pixels_do_not_count() {
        // A red pixel and a (hidden) white one with no alpha.
        assert_eq!(average_rgba(&[200, 0, 0, 255, 255, 255, 255, 0]), Some([200, 0, 0]));
        assert_eq!(average_rgba(&[255, 255, 255, 0]), None);
        assert_eq!(average_rgba(&[]), None);
    }

    #[test]
    fn white_text_reads_on_any_card_colour() {
        for average in [[255, 255, 255], [255, 255, 0], [0, 255, 255], [255, 0, 0], [0, 0, 0], [0, 255, 0]] {
            let card = card_color(average);
            assert!(contrast_with_white(card) >= 7.0, "{average:?} -> {card:06x}");
        }
    }

    #[test]
    fn the_card_keeps_the_pictures_hue_but_not_all_its_colour() {
        let red = channels(card_color([230, 30, 30]));
        assert!(red[0] > red[1] + 15.0 && red[0] > red[2] + 15.0, "reddish: {red:?}");
        let blue = channels(card_color([30, 40, 230]));
        assert!(blue[2] > blue[0] + 15.0 && blue[2] > blue[1] + 15.0, "bluish: {blue:?}");
        // Less saturated than the picture's own colour.
        let spread = |c: [f64; 3]| c.iter().cloned().fold(0.0, f64::max) - c.iter().cloned().fold(255.0, f64::min);
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
