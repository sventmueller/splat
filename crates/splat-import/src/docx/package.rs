//! How a Word file names what it holds: the namespaces of its vocabularies and how elements and attributes in them are
//! found.

use roxmltree::Node;

pub(super) const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(super) const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(super) const M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
pub(super) const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
pub(super) const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub(super) const V: &str = "urn:schemas-microsoft-com:vml";
pub(super) const OFFICE: &str = "urn:schemas-microsoft-com:office:office";
pub(super) const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// Whether `node` is the element `name` of `namespace`, whatever prefix the file uses for it.
pub(super) fn named(node: Node, namespace: &str, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace() == Some(namespace)
}

pub(super) fn is_w(node: Node, name: &str) -> bool {
    named(node, W, name)
}

pub(super) fn w_attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attribute((W, name))
}

pub(super) fn w_val<'a>(node: Node<'a, '_>) -> Option<&'a str> {
    w_attr(node, "val")
}

pub(super) fn child_w<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|child| is_w(*child, name))
}

/// The value of a property that a bare element switches on.
pub(super) fn is_on(node: Node) -> bool {
    !matches!(w_val(node), Some("0" | "false" | "off"))
}

/// What a reader takes of `mc:AlternateContent`: its first choice, if there is anything in it, or the fallback.
pub(super) fn chosen_content<'a, 'i>(alternate: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    let choice = alternate
        .children()
        .find(|child| named(*child, MC, "Choice"))
        .filter(|choice| choice.children().any(|child| child.is_element()));
    choice.or_else(|| alternate.children().find(|child| named(*child, MC, "Fallback")))
}
