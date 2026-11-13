//! The CSS selector subset the e2e tests use: tag, `.class`, `#id`, `[attr]`,
//! `[attr=value]`, `[attr="value"]`, `[attr^=value]`, `:not(compound)`,
//! descendant (space), child (`>`) and comma lists. Anything else is an
//! "unsupported selector" error.

use super::tree::Tree;

#[derive(Debug, PartialEq)]
enum AttrTest {
    Exists(String),
    Equals(String, String),
    StartsWith(String, String),
}

#[derive(Debug, Default, PartialEq)]
struct Compound {
    /// `None` for no tag or `*`.
    tag: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
    attrs: Vec<AttrTest>,
    /// `:not(...)`: none of these may match.
    nots: Vec<Compound>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Combinator {
    Descendant,
    Child,
}

/// `parts[0]`'s combinator is unused; later ones link to the part before.
#[derive(Debug, PartialEq)]
struct Complex {
    parts: Vec<(Combinator, Compound)>,
}

/// A parsed selector list.
#[derive(Debug, PartialEq)]
pub struct Selector {
    alternatives: Vec<Complex>,
}

impl Selector {
    pub fn parse(source: &str) -> Result<Self, String> {
        parse(source).map_err(|why| format!("unsupported selector: {source} ({why})"))
    }

    /// Indices of the matching nodes, in document order.
    pub fn select(&self, tree: &Tree) -> Vec<usize> {
        (0..tree.nodes.len())
            .filter(|&i| {
                self.alternatives
                    .iter()
                    .any(|complex| matches_from(tree, i, &complex.parts, complex.parts.len() - 1))
            })
            .collect()
    }
}

fn matches_compound(tree: &Tree, index: usize, compound: &Compound) -> bool {
    let node = &tree.nodes[index];
    if compound.tag.as_ref().is_some_and(|tag| *tag != node.tag) {
        return false;
    }
    if compound
        .classes
        .iter()
        .any(|class| !node.classes.contains(class))
    {
        return false;
    }
    if compound
        .id
        .as_ref()
        .is_some_and(|id| node.attr("id").as_ref() != Some(id))
    {
        return false;
    }
    compound.attrs.iter().all(|test| match test {
        AttrTest::Exists(name) => node.attr(name).is_some(),
        AttrTest::Equals(name, value) => node.attr(name).as_ref() == Some(value),
        AttrTest::StartsWith(name, value) => node
            .attr(name)
            .is_some_and(|actual| actual.starts_with(value.as_str())),
    }) && !compound
        .nots
        .iter()
        .any(|excluded| matches_compound(tree, index, excluded))
}

/// Whether `parts[..=k]` match with `parts[k]` on node `index`.
fn matches_from(tree: &Tree, index: usize, parts: &[(Combinator, Compound)], k: usize) -> bool {
    let (combinator, compound) = &parts[k];
    if !matches_compound(tree, index, compound) {
        return false;
    }
    if k == 0 {
        return true;
    }
    let parent = tree.nodes[index].parent;
    match combinator {
        Combinator::Child => parent.is_some_and(|p| matches_from(tree, p, parts, k - 1)),
        Combinator::Descendant => {
            let mut ancestor = parent;
            while let Some(a) = ancestor {
                if matches_from(tree, a, parts, k - 1) {
                    return true;
                }
                ancestor = tree.nodes[a].parent;
            }
            false
        }
    }
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

impl Parser<'_> {
    fn skip_space(&mut self) -> bool {
        let mut any = false;
        while self.chars.peek().is_some_and(|c| c.is_whitespace()) {
            self.chars.next();
            any = true;
        }
        any
    }

    fn ident(&mut self) -> Result<String, String> {
        let mut out = String::new();
        while let Some(&c) = self.chars.peek() {
            if is_ident(c) {
                out.push(c);
                self.chars.next();
            } else {
                break;
            }
        }
        if out.is_empty() {
            Err(match self.chars.peek() {
                Some(c) => format!("unexpected {c:?}"),
                None => "unexpected end".into(),
            })
        } else {
            Ok(out)
        }
    }

    fn attribute(&mut self) -> Result<AttrTest, String> {
        self.skip_space();
        let name = self.ident()?;
        self.skip_space();
        let prefix = self.chars.peek() == Some(&'^');
        if prefix {
            self.chars.next();
            if self.chars.peek() != Some(&'=') {
                return Err("^ without =".into());
            }
        }
        let value = match self.chars.next() {
            Some(']') => return Ok(AttrTest::Exists(name)),
            Some('=') => {
                self.skip_space();
                match self.chars.peek().copied() {
                    Some(quote @ ('"' | '\'')) => {
                        self.chars.next();
                        let mut value = String::new();
                        loop {
                            match self.chars.next() {
                                Some(c) if c == quote => break,
                                Some('\\') => return Err("escape in a value".into()),
                                Some(c) => value.push(c),
                                None => return Err("unterminated string".into()),
                            }
                        }
                        value
                    }
                    _ => {
                        let mut value = String::new();
                        while let Some(&c) = self.chars.peek() {
                            if c == ']' || c.is_whitespace() || matches!(c, '"' | '\'' | '\\') {
                                break;
                            }
                            value.push(c);
                            self.chars.next();
                        }
                        if value.is_empty() {
                            return Err("empty attribute value".into());
                        }
                        value
                    }
                }
            }
            Some(c) => return Err(format!("attribute operator at {c:?}")),
            None => return Err("unterminated [".into()),
        };
        self.skip_space();
        match self.chars.next() {
            Some(']') if prefix => Ok(AttrTest::StartsWith(name, value)),
            Some(']') => Ok(AttrTest::Equals(name, value)),
            Some(c) => Err(format!("unexpected {c:?} in attribute")),
            None => Err("unterminated [".into()),
        }
    }

    fn compound(&mut self) -> Result<Compound, String> {
        let mut compound = Compound::default();
        let mut any = false;
        match self.chars.peek().copied() {
            Some('*') => {
                self.chars.next();
                any = true;
            }
            Some(c) if is_ident(c) => {
                compound.tag = Some(self.ident()?.to_ascii_lowercase());
                any = true;
            }
            _ => {}
        }
        loop {
            match self.chars.peek().copied() {
                Some('.') => {
                    self.chars.next();
                    compound.classes.push(self.ident()?);
                }
                Some('#') => {
                    self.chars.next();
                    if compound.id.replace(self.ident()?).is_some() {
                        return Err("two ids".into());
                    }
                }
                Some('[') => {
                    self.chars.next();
                    compound.attrs.push(self.attribute()?);
                }
                Some(':') => {
                    self.chars.next();
                    let pseudo = self.ident()?;
                    if pseudo == "disabled" {
                        // A disabled control has a `disabled` attribute.
                        compound.attrs.push(AttrTest::Exists("disabled".into()));
                        any = true;
                        continue;
                    }
                    if pseudo == "checked" {
                        // A radio or checkbox that is on has a `checked` attribute.
                        compound.attrs.push(AttrTest::Exists("checked".into()));
                        any = true;
                        continue;
                    }
                    if pseudo != "not" || self.chars.next() != Some('(') {
                        return Err("only :not(...), :checked and :disabled are supported".into());
                    }
                    self.skip_space();
                    let excluded = self.compound()?;
                    self.skip_space();
                    if self.chars.next() != Some(')') {
                        return Err("unterminated :not(".into());
                    }
                    compound.nots.push(excluded);
                }
                _ => break,
            }
            any = true;
        }
        if any {
            Ok(compound)
        } else {
            Err(match self.chars.peek() {
                Some(c) => format!("unexpected {c:?}"),
                None => "unexpected end".into(),
            })
        }
    }

    fn complex(&mut self) -> Result<Complex, String> {
        self.skip_space();
        let mut parts = vec![(Combinator::Descendant, self.compound()?)];
        loop {
            let spaced = self.skip_space();
            match self.chars.peek().copied() {
                None | Some(',') => break,
                Some('>') => {
                    self.chars.next();
                    self.skip_space();
                    parts.push((Combinator::Child, self.compound()?));
                }
                Some(_) if spaced => parts.push((Combinator::Descendant, self.compound()?)),
                Some(c) => return Err(format!("unexpected {c:?}")),
            }
        }
        Ok(Complex { parts })
    }
}

fn parse(source: &str) -> Result<Selector, String> {
    let mut parser = Parser {
        chars: source.chars().peekable(),
    };
    let mut alternatives = vec![parser.complex()?];
    while parser.chars.next_if_eq(&',').is_some() {
        alternatives.push(parser.complex()?);
    }
    match parser.chars.peek() {
        None => Ok(Selector { alternatives }),
        Some(c) => Err(format!("unexpected {c:?}")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::e2e::tree::{Tree, fixture::node};

    /// body
    ///   div.app#root
    ///     form.login[name=signin]
    ///       input[name=username][placeholder="User name"]
    ///       input[name=password][type=password]
    ///       button.submit.primary[type=submit] "Sign in"
    ///     div.rooms
    ///       div.room.active[data-id="!a:b"]
    ///         span.room-name "general"
    ///       div.room[data-id="!c:d"]
    ///         span.room-name "random"
    fn sample() -> Tree {
        let mut t = Tree::default();
        let body = t.push(node("body", "", &[], ""), None);
        let app = t.push(node("div", "app", &[("id", "root")], ""), Some(body));
        let form = t.push(node("form", "login", &[("name", "signin")], ""), Some(app));
        t.push(
            node(
                "input",
                "",
                &[("name", "username"), ("placeholder", "User name")],
                "",
            ),
            Some(form),
        );
        t.push(
            node(
                "input",
                "",
                &[("name", "password"), ("type", "password")],
                "",
            ),
            Some(form),
        );
        t.push(
            node("button", "submit primary", &[("type", "submit")], "Sign in"),
            Some(form),
        );
        let rooms = t.push(node("div", "rooms", &[], ""), Some(app));
        let a = t.push(
            node("div", "room active", &[("data-id", "!a:b")], ""),
            Some(rooms),
        );
        t.push(node("span", "room-name", &[], "general"), Some(a));
        let c = t.push(node("div", "room", &[("data-id", "!c:d")], ""), Some(rooms));
        t.push(node("span", "room-name", &[], "random"), Some(c));
        t
    }

    fn count(selector: &str) -> usize {
        Selector::parse(selector)
            .unwrap_or_else(|e| panic!("{e}"))
            .select(&sample())
            .len()
    }

    fn names(selector: &str) -> Vec<String> {
        let tree = sample();
        Selector::parse(selector)
            .unwrap()
            .select(&tree)
            .into_iter()
            .map(|i| tree.text(i))
            .collect()
    }

    #[test]
    fn tag() {
        assert_eq!(count("input"), 2);
        assert_eq!(count("INPUT"), 2);
        assert_eq!(count("body"), 1);
        assert_eq!(count("table"), 0);
        assert_eq!(count("*"), 11);
    }

    #[test]
    fn classes() {
        assert_eq!(count(".room"), 2);
        assert_eq!(count(".room.active"), 1);
        assert_eq!(count(".active.room"), 1);
        assert_eq!(count("div.room"), 2);
        assert_eq!(count("span.room"), 0);
        assert_eq!(count(".submit.primary"), 1);
        assert_eq!(count(".ro"), 0);
        assert_eq!(count(".room-name"), 2);
    }

    #[test]
    fn ids() {
        assert_eq!(count("#root"), 1);
        assert_eq!(count("div#root.app"), 1);
        assert_eq!(count("#other"), 0);
        assert_eq!(count("span#root"), 0);
    }

    #[test]
    fn attribute_presence() {
        assert_eq!(count("[name]"), 3);
        assert_eq!(count("input[placeholder]"), 1);
        assert_eq!(count("[data-id]"), 2);
        assert_eq!(count("[nothing]"), 0);
        assert_eq!(count("[ name ]"), 3);
    }

    #[test]
    fn attribute_values() {
        assert_eq!(count("[name=username]"), 1);
        assert_eq!(count("[name=\"username\"]"), 1);
        assert_eq!(count("[name='username']"), 1);
        assert_eq!(count("form.login input[name=password]"), 1);
        assert_eq!(count("[placeholder=\"User name\"]"), 1);
        assert_eq!(count("[data-id=\"!a:b\"]"), 1);
        assert_eq!(count("[type=submit]"), 1);
        assert_eq!(count("[name=user]"), 0);
        assert_eq!(count("[name = username]"), 1);
        assert_eq!(count("[class=\"room active\"]"), 1);
        assert_eq!(count("[id=root]"), 1);
    }

    #[test]
    fn descendant() {
        assert_eq!(count(".app input"), 2);
        assert_eq!(count("body .room-name"), 2);
        assert_eq!(count(".rooms .room .room-name"), 2);
        assert_eq!(count(".login .room-name"), 0);
        assert_eq!(count("  .app   input  "), 2);
        assert_eq!(count("div div"), 3);
    }

    #[test]
    fn child() {
        assert_eq!(count(".rooms > .room"), 2);
        assert_eq!(count(".rooms>.room"), 2);
        assert_eq!(count(".app > .room"), 0);
        assert_eq!(count(".app > .rooms > .room > span"), 2);
        assert_eq!(count("form > input"), 2);
        assert_eq!(count("body > input"), 0);
        // Child and descendant mixed: backtracking over ancestors.
        assert_eq!(count("body .rooms > .room"), 2);
        assert_eq!(count(".app > .room-name"), 0);
        assert_eq!(count("div > div div"), 2);
    }

    #[test]
    fn not_and_prefix_tests() {
        assert_eq!(count(".room:not(.active)"), 1);
        assert_eq!(count(".room:not(.active) .room-name"), 1);
        assert_eq!(count("input:not([type=password])"), 1);
        assert_eq!(count("input:not([name=username]):not([type])"), 0);
        assert_eq!(count("input:not( .nothing )"), 2);
        assert_eq!(count("input:checked"), 0);
        assert_eq!(count("input:disabled"), count("input[disabled]"));
        assert_eq!(count("[name^=us]"), 1);
        assert_eq!(count("[data-id^='!']"), 2);
        assert_eq!(count("[data-id^=\"!a\"]"), 1);
        assert_eq!(count("[name^=x]"), 0);
        assert!(Selector::parse(".room:hover").is_err());
        assert!(Selector::parse(".room:not(.a").is_err());
        assert!(Selector::parse("[name^x]").is_err());
    }

    #[test]
    fn lists() {
        assert_eq!(count("input, button"), 3);
        assert_eq!(count("input,button"), 3);
        assert_eq!(count(".room, .room-name"), 4);
        assert_eq!(count("#root, .app"), 1, "a node matching twice counts once");
        assert_eq!(count(".submit , form"), 2);
        assert_eq!(count("input, .nothing"), 2);
    }

    #[test]
    fn document_order() {
        assert_eq!(
            names(".room-name, .submit"),
            ["Sign in", "general", "random"]
        );
        assert_eq!(names(".room"), ["general", "random"]);
    }

    #[test]
    fn unsupported_is_reported() {
        for selector in [
            "h1:first-child",
            ".room:hover",
            "a + b",
            "a ~ b",
            "[name$=me]",
            "[name*=ser]",
            "[name~=x]",
            "[name|=x]",
            "[name=x i]",
            "> input",
            "input >",
            "a >> b",
            "",
            ",",
            "input,",
            "#",
            ".",
            "[name",
            "[name=\"x]",
            "[]",
            "div::before",
            "a\\:b",
            "div#a#b",
            "(a)",
        ] {
            let err = Selector::parse(selector).expect_err(selector);
            assert!(
                err.starts_with(&format!("unsupported selector: {selector}")),
                "{err}"
            );
        }
    }

    #[test]
    fn parses_into_expected_shape() {
        let s = Selector::parse("form.login > input[name=\"u\"], #x").unwrap();
        assert_eq!(s.alternatives.len(), 2);
        let first = &s.alternatives[0];
        assert_eq!(first.parts.len(), 2);
        assert_eq!(first.parts[1].0, Combinator::Child);
        assert_eq!(first.parts[0].1.tag.as_deref(), Some("form"));
        assert_eq!(first.parts[0].1.classes, ["login"]);
        assert_eq!(
            first.parts[1].1.attrs,
            [AttrTest::Equals("name".into(), "u".into())]
        );
        assert_eq!(s.alternatives[1].parts[0].1.id.as_deref(), Some("x"));
    }
}
