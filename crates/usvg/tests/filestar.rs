// Copyright 2026 the Resvg Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Filestar's patches: the inputs 0.35.1 (filestar/resvg) read and upstream refuses,
//! and what the writer keeps for Filestar.Converters.Vector.

fn forgiving() -> usvg::Options<'static> {
    usvg::Options {
        forgiving: true,
        ..Default::default()
    }
}

fn written(svg: &[u8]) -> String {
    usvg::Tree::from_data(svg, &forgiving())
        .unwrap()
        .to_string(&usvg::WriteOptions::default())
}

const RECT: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'><rect width='5' height='5'/></svg>";

#[test]
fn utf16_input() {
    for bom_le in [true, false] {
        let mut data = Vec::new();
        if bom_le {
            data.extend([0xFF, 0xFE]);
            RECT.encode_utf16().for_each(|u| data.extend(u.to_le_bytes()));
        } else {
            data.extend([0xFE, 0xFF]);
            RECT.encode_utf16().for_each(|u| data.extend(u.to_be_bytes()));
        }
        // Not only with `forgiving`: an encoding is not an error in the drawing.
        let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).unwrap();
        assert!(tree.root().has_children());
    }
}

#[test]
fn utf8_bom_input() {
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend(RECT.as_bytes());
    assert!(usvg::Tree::from_data(&data, &usvg::Options::default()).is_ok());
}

#[test]
fn nul_characters_are_removed_when_forgiving() {
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>\0<rect width='5' height='5'/>\0</svg>";
    assert!(usvg::Tree::from_str(svg, &usvg::Options::default()).is_err());
    assert!(usvg::Tree::from_str(svg, &forgiving()).unwrap().root().has_children());
}

#[test]
fn unreadable_attribute_is_dropped_when_forgiving() {
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'><rect id='a&b' width='5' height='5'/></svg>";
    assert!(usvg::Tree::from_str(svg, &usvg::Options::default()).is_err());
    assert!(usvg::Tree::from_str(svg, &forgiving()).unwrap().root().has_children());
}

#[test]
fn invalid_size_is_1x1_when_forgiving() {
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='0' height='0'><rect width='5' height='5'/></svg>";
    assert!(usvg::Tree::from_str(svg, &usvg::Options::default()).is_err());
    let tree = usvg::Tree::from_str(svg, &forgiving()).unwrap();
    assert_eq!((tree.size().width(), tree.size().height()), (1.0, 1.0));
}

#[test]
fn more_than_a_million_elements() {
    let mut svg = String::from("<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>");
    svg.push_str(&"<g/>".repeat(1_000_100));
    svg.push_str("<rect width='5' height='5'/></svg>");
    assert!(usvg::Tree::from_str(&svg, &usvg::Options::default()).is_ok());
}

#[test]
fn titles_and_inkscape_labels_are_written() {
    let out = written(
        b"<svg xmlns='http://www.w3.org/2000/svg' xmlns:inkscape='http://www.inkscape.org/namespaces/inkscape' \
          xmlns:xlink='http://www.w3.org/1999/xlink' width='10' height='10'>\
            <g id='g1' inkscape:label='Layer &amp; 1'>\
              <rect width='5' height='5'><title>A  &lt;box&gt;</title></rect>\
            </g>\
            <image width='1' height='1' xlink:href='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='>\
              <title>Photo</title>\
            </image>\
          </svg>",
    );
    assert!(out.contains("<title>Layer &amp; 1</title>") || out.contains("Layer &amp; 1\n"), "{out}");
    assert!(out.contains("A &lt;box&gt;"), "{out}");
    assert!(out.contains("Photo"), "{out}");
    // Only the title's text is kept: no <title> element inside another.
    assert_eq!(out.matches("<title>").count(), 3, "{out}");
}

#[test]
fn vector_effect_is_written() {
    let out = written(
        b"<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>\
            <g vector-effect='non-scaling-stroke'><path d='M 1 1 L 9 9' stroke='black'/></g>\
            <path d='M 1 9 L 9 1' stroke='black'/>\
          </svg>",
    );
    assert_eq!(out.matches("vector-effect=\"non-scaling-stroke\"").count(), 1, "{out}");
}

#[test]
fn xlink_namespace_is_always_declared() {
    assert!(written(RECT.as_bytes()).contains("xmlns:xlink=\"http://www.w3.org/1999/xlink\""));
}
