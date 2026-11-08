const MAIN_CSS: &str = include_str!("../assets/main.css");
const HOVER_MEDIA_QUERY: &str = "@media (hover: hover) and (pointer: fine)";

#[test]
fn load_band_palette_distinguishes_adjacent_blue_and_green_segments() {
    let root = block_body(MAIN_CSS, ":root");

    assert!(root.contains("--band-repetitive: #60a5fa;"));
    assert!(root.contains("--band-leeway: #166534;"));
    assert!(root.contains("--band-free: #4ade80;"));
}

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

    let grid = block_body(MAIN_CSS, ".load-grid");
    assert!(grid.contains("--load-columns: 64px 6ch 6ch 5ch minmax(0, 1fr);"));
    assert!(row.contains("font-weight: inherit;"));
    let heading = block_body(MAIN_CSS, ".load-column-headings");
    for block in [heading, row] {
        assert!(block.contains("grid-template-columns: var(--load-columns);"));
        assert!(block.contains("column-gap: var(--load-column-gap);"));
    }
    let narrow = block_body(MAIN_CSS, "@media (max-width: 380px)");
    assert!(block_body(narrow, ".load-grid")
        .contains("--load-columns: 60px 6ch 6ch 5ch minmax(0, 1fr);"));
    assert!(block_body(MAIN_CSS, ".load-bands").contains("gap: 4px;"));
    assert!(
        block_body(MAIN_CSS, ".load-metric-value.is-within").contains("color: var(--green-dark);")
    );
    assert!(block_body(MAIN_CSS, ".load-metric-value.is-over").contains("color: var(--red);"));
}

#[test]
fn routine_load_table_scrolls_only_the_subject_column() {
    let wrapper = block_body(MAIN_CSS, ".routine-load-table-wrap");
    assert!(wrapper.contains("overflow-x: visible;"));
    assert!(!wrapper.contains("overflow-x: auto;"));
    assert!(wrapper.contains("max-width: 100%;"));

    let table = block_body(MAIN_CSS, ".routine-load-table {");
    assert!(table.contains("min-width: 0;"));
    assert!(table.contains("width: 100%;"));
    assert!(table.contains("table-layout: fixed;"));

    for (selector, width) in [
        (".routine-load-interval", "6rem"),
        (".routine-load-total", "6.5rem"),
        (".routine-load-occurrences", "6.5rem"),
        (".routine-load-average", "6.5rem"),
        (".routine-load-difference", "5rem"),
    ] {
        let column = block_body(MAIN_CSS, selector);
        assert!(
            column.contains(&format!("width: {width};")),
            "{selector}: {column}"
        );
        assert!(
            column.contains(&format!("min-width: {width};")),
            "{selector}: {column}"
        );
        assert!(
            column.contains("text-align: right;"),
            "{selector}: {column}"
        );
    }

    let subject = block_body(MAIN_CSS, ".routine-load-subject");
    assert!(subject.contains("min-width: 0;"));
    assert!(subject.contains("overflow: hidden;"));
    assert!(subject.contains("text-align: left;"));

    let subject_scroll = block_body(MAIN_CSS, ".routine-load-subject-scroll");
    for contract in [
        "min-width: 0;",
        "width: 100%;",
        "overflow-x: auto;",
        "overflow-y: hidden;",
        "overscroll-behavior-inline: contain;",
        "white-space: nowrap;",
        "scrollbar-width: thin;",
        "touch-action: pan-x pan-y pinch-zoom;",
    ] {
        assert!(
            subject_scroll.contains(contract),
            "missing {contract}: {subject_scroll}"
        );
    }
    let subject_focus = block_body(MAIN_CSS, ".routine-load-subject-scroll:focus-visible");
    assert!(subject_focus.contains("outline:"));

    let narrow = block_body(MAIN_CSS, "@media (max-width: 46rem)");
    for (selector, width) in [
        (".routine-load-interval", "11%"),
        (".routine-load-total", "15%"),
        (".routine-load-occurrences", "14%"),
        (".routine-load-average", "15%"),
        (".routine-load-difference", "13%"),
        (".routine-load-subject", "32%"),
    ] {
        let column = block_body(narrow, selector);
        assert!(
            column.contains(&format!("width: {width};")),
            "{selector}: {column}"
        );
        assert!(column.contains("min-width: 0;"), "{selector}: {column}");
        assert!(
            !column.contains("overflow-x: auto;"),
            "{selector}: {column}"
        );
    }
    let narrow_cells = block_body(
        narrow,
        ".routine-load-table th,\n    .routine-load-table td",
    );
    assert!(narrow_cells.contains("white-space: normal;"));
    assert!(narrow_cells.contains("overflow-wrap: anywhere;"));
    assert!(!MAIN_CSS.contains(".routine-load-weekly"));

    assert!(!MAIN_CSS.contains("routine-load-peak"));
    assert!(MAIN_CSS.contains(".task-kind-repetitive {\n    color: var(--task-repetitive);"));
}

#[test]
fn routine_load_map_and_scope_table_fit_320px_touch_viewports() {
    let map = block_body(MAIN_CSS, ".routine-load-map");
    assert!(map.contains("grid-template-columns: repeat(7, minmax(0, 1fr));"));
    assert!(map.contains("min-width: 0;"));
    assert!(map.contains("width: 100%;"));

    let day = block_body(MAIN_CSS, ".routine-load-day");
    assert!(day.contains("min-height: 40px;"));
    assert!(day.contains("min-width: 0;"));

    let wrapper = block_body(MAIN_CSS, ".routine-load-table-wrap");
    assert!(!wrapper.contains("overflow-x: auto;"));
    let narrow = block_body(MAIN_CSS, "@media (max-width: 46rem)");
    assert!(block_body(narrow, ".routine-load-map").contains("gap:"));
}

#[test]
fn routine_load_capacity_threshold_uses_dark_red_with_white_text() {
    let root = block_body(MAIN_CSS, ":root");
    assert!(root.contains("--red-dark: #982b31;"), "{root}");

    let weekday = block_body(MAIN_CSS, ".routine-load-weekday");
    assert!(
        weekday.contains("background: var(--surface-muted);"),
        "{weekday}"
    );
    let day = block_body(MAIN_CSS, ".routine-load-day");
    assert!(day.contains("var(--routine-load-intensity)"), "{day}");

    let capacity = block_body(
        MAIN_CSS,
        "button.routine-load-weekday.is-at-capacity,\nbutton.routine-load-day.is-at-capacity",
    );
    assert!(
        capacity.contains("background: var(--red-dark);"),
        "{capacity}"
    );
    assert!(capacity.contains("color: white;"), "{capacity}");
}

#[test]
fn load_rows_keep_readable_heights_with_page_scrolling() {
    assert!(!MAIN_CSS.contains("@media (min-height: 35rem)"));
    assert!(!MAIN_CSS.contains("grid-template-rows: repeat(8,"));
    let days = block_body(MAIN_CSS, ".load-days");
    assert!(days.contains("--load-row-gap: 5px;"));
    assert!(days.contains("gap: calc(var(--load-row-gap) + 8px);"));
    let week = block_body(MAIN_CSS, ".load-week {");
    assert!(week.contains("grid-auto-rows: auto;"));
    assert!(week.contains("gap: var(--load-row-gap);"));
    assert!(!week.contains("margin"));
    let row = block_body(MAIN_CSS, ".load-day {");
    assert!(row.contains("min-height: 48px;"));
    let view = block_body(MAIN_CSS, ".load-view");
    assert!(!view.contains("height:"));
    assert!(!view.contains("overflow:"));
    let compact = block_body(MAIN_CSS, "@media (max-height: 60rem)");
    assert!(block_body(compact, ".load-days").contains("--load-row-gap: 2px;"));
    let legend = block_body(compact, ".band-legend");
    assert!(legend.contains("flex-wrap: nowrap;"));
    let row = block_body(compact, ".load-day {");
    assert!(row.contains("min-height: 36px;"));
    let stack = block_body(MAIN_CSS, ".load-band-stack");
    assert!(stack.contains("min-width: 0;"));
    assert!(stack.contains("overflow: hidden;"));
    let fill = block_body(MAIN_CSS, ".load-overflow-fill");
    assert!(fill.contains("min-width: 2px;"));
    assert!(fill.contains("margin-left: auto;"));
    assert!(block_body(MAIN_CSS, ".load-overflow-fill.is-zero").contains("min-width: 0;"));
    assert!(block_body(compact, ".load-band {").contains("height: 5px;"));
    assert!(block_body(compact, ".load-overflow-rail").contains("height: 2px;"));
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

#[test]
fn 完了表は狭幅へ収まりtaskとprojectのcellだけが横scrollする() {
    let list_screen = block_body(MAIN_CSS, ".list-screen");
    assert!(list_screen.contains("display: grid;"));
    assert!(list_screen.contains("min-width: 0;"));

    let mode_control = block_body(MAIN_CSS, ".list-mode-control");
    assert!(mode_control.contains("grid-template-columns: repeat(2, minmax(0, 1fr));"));

    let scroll = block_body(MAIN_CSS, ".completed-task-table-scroll");
    assert!(scroll.contains("width: 100%;"));
    assert!(scroll.contains("min-width: 0;"));
    assert!(scroll.contains("overflow-x: visible;"));
    assert!(!scroll.contains("overflow-x: auto;"));

    let table = block_body(MAIN_CSS, ".completed-task-table {");
    assert!(table.contains("width: 100%;"));
    assert!(table.contains("min-width: 0;"));
    assert!(table.contains("table-layout: fixed;"));

    let subject = block_body(MAIN_CSS, ".completed-task-table .completed-subject");
    assert!(subject.contains("min-width: 0;"));
    assert!(subject.contains("overflow: hidden;"));
    assert!(subject.contains("text-align: left;"));

    let subject_scroll = block_body(MAIN_CSS, ".completed-subject-scroll");
    for contract in [
        "min-width: 0;",
        "width: 100%;",
        "overflow-x: auto;",
        "overflow-y: hidden;",
        "overscroll-behavior-inline: contain;",
        "white-space: nowrap;",
        "scrollbar-width: thin;",
        "touch-action: pan-x pan-y pinch-zoom;",
    ] {
        assert!(
            subject_scroll.contains(contract),
            "missing {contract}: {subject_scroll}"
        );
    }
    let subject_focus = block_body(MAIN_CSS, ".completed-subject-scroll:focus-visible");
    assert!(subject_focus.contains("outline:"));

    for (selector, width) in [
        (".completed-task-table .completed-time", "4.25rem"),
        (".completed-task-table .completed-actual", "5.5rem"),
        (".completed-task-table .completed-difference", "6rem"),
    ] {
        let column = block_body(MAIN_CSS, selector);
        assert!(
            column.contains(&format!("width: {width};")),
            "{selector}: {column}"
        );
        assert!(
            column.contains(&format!("min-width: {width};")),
            "{selector}: {column}"
        );
    }

    let narrow = block_body(MAIN_CSS, "@media (max-width: 46rem)");
    for (selector, width) in [
        (".completed-time", "13%"),
        (".completed-actual", "21%"),
        (".completed-difference", "23%"),
        (".completed-subject", "43%"),
    ] {
        let column = block_body(narrow, selector);
        assert!(
            column.contains(&format!("width: {width};")),
            "{selector}: {column}"
        );
        assert!(column.contains("min-width: 0;"), "{selector}: {column}");
    }
    let narrow_cells = block_body(
        narrow,
        ".completed-task-table th,\n    .completed-task-table td",
    );
    assert!(narrow_cells.contains("padding: 0.55rem 0.2rem;"));
    assert!(narrow_cells.contains("font-size: 0.7rem;"));

    let summary = block_body(MAIN_CSS, ".completed-report-summary");
    assert!(summary.contains("display: grid;"));
    assert!(summary.contains("width: 100%;"));
    assert!(summary.contains("min-width: 0;"));
    assert!(summary.contains("repeat(auto-fit"));
    assert!(summary.contains("minmax(min(100%,"));

    let summary_item = block_body(MAIN_CSS, ".completed-report-summary-item");
    assert!(summary_item.contains("min-width: 0;"));
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

#[test]
fn obsidian操作領域は時計の隣で縮まずtouch時に拡大する() {
    let countdown = block_body(MAIN_CSS, ".session-countdown {");
    assert!(countdown.contains("display: flex;"));
    assert!(countdown.contains("flex-wrap: wrap;"));
    let icon = block_body(MAIN_CSS, ".session-obsidian-shortcut {");
    assert!(icon.contains("align-self: center;"));
    assert!(icon.contains("flex: 0 0 auto;"));
    assert!(icon.contains("width: 28px;"));
    assert!(icon.contains("height: 28px;"));
    assert!(icon.contains("border: 0;"));
    let svg = block_body(MAIN_CSS, ".session-obsidian-shortcut svg {");
    assert!(svg.contains("width: 18px;"));
    assert!(svg.contains("height: 18px;"));
    let coarse = block_body(MAIN_CSS, "@media (pointer: coarse)");
    let touch = block_body(coarse, ".session-obsidian-shortcut {");
    assert!(touch.contains("width: 44px;"));
    assert!(touch.contains("height: 44px;"));
}

#[test]
fn routine_estimate_difference_colors_follow_displayed_sign() {
    assert!(
        block_body(MAIN_CSS, ".routine-load-difference.is-positive").contains("color: var(--red);")
    );
    assert!(block_body(MAIN_CSS, ".routine-load-difference.is-negative")
        .contains("color: var(--blue);"));
    assert!(
        !block_body(MAIN_CSS, ".routine-load-table .routine-load-difference {").contains("color:")
    );
}

#[test]
fn daily_load_compact_layout_retains_zero_and_weekend_colors_and_touch_target() {
    assert!(block_body(MAIN_CSS, ".load-metric-value.is-zero").contains("color: var(--load-zero);"));
    assert!(block_body(MAIN_CSS, ".load-weekday.is-saturday").contains("color: var(--blue);"));
    assert!(block_body(MAIN_CSS, ".load-weekday.is-sunday").contains("color: var(--red);"));
    let compact = block_body(MAIN_CSS, "@media (max-height: 60rem)");
    assert!(block_body(compact, ".load-day.is-today").contains("min-height: 40px;"));
    assert!(block_body(compact, ".load-refresh {").contains("min-height: 44px;"));
    assert!(block_body(compact, ".load-refresh span").contains("height: 26px;"));
}

#[test]
fn load_views_share_shell_padding_and_reserve_wrapped_navigation_height() {
    let selector = ".shell:has(.load-view)";
    assert!(!MAIN_CSS.contains(":not(.is-routine)"));
    let compact = block_body(MAIN_CSS, "@media (max-height: 60rem)");
    assert!(block_body(compact, selector).contains("+ 8px)"));
    let narrow = MAIN_CSS.rsplit("@media (max-width: 380px)").next().unwrap();
    assert!(block_body(narrow, selector).contains("+ 32px)"));
}

#[test]
fn future_session_start_uses_shared_palette_with_dark_text_on_hover() {
    let root = block_body(MAIN_CSS, ":root");
    for variable in [
        "--session-start-future:",
        "--session-start-future-hover:",
        "--session-start-future-border:",
    ] {
        assert!(root.contains(variable));
    }
    let normal = block_body(MAIN_CSS, ".session-start.is-future");
    assert!(normal.contains("background: var(--session-start-future);"));
    assert!(normal.contains("color: var(--ink);"));
    assert!(normal.contains("border-color: var(--session-start-future-border);"));
    let hover = block_body(
        block_body(MAIN_CSS, HOVER_MEDIA_QUERY),
        ".session-start.is-future:hover:not(:disabled)",
    );
    assert!(hover.contains("background: var(--session-start-future-hover);"));
    assert!(hover.contains("border-color: var(--session-start-future-hover);"));
}
