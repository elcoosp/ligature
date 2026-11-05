//! S3 spike proof: the exact const-fn syntax the emitter produces compiles
//! and const-evaluates (exit criterion "const-compiles + snapshot").
//!
//! The hand-written fns use the emitter's output form; the snapshot test
//! pins the emitter output so any drift breaks the build here first.

use taffy::Style;

/// `display:flex;width:100px;align-items:center` (emitter output order)
pub const fn counter_style() -> Style {
    let mut s = Style::DEFAULT;
    s.display = taffy::style::Display::Flex;
    s.size.width = taffy::Dimension::length(100.0);
    s.align_items = taffy::style::AlignItems::CENTER;
    s
}

/// `padding:16px`
pub const fn padding_style() -> Style {
    let mut s = Style::DEFAULT;
    s.padding = taffy::Rect {
        top: taffy::LengthPercentage::length(16.0),
        right: taffy::LengthPercentage::length(16.0),
        bottom: taffy::LengthPercentage::length(16.0),
        left: taffy::LengthPercentage::length(16.0),
    };
    s
}

/// `gap:8px;flex-direction:column`
pub const fn list_style() -> Style {
    let mut s = Style::DEFAULT;
    s.gap = taffy::Size {
        width: taffy::LengthPercentage::length(8.0),
        height: taffy::LengthPercentage::length(0.0),
    };
    s.flex_direction = taffy::style::FlexDirection::Column;
    s
}

#[test]
fn profile_v1_consts_compile_and_evaluate() {
    // Const evaluation is the real test; these asserts read evaluated values.
    assert_eq!(counter_style().display, taffy::style::Display::Flex);
    assert_eq!(counter_style().size.width, taffy::Dimension::length(100.0));
    assert_eq!(
        padding_style().padding.top,
        taffy::LengthPercentage::length(16.0)
    );
    assert_eq!(
        list_style().flex_direction,
        taffy::style::FlexDirection::Column
    );
}

#[test]
fn emitter_output_matches_the_const_fn_syntax() {
    use lig_core::span::Span;
    use lig_style::parse_declarations;

    let ir = parse_declarations(
        "display:flex;width:100px;align-items:center",
        Span::PLACEHOLDER,
    )
    .expect("ok");
    let out = lig_taffy_profile::lower_to_taffy_const_fn(&ir, "counter_style");
    insta::assert_snapshot!("counter_emitted_const_fn", out.const_fn_text);
    assert!(
        out.const_fn_text
            .contains("s.display = taffy::style::Display::Flex;")
    );
    assert!(
        out.const_fn_text
            .contains("s.size.width = taffy::Dimension::length(100.0);")
    );
    assert!(
        out.const_fn_text
            .contains("s.align_items = taffy::style::AlignItems::CENTER;")
    );
}

#[test]
fn emitter_matches_padding_const_syntax() {
    use lig_core::span::Span;
    use lig_style::parse_declarations;

    let ir = parse_declarations("padding:16px", Span::PLACEHOLDER).expect("ok");
    let out = lig_taffy_profile::lower_to_taffy_const_fn(&ir, "padding_style");
    assert!(
        out.const_fn_text.contains("s.padding = taffy::Rect { top: taffy::LengthPercentage::length(16.0), right: taffy::LengthPercentage::length(16.0), bottom: taffy::LengthPercentage::length(16.0), left: taffy::LengthPercentage::length(16.0) };"),
        "{}",
        out.const_fn_text
    );
}

#[test]
fn emitter_matches_gap_const_syntax() {
    use lig_core::span::Span;
    use lig_style::parse_declarations;

    let ir = parse_declarations("gap:8px", Span::PLACEHOLDER).expect("ok");
    let out = lig_taffy_profile::lower_to_taffy_const_fn(&ir, "list_style");
    assert!(
        out.const_fn_text.contains("s.gap = taffy::Size { width: taffy::LengthPercentage::length(8.0), height: taffy::LengthPercentage::length(0.0) };"),
        "{}",
        out.const_fn_text
    );
}

#[test]
fn snapshot_profile_v1_emission() {
    use lig_core::span::Span;
    use lig_style::parse_declarations;

    let ir = parse_declarations(
        "display:flex;padding:16px;gap:8px 16px;flex-direction:column;align-items:center;justify-content:space-between",
        Span::PLACEHOLDER,
    )
    .expect("ok");
    let out = lig_taffy_profile::lower_to_taffy_const_fn(&ir, "s0");
    assert_eq!(
        out.unmapped.len(),
        0,
        "all six properties map: {:?}",
        out.unmapped
    );
    insta::assert_snapshot!("profile_v1_taffy_const", out.const_fn_text);
}
