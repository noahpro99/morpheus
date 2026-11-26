//! What a later launch of Morpheus asks the running one for, over the
//! single-instance socket: to show its window, or to open a Matrix link.

/// What a later launch asked the running app for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Show,
    Open(String),
}

impl Request {
    /// The line sent over the socket.
    pub fn line(&self) -> String {
        match self {
            Request::Show => "show\n".to_owned(),
            Request::Open(link) => format!("open {link}\n"),
        }
    }

    pub fn parse(line: &str) -> Request {
        match line.trim().strip_prefix("open ") {
            Some(link) => Request::Open(link.to_owned()),
            None => Request::Show,
        }
    }
}

/// The first Matrix link among a program's arguments: given as one, or as a
/// Windows toast's click (`morpheus-notify:`), which carries one.
pub fn link_in_args(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter().find_map(|arg| {
        crate::win_toast::target_of(&arg)
            .or_else(|| morpheus_core::parse_matrix_link(&arg).is_some().then_some(arg))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_over_a_line() {
        assert_eq!(Request::parse(&Request::Show.line()), Request::Show);
        let open = Request::Open("matrix:u/bob:localhost".into());
        assert_eq!(Request::parse(&open.line()), open);
        assert_eq!(Request::parse(""), Request::Show);
    }

    #[test]
    fn the_matrix_link_among_arguments() {
        let args = ["--hidden", "matrix:u/bob:localhost"].map(String::from);
        assert_eq!(link_in_args(args).as_deref(), Some("matrix:u/bob:localhost"));
        assert_eq!(link_in_args(["--hidden".to_owned()]), None);
        let click = crate::win_toast::launch_arg("!r:x", Some("$e"));
        assert_eq!(link_in_args([click]).as_deref(), Some("matrix:roomid/r:x/e/e"));
    }
}
