const MAIN_CSS: &str = include_str!("../assets/main.css");
const HOVER_MEDIA_QUERY: &str = "@media (hover: hover) and (pointer: fine)";

#[test]
fn button_hover_styles_are_limited_to_hover_capable_fine_pointers() {
    let hover_block = block_body(MAIN_CSS, HOVER_MEDIA_QUERY);
    let media_start = MAIN_CSS
        .find(HOVER_MEDIA_QUERY)
        .expect("button hover styles must have a pointer capability guard");
    let media_end = matching_block_end(MAIN_CSS, media_start);
    let css_outside_hover_guard =
        format!("{}{}", &MAIN_CSS[..media_start], &MAIN_CSS[media_end + 1..]);

    assert!(
        !css_outside_hover_guard.contains(":hover"),
        "hover selectors must not apply to touch-primary pointers"
    );
    assert!(css_outside_hover_guard.contains("button:active:not(:disabled)"));
    assert!(css_outside_hover_guard.contains("button:focus-visible"));

    for selector in [
        "button:hover:not(:disabled)",
        ".tab-button:hover:not(:disabled)",
        ".primary-action:hover:not(:disabled)",
        ".session-start:hover:not(:disabled)",
    ] {
        assert!(
            hover_block.contains(selector),
            "hover guard must preserve `{selector}`"
        );
    }

    let selected_date_hover =
        block_body(hover_block, ".date-pill.is-selected:hover:not(:disabled)");
    assert!(selected_date_hover.contains("border-color: var(--green-dark);"));
    assert!(selected_date_hover.contains("background: var(--green);"));
    assert!(selected_date_hover.contains("color: white;"));
}

#[test]
fn history_invocation_wraps_long_arguments_inside_the_viewport() {
    let invocation = block_body(MAIN_CSS, ".history-invocation");

    assert!(invocation.contains("overflow-wrap: anywhere;"));
    assert!(invocation.contains("min-width: 0;"));
}

#[test]
fn loading_overlay_blocks_the_viewport_and_respects_reduced_motion() {
    let overlay = block_body(MAIN_CSS, ".loading-overlay");
    assert!(overlay.contains("position: fixed;"));
    assert!(overlay.contains("inset: 0;"));
    assert!(overlay.contains("z-index:"));
    assert!(overlay.contains("pointer-events: auto;"));

    let spinner = block_body(MAIN_CSS, ".loading-spinner");
    assert!(spinner.contains("animation:"));
    assert!(MAIN_CSS.contains("@keyframes loading-spin"));

    let reduced_motion = block_body(MAIN_CSS, "@media (prefers-reduced-motion: reduce)");
    assert!(block_body(reduced_motion, ".loading-spinner").contains("animation: none;"));
}

#[test]
fn navigation_is_fixed_safe_and_never_covers_page_content() {
    let root = block_body(MAIN_CSS, ":root");
    assert!(root.contains("--bottom-navigation-height: 3.5rem;"));

    let tabs = block_body(MAIN_CSS, ".tabs");
    assert!(tabs.contains("position: fixed;"));
    assert!(tabs.contains("bottom: 0;"));
    assert!(tabs.contains("env(safe-area-inset-bottom)"));
    assert!(tabs.contains(
        "min-height: calc(var(--bottom-navigation-height) + env(safe-area-inset-bottom));"
    ));
    assert!(tabs.contains("grid-template-columns: repeat(4, minmax(0, 1fr));"));

    let tab_button = block_body(MAIN_CSS, ".tab-button");
    assert!(tab_button.contains("min-height: max(2.75rem, 44px);"));

    let mobile = block_body(MAIN_CSS, "@media (max-width: 46rem)");
    let mobile_root = block_body(mobile, ":root");
    assert!(mobile_root.contains("--bottom-navigation-height: 40px;"));
    let mobile_tab_button = block_body(mobile, ".tab-button");
    assert!(mobile_tab_button.contains("min-height: 40px;"));
    assert!(mobile_tab_button.contains("padding: 0.4rem 0.35rem;"));

    let shell = block_body(MAIN_CSS, ".shell");
    assert!(shell.contains("var(--bottom-navigation-height)"));
    assert!(shell.contains("env(safe-area-inset-bottom)"));

    let tabs_z_index = numeric_property(tabs, "z-index");
    let overlay_z_index = numeric_property(block_body(MAIN_CSS, ".loading-overlay"), "z-index");
    assert!(tabs_z_index < overlay_z_index);
}

#[test]
fn load_rows_fit_320_360_46rem_and_1024px_viewports() {
    let view = block_body(MAIN_CSS, ".load-view");
    assert!(view.contains("width: min(100%, 44rem);"));

    let days = block_body(MAIN_CSS, ".load-days");
    assert!(days.contains("width: 100%;"));
    assert!(days.contains("min-width: 0;"));

    let row = block_body(MAIN_CSS, ".load-day {");
    assert!(row.contains("width: 100%;"));
    assert!(row.contains("min-width: 0;"));

    for selector in [".load-day-heading", ".load-day-footer"] {
        let shared = block_body(
            MAIN_CSS,
            ".load-toolbar,\n.load-day-heading,\n.load-day-footer",
        );
        assert!(MAIN_CSS.contains(selector));
        assert!(shared.contains("min-width: 0;"));
        assert!(shared.contains("flex-wrap: wrap;"));
    }
    let metric = block_body(MAIN_CSS, ".load-metric {");
    assert!(metric.contains("align-items: baseline;"));
    assert!(MAIN_CSS.contains("@media (max-width: 46rem)"));
}

#[test]
fn load_rows_fit_without_scroll_from_35rem_and_compact_through_50rem() {
    let no_scroll = block_body(MAIN_CSS, "@media (min-height: 35rem)");
    assert!(!MAIN_CSS.contains("@media (max-height: 50rem)"));

    let view = block_body(no_scroll, ".load-view");
    assert!(view.contains("height: calc(100dvh"));
    assert!(view.contains("display: flex;"));
    assert!(view.contains("flex-direction: column;"));
    let error_view = block_body(no_scroll, ".load-view.has-error");
    assert!(error_view.contains("height: auto;"));

    let days = block_body(no_scroll, ".load-days");
    assert!(days.contains("flex: 1 1 auto;"));
    assert!(days.contains("min-height: 0;"));
    assert!(days
        .contains("grid-template-rows: minmax(4.25rem, 1.55fr) repeat(6, minmax(2.75rem, 1fr));"));

    let compact = block_body(
        MAIN_CSS,
        "@media (min-height: 35rem) and (max-height: 50rem)",
    );
    let legend = block_body(compact, ".band-legend");
    assert!(legend.contains("grid-template-columns: repeat(4, minmax(0, 1fr));"));
    let overview_caption = block_body(compact, ".load-overview-caption");
    assert!(overview_caption.contains("display: none;"));

    let row = block_body(compact, ".load-day {");
    assert!(row.contains("min-height: 0;"));
    assert!(row.contains("padding: 0.2rem 0.4rem;"));
    assert!(row.contains("font-size: 0.72rem;"));
    assert!(row.contains("line-height: 1.15;"));
}

#[test]
fn background_refresh_status_floats_above_navigation_without_affecting_layout() {
    let status = block_body(MAIN_CSS, ".background-refresh-status");
    assert!(status.contains("position: fixed;"));
    assert!(status.contains("left: 50%;"));
    assert!(status.contains(
        "bottom: calc(var(--bottom-navigation-height) + env(safe-area-inset-bottom) + 0.75rem);"
    ));
    assert!(status.contains("width: min(calc(100% - 2rem), 40rem);"));
    assert!(status.contains("transform: translateX(-50%);"));

    let status_z_index = numeric_property(status, "z-index");
    let tabs_z_index = numeric_property(block_body(MAIN_CSS, ".tabs"), "z-index");
    let overlay_z_index = numeric_property(block_body(MAIN_CSS, ".loading-overlay"), "z-index");
    assert!(tabs_z_index < status_z_index);
    assert!(status_z_index < overlay_z_index);
}

#[test]
fn session_countdown_is_prominent_and_metadata_wraps_at_mobile_widths() {
    let desktop_card = block_body(MAIN_CSS, ".session-card");
    assert!(desktop_card
        .contains("grid-template-columns: minmax(11rem, 1.1fr) minmax(18rem, 2fr) auto;"));
    for area in [
        "\"heading progress actions\"",
        "\"timing progress actions\"",
    ] {
        assert!(
            desktop_card.contains(area),
            "missing {area} in {desktop_card}"
        );
    }

    let timing = block_body(MAIN_CSS, ".session-timing");
    assert!(timing.contains("display: grid;"));

    let remaining = block_body(MAIN_CSS, ".session-remaining {");
    assert!(remaining.contains("font-size: clamp(1.75rem, 4vw, 2.5rem);"));

    let metadata = block_body(MAIN_CSS, ".session-timing-meta");
    assert!(metadata.contains("display: flex;"));
    assert!(metadata.contains("min-width: 0;"));
    assert!(metadata.contains("flex-wrap: wrap;"));

    let actual_at_start = block_body(MAIN_CSS, ".session-start-actual");
    assert!(actual_at_start.contains("flex: 0 0 auto;"));
    assert!(actual_at_start.contains("white-space: nowrap;"));

    let mobile = block_body(MAIN_CSS, "@media (max-width: 52rem)");
    let card = block_body(mobile, ".session-card");
    for area in ["\"heading\"", "\"timing\"", "\"progress\"", "\"actions\""] {
        assert!(card.contains(area), "missing {area} in {card}");
    }

    let narrow = block_body(MAIN_CSS, "@media (max-width: 34rem)");
    let narrow_remaining = block_body(narrow, ".session-remaining");
    assert!(narrow_remaining.contains("font-size: clamp(1.75rem, 9vw, 2.25rem);"));
}

#[test]
fn session_progressは150_percent超過を赤色の横scroll領域として保持する() {
    let scroll = block_body(MAIN_CSS, ".session-progress-scroll");
    assert!(scroll.contains("overflow-x: auto;"));

    let track = block_body(MAIN_CSS, ".session-progress-track");
    assert!(track.contains("overflow: visible;"));

    let segments = block_body(
        MAIN_CSS,
        ".session-progress-normal,\n.session-progress-overrun",
    );
    assert!(segments.contains("flex: 0 0 auto;"));

    assert!(MAIN_CSS.contains(
        ".session-progress-overrun {\n    background: var(--red);\n    border-radius: 0 999px 999px 0;\n}"
    ));
}

#[test]
fn session_progressは見積も100_percent位置に常時表示の境界線を置く() {
    let track = block_body(MAIN_CSS, ".session-progress-track");
    assert!(track.contains("position: relative;"));

    let marker = block_body(MAIN_CSS, ".session-progress-estimate-marker");
    assert!(marker.contains("position: absolute;"));
    assert!(marker.contains("left: calc(100% / 1.5);"));
    assert!(marker.contains("width: 2px;"));
    assert!(marker.contains("background: var(--ink);"));
    assert!(marker.contains("box-shadow: 0 0 0 1px var(--surface);"));
    assert!(marker.contains("pointer-events: none;"));
}

fn block_body<'a>(source: &'a str, header: &str) -> &'a str {
    let header_start = source
        .find(header)
        .unwrap_or_else(|| panic!("missing CSS block `{header}`"));
    let open_brace = source[header_start..]
        .find('{')
        .map(|offset| header_start + offset)
        .unwrap_or_else(|| panic!("missing opening brace for `{header}`"));
    let close_brace = matching_block_end(source, open_brace);

    &source[open_brace + 1..close_brace]
}

fn matching_block_end(source: &str, block_start: usize) -> usize {
    let open_brace = source[block_start..]
        .find('{')
        .map(|offset| block_start + offset)
        .expect("CSS block must have an opening brace");
    let mut depth = 0_u32;

    for (offset, character) in source[open_brace..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return open_brace + offset;
                }
            }
            _ => {}
        }
    }

    panic!("CSS block must have a closing brace");
}

fn numeric_property(block: &str, property: &str) -> i32 {
    block
        .lines()
        .find_map(|line| {
            let value = line.trim().strip_prefix(&format!("{property}:"))?;
            value.trim().trim_end_matches(';').parse().ok()
        })
        .unwrap_or_else(|| panic!("missing numeric property `{property}`"))
}
