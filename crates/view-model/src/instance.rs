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

/// The first Matrix link among a program's arguments.
pub fn link_in_args(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter()
        .find(|arg| morpheus_core::parse_matrix_link(arg).is_some())
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
    }
}
