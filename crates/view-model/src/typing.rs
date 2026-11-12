//! The line under the messages that says who is typing.

/// What the typing line says for the names of the people typing (never
/// yourself): nothing when nobody is.
pub fn line(names: &[String]) -> Option<String> {
    match names {
        [] => None,
        [one] => Some(format!("{one} is typing…")),
        [a, b] => Some(format!("{a} and {b} are typing…")),
        [a, b, ..] => Some(format!("{a}, {b} and others are typing…")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_line_names_up_to_two_people() {
        assert_eq!(line(&names(&[])), None);
        assert_eq!(line(&names(&["bob"])).as_deref(), Some("bob is typing…"));
        assert_eq!(
            line(&names(&["bob", "carol"])).as_deref(),
            Some("bob and carol are typing…")
        );
        assert_eq!(
            line(&names(&["bob", "carol", "dave"])).as_deref(),
            Some("bob, carol and others are typing…")
        );
    }
}
