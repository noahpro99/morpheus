//! A small emoji table with Discord-style shortcodes, for the composer's
//! `:code` autocomplete and the emoji panel.

/// `(emoji, shortcode)`, roughly in Discord's "frequently used" order.
pub const EMOJI: &[(&str, &str)] = &[
    ("👍", "thumbsup"),
    ("👎", "thumbsdown"),
    ("❤️", "heart"),
    ("😂", "joy"),
    ("😀", "grinning"),
    ("😄", "smile"),
    ("😆", "laughing"),
    ("😅", "sweat_smile"),
    ("🤣", "rofl"),
    ("😊", "blush"),
    ("😉", "wink"),
    ("😍", "heart_eyes"),
    ("😘", "kissing_heart"),
    ("😎", "sunglasses"),
    ("🤔", "thinking"),
    ("😐", "neutral_face"),
    ("🙄", "rolling_eyes"),
    ("😏", "smirk"),
    ("😮", "open_mouth"),
    ("😢", "cry"),
    ("😭", "sob"),
    ("😡", "rage"),
    ("😱", "scream"),
    ("🥳", "partying_face"),
    ("🤯", "exploding_head"),
    ("🥺", "pleading_face"),
    ("🫡", "saluting_face"),
    ("💀", "skull"),
    ("👀", "eyes"),
    ("🙏", "pray"),
    ("👏", "clap"),
    ("🤝", "handshake"),
    ("👋", "wave"),
    ("✌️", "v"),
    ("🤞", "crossed_fingers"),
    ("💪", "muscle"),
    ("🎉", "tada"),
    ("🔥", "fire"),
    ("💯", "100"),
    ("✅", "white_check_mark"),
    ("❌", "x"),
    ("⭐", "star"),
    ("✨", "sparkles"),
    ("💡", "bulb"),
    ("🚀", "rocket"),
    ("🐛", "bug"),
    ("🎯", "dart"),
    ("🏆", "trophy"),
    ("🎮", "video_game"),
    ("🍕", "pizza"),
    ("☕", "coffee"),
    ("🍺", "beer"),
    ("🎂", "birthday"),
    ("🌈", "rainbow"),
    ("☀️", "sunny"),
    ("🌙", "crescent_moon"),
    ("🐱", "cat"),
    ("🐶", "dog"),
    ("🦀", "crab"),
    ("💻", "computer"),
    ("📎", "paperclip"),
    ("📌", "pushpin"),
    ("🔒", "lock"),
    ("⚠️", "warning"),
    ("❓", "question"),
    ("💬", "speech_balloon"),
    ("👉", "point_right"),
    ("🫶", "heart_hands"),
];

/// Emoji whose shortcode starts with (or contains) `query`, best first.
pub fn search(query: &str) -> Vec<(&'static str, &'static str)> {
    let query = query.to_ascii_lowercase();
    let mut starts: Vec<_> = EMOJI
        .iter()
        .copied()
        .filter(|(_, code)| code.starts_with(&query))
        .collect();
    let contains = EMOJI
        .iter()
        .copied()
        .filter(|(_, code)| !code.starts_with(&query) && code.contains(&query));
    starts.extend(contains);
    starts
}
