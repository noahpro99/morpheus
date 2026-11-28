//! A person's call card colour (#475): the desktop's rule (view-model `avatar_color`), so a card looks the same on both.

use morpheus_view_model::avatar_color::{card_color, dominant_rgba};

/// The card colour (`0xRRGGBB`) for a picture's RGBA pixels (row after row, four bytes a pixel; the desktop looks at 72 px): the
/// picture's dominant colour made into a card that white text reads on. `None` when there is nothing to take it from (no pixels,
/// or only transparent ones): the card then keeps the usual colour.
#[uniffi::export]
pub fn call_card_colour(rgba: Vec<u8>) -> Option<u32> {
    dominant_rgba(&rgba).map(card_color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_gives_a_card_that_white_text_reads_on_and_nothing_gives_none() {
        let pixels: Vec<u8> = (0..72 * 72).flat_map(|_| [200u8, 40, 40, 255]).collect();
        let card = call_card_colour(pixels).expect("a colour");
        let [r, g, b] = [(card >> 16) as u8, (card >> 8) as u8, card as u8];
        assert!(r > g && r > b, "the card keeps the picture's red hue: {card:06x}");
        assert!(call_card_colour(vec![]).is_none());
        assert!(call_card_colour(vec![0, 0, 0, 0, 9, 9, 9, 0]).is_none(), "transparent pixels give nothing");
    }
}

/// What the call heard of each remote voice (frames, loudest, whether the voice detector opened): a test diagnostic.
#[uniffi::export]
pub fn call_voice_stats() -> String {
    morpheus_call::voice_stats()
}
