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
