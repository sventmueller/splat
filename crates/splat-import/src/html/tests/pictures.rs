//! Pictures by their address or with their data.

use super::*;

fn images_of(html: &str) -> Vec<(Source, String)> {
    let rich = from_html(html);
    let mut found = Vec::new();
    let Some(Block::Paragraph(spans)) = rich.blocks.first() else {
        return found;
    };
    for span in spans {
        if let Piece::Image(image) = &span.piece {
            found.push((image.source.clone(), image.alt.clone()));
        }
    }
    found
}

#[test]
fn pictures_come_with_their_address_or_their_data() {
    assert_eq!(
        images_of("<img src=\"https://a.bc/d.png\" alt=\"x\">"),
        [(Source::Address("https://a.bc/d.png".to_string()), "x".to_string())]
    );
    assert_eq!(
        images_of("<img src=\" cid:abc \">"),
        [(Source::Address("cid:abc".to_string()), String::new())]
    );
    // "GIF89a" in base64.
    assert_eq!(
        images_of("<img src=\"data:image/gif;base64,R0lGODlh\" alt=\"x\">"),
        [(
            Source::Data {
                mime: "image/gif",
                bytes: b"GIF89a".to_vec()
            },
            "x".to_string()
        )]
    );
}

#[test]
fn a_picture_in_its_address_reaches_whoever_places_it_decoded() {
    let mut rich = from_html("a <img src=\"data:image/gif;base64,R0lGODlh\" alt=\"x\"> b");
    let mut seen = Vec::new();
    rich.place_images(&mut |source| {
        if let crate::ImageSource::Data { mime, bytes } = source {
            seen.push(((*mime).to_string(), bytes.to_vec()));
        }
        Some("images/p.gif".to_string())
    });
    assert_eq!(seen, [("image/gif".to_string(), b"GIF89a".to_vec())]);
    assert_eq!(rich.into_markdown(), "a ![x](images/p.gif) b\n");
}

#[test]
fn pictures_that_cannot_be_had_leave_their_description() {
    same(&[
        ("<img src=\"file:///a.png\" alt=\"x\">", "x\n"),
        ("<img src=\"data:image/png;base64,AAAA\" alt=\"x\">", "x\n"),
        ("<img src=\"data:text/html,x\" alt=\"x\">", "x\n"),
        ("a<img alt=\"x\">b", "axb\n"),
        ("a<img src=\"file:///a.png\">b", "ab\n"),
        ("<img src=\"javascript:x\" alt=\"x\">", "x\n"),
        ("<img src=\"a.png\" alt=\"x\">", "![x](a.png)\n"),
        ("a <img src=\"a b.png\" alt=\"x\"> c", "a ![x](<a b.png>) c\n"),
    ]);
}
