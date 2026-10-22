//! Read-only accessors over an `rcdom` node.
//!
//! `El` borrows the element's attribute vector, so it is deliberately
//! short-lived: callers scope it to the decision they are making and drop it
//! before recursing into children.

use std::cell::Ref;
use std::str::SplitAsciiWhitespace;

use html5ever::Attribute;
use markup5ever_rcdom::{Handle, NodeData};

/// An element node plus its borrowed attributes.
pub struct El<'a> {
    pub node: &'a Handle,
    /// Lowercase local name. html5ever's tree builder lowercases HTML tag names,
    /// so `<FONT>`, `<DIV>` and `<BR>` all arrive here as `font`, `div`, `br`.
    pub tag: &'a str,
    attrs: Ref<'a, Vec<Attribute>>,
}

impl<'a> El<'a> {
    /// Borrow `h` as an element, or `None` for text/comment/document nodes.
    pub fn new(h: &'a Handle) -> Option<El<'a>> {
        match &h.data {
            NodeData::Element { name, attrs, .. } => Some(El {
                node: h,
                tag: &name.local,
                attrs: attrs.borrow(),
            }),
            _ => None,
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| &*a.name.local == name)
            .map(|a| &*a.value)
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.classes().any(|c| c == class)
    }

    /// Any of `wanted` present in the element's class list.
    pub fn has_any_class(&self, wanted: &[&str]) -> bool {
        self.classes().any(|c| wanted.contains(&c))
    }

    pub fn classes(&self) -> SplitAsciiWhitespace<'_> {
        self.attr("class").unwrap_or("").split_ascii_whitespace()
    }

    pub fn has_child_element(&self, tag: &str) -> bool {
        self.node.children.borrow().iter().any(|c| match &c.data {
            NodeData::Element { name, .. } => &*name.local == tag,
            _ => false,
        })
    }
}
