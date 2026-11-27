//! What a picture or clip in a chat is, from its name and length, for every client (#412).

/// Whether a picture's file name says it is an animated GIF.
pub fn is_gif(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("gif"))
}

/// A clip's length as a player shows it: `0:07`, `12:03`, `1:02:03`.
pub fn duration_label(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gifs_are_known_by_their_extension() {
        assert!(is_gif("party.GIF") && is_gif("a.b.gif"));
        assert!(!is_gif("photo.png") && !is_gif("gif") && !is_gif("notgif"));
    }

    #[test]
    fn a_length_reads_like_a_player() {
        assert_eq!(duration_label(7_400), "0:07");
        assert_eq!(duration_label(723_000), "12:03");
        assert_eq!(duration_label(3_723_000), "1:02:03");
    }
}
