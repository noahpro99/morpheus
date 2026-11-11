//! The tagged-element tree of one frame: plain data, no GPUI types, so the
//! selector matcher can be tested without a window.

/// A rectangle in window (logical) pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// One tagged element.
#[derive(Clone, Debug, Default)]
pub struct Node {
    /// Tag name, lower case (`div`, `button`, `input`, …).
    pub tag: String,
    pub classes: Vec<String>,
    /// Attributes other than `class` (`id`, `name`, `title`, `data-*`, `value`, …).
    pub attrs: Vec<(String, String)>,
    /// Text this element draws itself (not its children's).
    pub text: String,
    pub bounds: Rect,
    /// Index of the parent in [`Tree::nodes`].
    pub parent: Option<usize>,
    /// Distance from the root; with pre-order, a subtree is contiguous.
    pub depth: usize,
}

impl Node {
    /// An attribute's value; `class` is the class list joined by spaces.
    pub fn attr(&self, name: &str) -> Option<String> {
        if name == "class" {
            return (!self.classes.is_empty()).then(|| self.classes.join(" "));
        }
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }
}

/// Nodes in document (pre-)order: a parent before its children.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
}

impl Tree {
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /// Add a node under `parent`; returns its index.
    pub fn push(&mut self, mut node: Node, parent: Option<usize>) -> usize {
        node.parent = parent;
        node.depth = parent.map_or(0, |p| self.nodes[p].depth + 1);
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// `innerText`-like: the element's own text, then its descendants', in
    /// document order, concatenated (as `textContent` does).
    pub fn text(&self, index: usize) -> String {
        let depth = self.nodes[index].depth;
        let mut out = self.nodes[index].text.clone();
        for node in self.nodes[index + 1..]
            .iter()
            .take_while(|node| node.depth > depth)
        {
            out.push_str(&node.text);
        }
        out
    }
}

#[cfg(test)]
pub mod fixture {
    use super::*;

    /// A node for tests: `div.a.b` style shorthand.
    pub fn node(tag: &str, classes: &str, attrs: &[(&str, &str)], text: &str) -> Node {
        Node {
            tag: tag.into(),
            classes: classes.split_whitespace().map(Into::into).collect(),
            attrs: attrs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            text: text.into(),
            ..Node::default()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{fixture::node, *};

    #[test]
    fn text_joins_own_and_descendants_in_order() {
        let mut tree = Tree::default();
        let root = tree.push(node("div", "", &[], "a"), None);
        let child = tree.push(node("span", "", &[], "b"), Some(root));
        tree.push(node("i", "", &[], "c"), Some(child));
        let sibling = tree.push(node("div", "", &[], "d"), Some(root));
        assert_eq!(tree.text(root), "abcd");
        assert_eq!(tree.text(child), "bc");
        assert_eq!(tree.text(sibling), "d");
    }

    #[test]
    fn class_attribute_is_the_joined_list() {
        let n = node("div", "a b", &[("id", "x")], "");
        assert_eq!(n.attr("class").as_deref(), Some("a b"));
        assert_eq!(n.attr("id").as_deref(), Some("x"));
        assert_eq!(n.attr("nope"), None);
    }
}
