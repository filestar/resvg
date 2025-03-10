// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::rc::Rc;

use kurbo::{ParamCurve, ParamCurveArclen};
use svgtypes::{Length, LengthUnit};
use usvg_tree::*;

use crate::svgtree::{AId, EId, FromValue, SvgNode};
use crate::{converter, style};

impl<'a, 'input: 'a> FromValue<'a, 'input> for usvg_tree::TextAnchor {
    fn parse(_: SvgNode, _: AId, value: &str) -> Option<Self> {
        match value {
            "start" => Some(usvg_tree::TextAnchor::Start),
            "middle" => Some(usvg_tree::TextAnchor::Middle),
            "end" => Some(usvg_tree::TextAnchor::End),
            _ => None,
        }
    }
}

impl<'a, 'input: 'a> FromValue<'a, 'input> for usvg_tree::AlignmentBaseline {
    fn parse(_: SvgNode, _: AId, value: &str) -> Option<Self> {
        match value {
            "auto" => Some(usvg_tree::AlignmentBaseline::Auto),
            "baseline" => Some(usvg_tree::AlignmentBaseline::Baseline),
            "before-edge" => Some(usvg_tree::AlignmentBaseline::BeforeEdge),
            "text-before-edge" => Some(usvg_tree::AlignmentBaseline::TextBeforeEdge),
            "middle" => Some(usvg_tree::AlignmentBaseline::Middle),
            "central" => Some(usvg_tree::AlignmentBaseline::Central),
            "after-edge" => Some(usvg_tree::AlignmentBaseline::AfterEdge),
            "text-after-edge" => Some(usvg_tree::AlignmentBaseline::TextAfterEdge),
            "ideographic" => Some(usvg_tree::AlignmentBaseline::Ideographic),
            "alphabetic" => Some(usvg_tree::AlignmentBaseline::Alphabetic),
            "hanging" => Some(usvg_tree::AlignmentBaseline::Hanging),
            "mathematical" => Some(usvg_tree::AlignmentBaseline::Mathematical),
            _ => None,
        }
    }
}

impl<'a, 'input: 'a> FromValue<'a, 'input> for usvg_tree::DominantBaseline {
    fn parse(_: SvgNode, _: AId, value: &str) -> Option<Self> {
        match value {
            "auto" => Some(usvg_tree::DominantBaseline::Auto),
            "use-script" => Some(usvg_tree::DominantBaseline::UseScript),
            "no-change" => Some(usvg_tree::DominantBaseline::NoChange),
            "reset-size" => Some(usvg_tree::DominantBaseline::ResetSize),
            "ideographic" => Some(usvg_tree::DominantBaseline::Ideographic),
            "alphabetic" => Some(usvg_tree::DominantBaseline::Alphabetic),
            "hanging" => Some(usvg_tree::DominantBaseline::Hanging),
            "mathematical" => Some(usvg_tree::DominantBaseline::Mathematical),
            "central" => Some(usvg_tree::DominantBaseline::Central),
            "middle" => Some(usvg_tree::DominantBaseline::Middle),
            "text-after-edge" => Some(usvg_tree::DominantBaseline::TextAfterEdge),
            "text-before-edge" => Some(usvg_tree::DominantBaseline::TextBeforeEdge),
            _ => None,
        }
    }
}

impl<'a, 'input: 'a> FromValue<'a, 'input> for usvg_tree::LengthAdjust {
    fn parse(_: SvgNode, _: AId, value: &str) -> Option<Self> {
        match value {
            "spacing" => Some(usvg_tree::LengthAdjust::Spacing),
            "spacingAndGlyphs" => Some(usvg_tree::LengthAdjust::SpacingAndGlyphs),
            _ => None,
        }
    }
}

impl<'a, 'input: 'a> FromValue<'a, 'input> for usvg_tree::FontStyle {
    fn parse(_: SvgNode, _: AId, value: &str) -> Option<Self> {
        match value {
            "normal" => Some(usvg_tree::FontStyle::Normal),
            "italic" => Some(usvg_tree::FontStyle::Italic),
            "oblique" => Some(usvg_tree::FontStyle::Oblique),
            _ => None,
        }
    }
}

pub(crate) fn convert(
    text_node: SvgNode,
    state: &converter::State,
    cache: &mut converter::Cache,
    parent: &mut Node,
) {
    let pos_list = resolve_positions_list(text_node, state);
    let rotate_list = resolve_rotate_list(text_node);
    let writing_mode = convert_writing_mode(text_node);

    let chunks = collect_text_chunks(text_node, &pos_list, state, cache);

    let rendering_mode: TextRendering = text_node
        .find_attribute(AId::TextRendering)
        .unwrap_or(state.opt.text_rendering);

    let title = text_node.title().map(ToOwned::to_owned);
    // Nodes generated by markers must not have an ID. Otherwise we would have duplicates.
    let id = if state.parent_markers.is_empty() {
        text_node.element_id().to_string()
    } else {
        String::new()
    };

    let text = Text {
        id,
        transform: Transform::default(),
        rendering_mode,
        positions: pos_list,
        rotate: rotate_list,
        writing_mode,
        chunks,
        title,
    };
    parent.append_kind(NodeKind::Text(text));
}

struct IterState {
    chars_count: usize,
    chunk_bytes_count: usize,
    split_chunk: bool,
    text_flow: TextFlow,
    chunks: Vec<TextChunk>,
}

fn collect_text_chunks(
    text_node: SvgNode,
    pos_list: &[CharacterPosition],
    state: &converter::State,
    cache: &mut converter::Cache,
) -> Vec<TextChunk> {
    let mut iter_state = IterState {
        chars_count: 0,
        chunk_bytes_count: 0,
        split_chunk: false,
        text_flow: TextFlow::Linear,
        chunks: Vec::new(),
    };

    collect_text_chunks_impl(
        text_node,
        text_node,
        pos_list,
        state,
        cache,
        &mut iter_state,
    );

    iter_state.chunks
}

fn collect_text_chunks_impl(
    text_node: SvgNode,
    parent: SvgNode,
    pos_list: &[CharacterPosition],
    state: &converter::State,
    cache: &mut converter::Cache,
    iter_state: &mut IterState,
) {
    for child in parent.children() {
        if child.is_element() {
            if child.tag_name() == Some(EId::TextPath) {
                if parent.tag_name() != Some(EId::Text) {
                    // `textPath` can be set only as a direct `text` element child.
                    iter_state.chars_count += count_chars(child);
                    continue;
                }

                match resolve_text_flow(child, state) {
                    Some(v) => {
                        iter_state.text_flow = v;
                    }
                    None => {
                        // Skip an invalid text path and all it's children.
                        // We have to update the chars count,
                        // because `pos_list` was calculated including this text path.
                        iter_state.chars_count += count_chars(child);
                        continue;
                    }
                }

                iter_state.split_chunk = true;
            }

            collect_text_chunks_impl(text_node, child, pos_list, state, cache, iter_state);

            iter_state.text_flow = TextFlow::Linear;

            // Next char after `textPath` should be split too.
            if child.tag_name() == Some(EId::TextPath) {
                iter_state.split_chunk = true;
            }

            continue;
        }

        if !parent.is_visible_element(state.opt) {
            iter_state.chars_count += child.text().chars().count();
            continue;
        }

        let anchor = parent.find_attribute(AId::TextAnchor).unwrap_or_default();

        // TODO: what to do when <= 0? UB?
        let font_size = crate::units::resolve_font_size(parent, state);
        let font_size = match NonZeroPositiveF32::new(font_size) {
            Some(n) => n,
            None => {
                // Skip this span.
                iter_state.chars_count += child.text().chars().count();
                continue;
            }
        };

        let font = convert_font(parent, state);

        let raw_paint_order: svgtypes::PaintOrder =
            parent.find_attribute(AId::PaintOrder).unwrap_or_default();
        let paint_order = crate::converter::svg_paint_order_to_usvg(raw_paint_order);

        let mut dominant_baseline = parent
            .find_attribute(AId::DominantBaseline)
            .unwrap_or_default();

        // `no-change` means "use parent".
        if dominant_baseline == DominantBaseline::NoChange {
            dominant_baseline = parent
                .parent_element()
                .unwrap()
                .find_attribute(AId::DominantBaseline)
                .unwrap_or_default();
        }

        let mut apply_kerning = true;
        #[allow(clippy::if_same_then_else)]
        if parent.resolve_length(AId::Kerning, state, -1.0) == 0.0 {
            apply_kerning = false;
        } else if parent.find_attribute::<&str>(AId::FontKerning) == Some("none") {
            apply_kerning = false;
        }

        let mut text_length =
            parent.try_convert_length(AId::TextLength, Units::UserSpaceOnUse, state);
        // Negative values should be ignored.
        if let Some(n) = text_length {
            if n < 0.0 {
                text_length = None;
            }
        }

        let title = child.title()
            .or_else(|| parent.title())
            .map(ToOwned::to_owned);
        let span = TextSpan {
            start: 0,
            end: 0,
            fill: style::resolve_fill(parent, true, state, cache),
            stroke: style::resolve_stroke(parent, true, state, cache),
            paint_order,
            font,
            font_size,
            small_caps: parent.find_attribute::<&str>(AId::FontVariant) == Some("small-caps"),
            apply_kerning,
            decoration: resolve_decoration(text_node, parent, state, cache),
            visibility: parent.find_attribute(AId::Visibility).unwrap_or_default(),
            dominant_baseline,
            alignment_baseline: parent
                .find_attribute(AId::AlignmentBaseline)
                .unwrap_or_default(),
            baseline_shift: convert_baseline_shift(parent, state),
            letter_spacing: parent.resolve_length(AId::LetterSpacing, state, 0.0),
            word_spacing: parent.resolve_length(AId::WordSpacing, state, 0.0),
            text_length,
            length_adjust: parent.find_attribute(AId::LengthAdjust).unwrap_or_default(),
            title,
        };

        let mut is_new_span = true;
        for c in child.text().chars() {
            let char_len = c.len_utf8();

            // Create a new chunk if:
            // - this is the first span (yes, position can be None)
            // - text character has an absolute coordinate assigned to it (via x/y attribute)
            // - `c` is the first char of the `textPath`
            // - `c` is the first char after `textPath`
            let is_new_chunk = pos_list[iter_state.chars_count].x.is_some()
                || pos_list[iter_state.chars_count].y.is_some()
                || iter_state.split_chunk
                || iter_state.chunks.is_empty();

            iter_state.split_chunk = false;

            if is_new_chunk {
                iter_state.chunk_bytes_count = 0;

                let mut span2 = span.clone();
                span2.start = 0;
                span2.end = char_len;

                iter_state.chunks.push(TextChunk {
                    x: pos_list[iter_state.chars_count].x,
                    y: pos_list[iter_state.chars_count].y,
                    anchor,
                    spans: vec![span2],
                    text_flow: iter_state.text_flow.clone(),
                    text: c.to_string(),
                });
            } else if is_new_span {
                // Add this span to the last text chunk.
                let mut span2 = span.clone();
                span2.start = iter_state.chunk_bytes_count;
                span2.end = iter_state.chunk_bytes_count + char_len;

                if let Some(chunk) = iter_state.chunks.last_mut() {
                    chunk.text.push(c);
                    chunk.spans.push(span2);
                }
            } else {
                // Extend the last span.
                if let Some(chunk) = iter_state.chunks.last_mut() {
                    chunk.text.push(c);
                    if let Some(span) = chunk.spans.last_mut() {
                        debug_assert_ne!(span.end, 0);
                        span.end += char_len;
                    }
                }
            }

            is_new_span = false;
            iter_state.chars_count += 1;
            iter_state.chunk_bytes_count += char_len;
        }
    }
}

fn resolve_text_flow(node: SvgNode, state: &converter::State) -> Option<TextFlow> {
    let linked_node = node.attribute::<SvgNode>(AId::Href)?;
    let path = crate::shapes::convert(linked_node, state)?;

    // The reference path's transform needs to be applied
    let path = if let Some(node_transform) = linked_node.attribute::<Transform>(AId::Transform) {
        let mut path_copy = path.as_ref().clone();
        path_copy = path_copy.transform(node_transform)?;
        Rc::new(path_copy)
    } else {
        path
    };

    let start_offset: Length = node.attribute(AId::StartOffset).unwrap_or_default();
    let start_offset = if start_offset.unit == LengthUnit::Percent {
        // 'If a percentage is given, then the `startOffset` represents
        // a percentage distance along the entire path.'
        let path_len = path_length(&path);
        (path_len * (start_offset.number / 100.0)) as f32
    } else {
        node.resolve_length(AId::StartOffset, state, 0.0)
    };

    Some(TextFlow::Path(Rc::new(TextPath { start_offset, path })))
}

fn convert_font(node: SvgNode, state: &converter::State) -> Font {
    let style: FontStyle = node.find_attribute(AId::FontStyle).unwrap_or_default();
    let stretch = conv_font_stretch(node);
    let weight = resolve_font_weight(node);

    let font_family = if let Some(n) = node.ancestors().find(|n| n.has_attribute(AId::FontFamily)) {
        n.attribute(AId::FontFamily).unwrap_or("")
    } else {
        ""
    };

    let mut families = Vec::new();
    for mut family in font_family.split(',') {
        // TODO: to a proper parser

        if family.starts_with(['\'', '"']) {
            family = &family[1..];
        }

        if family.ends_with(['\'', '"']) {
            family = &family[..family.len() - 1];
        }

        family = family.trim();

        if !family.is_empty() {
            families.push(family.to_string());
        }
    }

    if families.is_empty() {
        families.push(state.opt.font_family.clone())
    }

    Font {
        families,
        style,
        stretch,
        weight,
    }
}

// TODO: properly resolve narrower/wider
fn conv_font_stretch(node: SvgNode) -> FontStretch {
    if let Some(n) = node.ancestors().find(|n| n.has_attribute(AId::FontStretch)) {
        match n.attribute(AId::FontStretch).unwrap_or("") {
            "narrower" | "condensed" => FontStretch::Condensed,
            "ultra-condensed" => FontStretch::UltraCondensed,
            "extra-condensed" => FontStretch::ExtraCondensed,
            "semi-condensed" => FontStretch::SemiCondensed,
            "semi-expanded" => FontStretch::SemiExpanded,
            "wider" | "expanded" => FontStretch::Expanded,
            "extra-expanded" => FontStretch::ExtraExpanded,
            "ultra-expanded" => FontStretch::UltraExpanded,
            _ => FontStretch::Normal,
        }
    } else {
        FontStretch::Normal
    }
}

fn resolve_font_weight(node: SvgNode) -> u16 {
    fn bound(min: usize, val: usize, max: usize) -> usize {
        std::cmp::max(min, std::cmp::min(max, val))
    }

    let nodes: Vec<_> = node.ancestors().collect();
    let mut weight = 400;
    for n in nodes.iter().rev().skip(1) {
        // skip Root
        weight = match n.attribute(AId::FontWeight).unwrap_or("") {
            "normal" => 400,
            "bold" => 700,
            "100" => 100,
            "200" => 200,
            "300" => 300,
            "400" => 400,
            "500" => 500,
            "600" => 600,
            "700" => 700,
            "800" => 800,
            "900" => 900,
            "bolder" => {
                // By the CSS2 spec the default value should be 400
                // so `bolder` will result in 500.
                // But Chrome and Inkscape will give us 700.
                // Have no idea is it a bug or something, but
                // we will follow such behavior for now.
                let step = if weight == 400 { 300 } else { 100 };

                bound(100, weight + step, 900)
            }
            "lighter" => {
                // By the CSS2 spec the default value should be 400
                // so `lighter` will result in 300.
                // But Chrome and Inkscape will give us 200.
                // Have no idea is it a bug or something, but
                // we will follow such behavior for now.
                let step = if weight == 400 { 200 } else { 100 };

                bound(100, weight - step, 900)
            }
            _ => weight,
        };
    }

    weight as u16
}

/// Resolves text's character positions.
///
/// This includes: x, y, dx, dy.
///
/// # The character
///
/// The first problem with this task is that the *character* itself
/// is basically undefined in the SVG spec. Sometimes it's an *XML character*,
/// sometimes a *glyph*, and sometimes just a *character*.
///
/// There is an ongoing [discussion](https://github.com/w3c/svgwg/issues/537)
/// on the SVG working group that addresses this by stating that a character
/// is a Unicode code point. But it's not final.
///
/// Also, according to the SVG 2 spec, *character* is *a Unicode code point*.
///
/// Anyway, we treat a character as a Unicode code point.
///
/// # Algorithm
///
/// To resolve positions, we have to iterate over descendant nodes and
/// if the current node is a `tspan` and has x/y/dx/dy attribute,
/// than the positions from this attribute should be assigned to the characters
/// of this `tspan` and it's descendants.
///
/// Positions list can have more values than characters in the `tspan`,
/// so we have to clamp it, because values should not overlap, e.g.:
///
/// (we ignore whitespaces for example purposes,
/// so the `text` content is `Text` and not `T ex t`)
///
/// ```text
/// <text>
///   a
///   <tspan x="10 20 30">
///     bc
///   </tspan>
///   d
/// </text>
/// ```
///
/// In this example, the `d` position should not be set to `30`.
/// And the result should be: `[None, 10, 20, None]`
///
/// Another example:
///
/// ```text
/// <text>
///   <tspan x="100 110 120 130">
///     a
///     <tspan x="50">
///       bc
///     </tspan>
///   </tspan>
///   d
/// </text>
/// ```
///
/// The result should be: `[100, 50, 120, None]`
fn resolve_positions_list(text_node: SvgNode, state: &converter::State) -> Vec<CharacterPosition> {
    // Allocate a list that has all characters positions set to `None`.
    let total_chars = count_chars(text_node);
    let mut list = vec![
        CharacterPosition {
            x: None,
            y: None,
            dx: None,
            dy: None,
        };
        total_chars
    ];

    let mut offset = 0;
    for child in text_node.descendants() {
        if child.is_element() {
            // We must ignore text positions on `textPath`.
            if !matches!(child.tag_name(), Some(EId::Text) | Some(EId::Tspan)) {
                continue;
            }

            let child_chars = count_chars(child);
            macro_rules! push_list {
                ($aid:expr, $field:ident) => {
                    if let Some(num_list) = crate::units::convert_list(child, $aid, state) {
                        // Note that we are using not the total count,
                        // but the amount of characters in the current `tspan` (with children).
                        let len = std::cmp::min(num_list.len(), child_chars);
                        for i in 0..len {
                            list[offset + i].$field = Some(num_list[i]);
                        }
                    }
                };
            }

            push_list!(AId::X, x);
            push_list!(AId::Y, y);
            push_list!(AId::Dx, dx);
            push_list!(AId::Dy, dy);
        } else if child.is_text() {
            // Advance the offset.
            offset += child.text().chars().count();
        }
    }

    list
}

/// Resolves characters rotation.
///
/// The algorithm is well explained
/// [in the SVG spec](https://www.w3.org/TR/SVG11/text.html#TSpanElement) (scroll down a bit).
///
/// ![](https://www.w3.org/TR/SVG11/images/text/tspan05-diagram.png)
///
/// Note: this algorithm differs from the position resolving one.
fn resolve_rotate_list(text_node: SvgNode) -> Vec<f32> {
    // Allocate a list that has all characters angles set to `0.0`.
    let mut list = vec![0.0; count_chars(text_node)];
    let mut last = 0.0;
    let mut offset = 0;
    for child in text_node.descendants() {
        if child.is_element() {
            if let Some(rotate) = child.attribute::<Vec<f32>>(AId::Rotate) {
                for i in 0..count_chars(child) {
                    if let Some(a) = rotate.get(i).cloned() {
                        list[offset + i] = a;
                        last = a;
                    } else {
                        // If the rotate list doesn't specify the rotation for
                        // this character - use the last one.
                        list[offset + i] = last;
                    }
                }
            }
        } else if child.is_text() {
            // Advance the offset.
            offset += child.text().chars().count();
        }
    }

    list
}

/// Resolves node's `text-decoration` property.
///
/// `text` and `tspan` can point to the same node.
fn resolve_decoration(
    text_node: SvgNode,
    tspan: SvgNode,
    state: &converter::State,
    cache: &mut converter::Cache,
) -> TextDecoration {
    // TODO: explain the algorithm

    let text_dec = conv_text_decoration(text_node);
    let tspan_dec = conv_text_decoration2(tspan);

    let mut gen_style = |in_tspan: bool, in_text: bool| {
        let n = if in_tspan {
            tspan
        } else if in_text {
            text_node
        } else {
            return None;
        };

        Some(TextDecorationStyle {
            fill: style::resolve_fill(n, true, state, cache),
            stroke: style::resolve_stroke(n, true, state, cache),
        })
    };

    TextDecoration {
        underline: gen_style(tspan_dec.has_underline, text_dec.has_underline),
        overline: gen_style(tspan_dec.has_overline, text_dec.has_overline),
        line_through: gen_style(tspan_dec.has_line_through, text_dec.has_line_through),
    }
}

struct TextDecorationTypes {
    has_underline: bool,
    has_overline: bool,
    has_line_through: bool,
}

/// Resolves the `text` node's `text-decoration` property.
fn conv_text_decoration(text_node: SvgNode) -> TextDecorationTypes {
    fn find_decoration(node: SvgNode, value: &str) -> bool {
        node.ancestors().any(|n| {
            if let Some(str_value) = n.attribute::<&str>(AId::TextDecoration) {
                str_value.split(' ').any(|v| v == value)
            } else {
                false
            }
        })
    }

    TextDecorationTypes {
        has_underline: find_decoration(text_node, "underline"),
        has_overline: find_decoration(text_node, "overline"),
        has_line_through: find_decoration(text_node, "line-through"),
    }
}

/// Resolves the default `text-decoration` property.
fn conv_text_decoration2(tspan: SvgNode) -> TextDecorationTypes {
    let s = tspan.attribute(AId::TextDecoration);
    TextDecorationTypes {
        has_underline: s == Some("underline"),
        has_overline: s == Some("overline"),
        has_line_through: s == Some("line-through"),
    }
}

fn convert_baseline_shift(node: SvgNode, state: &converter::State) -> Vec<BaselineShift> {
    let mut shift = Vec::new();
    let nodes: Vec<_> = node
        .ancestors()
        .take_while(|n| n.tag_name() != Some(EId::Text))
        .collect();
    for n in nodes {
        if let Some(len) = n.attribute::<Length>(AId::BaselineShift) {
            if len.unit == LengthUnit::Percent {
                let n = crate::units::resolve_font_size(n, state) * (len.number as f32 / 100.0);
                shift.push(BaselineShift::Number(n));
            } else {
                let n = crate::units::convert_length(
                    len,
                    n,
                    AId::BaselineShift,
                    Units::ObjectBoundingBox,
                    state,
                );
                shift.push(BaselineShift::Number(n));
            }
        } else if let Some(s) = n.attribute(AId::BaselineShift) {
            match s {
                "sub" => shift.push(BaselineShift::Subscript),
                "super" => shift.push(BaselineShift::Superscript),
                _ => shift.push(BaselineShift::Baseline),
            }
        }
    }

    if shift
        .iter()
        .all(|base| matches!(base, BaselineShift::Baseline))
    {
        shift.clear();
    }

    shift
}

fn count_chars(node: SvgNode) -> usize {
    node.descendants()
        .filter(|n| n.is_text())
        .fold(0, |w, n| w + n.text().chars().count())
}

/// Converts the writing mode.
///
/// [SVG 2] references [CSS Writing Modes Level 3] for the definition of the
/// 'writing-mode' property, there are only two writing modes:
/// horizontal left-to-right and vertical right-to-left.
///
/// That specification introduces new values for the property. The SVG 1.1
/// values are obsolete but must still be supported by converting the specified
/// values to computed values as follows:
///
/// - `lr`, `lr-tb`, `rl`, `rl-tb` => `horizontal-tb`
/// - `tb`, `tb-rl` => `vertical-rl`
///
/// The current `vertical-lr` behaves exactly the same as `vertical-rl`.
///
/// Also, looks like no one really supports the `rl` and `rl-tb`, except `Batik`.
/// And I'm not sure if its behaviour is correct.
///
/// So we will ignore it as well, mainly because I have no idea how exactly
/// it should affect the rendering.
///
/// [SVG 2]: https://www.w3.org/TR/SVG2/text.html#WritingModeProperty
/// [CSS Writing Modes Level 3]: https://www.w3.org/TR/css-writing-modes-3/#svg-writing-mode-css
fn convert_writing_mode(text_node: SvgNode) -> WritingMode {
    if let Some(n) = text_node
        .ancestors()
        .find(|n| n.has_attribute(AId::WritingMode))
    {
        match n.attribute(AId::WritingMode).unwrap_or("lr-tb") {
            "tb" | "tb-rl" | "vertical-rl" | "vertical-lr" => WritingMode::TopToBottom,
            _ => WritingMode::LeftToRight,
        }
    } else {
        WritingMode::LeftToRight
    }
}

fn path_length(path: &tiny_skia_path::Path) -> f64 {
    let mut prev_mx = path.points()[0].x;
    let mut prev_my = path.points()[0].y;
    let mut prev_x = prev_mx;
    let mut prev_y = prev_my;

    fn create_curve_from_line(px: f32, py: f32, x: f32, y: f32) -> kurbo::CubicBez {
        let line = kurbo::Line::new(
            kurbo::Point::new(px as f64, py as f64),
            kurbo::Point::new(x as f64, y as f64),
        );
        let p1 = line.eval(0.33);
        let p2 = line.eval(0.66);
        kurbo::CubicBez::new(line.p0, p1, p2, line.p1)
    }

    let mut length = 0.0;
    for seg in path.segments() {
        let curve = match seg {
            tiny_skia_path::PathSegment::MoveTo(p) => {
                prev_mx = p.x;
                prev_my = p.y;
                prev_x = p.x;
                prev_y = p.y;
                continue;
            }
            tiny_skia_path::PathSegment::LineTo(p) => {
                create_curve_from_line(prev_x, prev_y, p.x, p.y)
            }
            tiny_skia_path::PathSegment::QuadTo(p1, p) => kurbo::QuadBez::new(
                kurbo::Point::new(prev_x as f64, prev_y as f64),
                kurbo::Point::new(p1.x as f64, p1.y as f64),
                kurbo::Point::new(p.x as f64, p.y as f64),
            )
            .raise(),
            tiny_skia_path::PathSegment::CubicTo(p1, p2, p) => kurbo::CubicBez::new(
                kurbo::Point::new(prev_x as f64, prev_y as f64),
                kurbo::Point::new(p1.x as f64, p1.y as f64),
                kurbo::Point::new(p2.x as f64, p2.y as f64),
                kurbo::Point::new(p.x as f64, p.y as f64),
            ),
            tiny_skia_path::PathSegment::Close => {
                create_curve_from_line(prev_x, prev_y, prev_mx, prev_my)
            }
        };

        length += curve.arclen(0.5);
        prev_x = curve.p3.x as f32;
        prev_y = curve.p3.y as f32;
    }

    length
}
