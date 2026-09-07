use schronu_web::client::date_input::{
    resolve_date_input, DateInputError, DateInputState, ResolvedDateInput,
};

#[test]
fn month_day_resolves_to_the_nearest_date_including_the_current_logical_date() {
    assert_eq!(
        resolve_date_input("9/16", "2026-09-15"),
        Ok(ResolvedDateInput {
            logical_date: "2026-09-16".to_owned(),
            display_value: "2026/9/16".to_owned(),
        })
    );
    assert_eq!(
        resolve_date_input("9/16", "2026-09-16"),
        Ok(ResolvedDateInput {
            logical_date: "2026-09-16".to_owned(),
            display_value: "2026/9/16".to_owned(),
        })
    );
    assert_eq!(
        resolve_date_input("9/16", "2026-09-17"),
        Ok(ResolvedDateInput {
            logical_date: "2027-09-16".to_owned(),
            display_value: "2027/9/16".to_owned(),
        })
    );
}

#[test]
fn month_day_crosses_the_year_boundary() {
    assert_eq!(
        resolve_date_input("1/1", "2026-12-31")
            .unwrap()
            .logical_date,
        "2027-01-01"
    );
}

#[test]
fn leap_day_resolves_to_the_nearest_valid_year() {
    assert_eq!(
        resolve_date_input("2/29", "2027-03-01")
            .unwrap()
            .logical_date,
        "2028-02-29"
    );
    assert_eq!(
        resolve_date_input("2/29", "2028-02-29")
            .unwrap()
            .logical_date,
        "2028-02-29"
    );
    assert_eq!(
        resolve_date_input("2/29", "2028-03-01")
            .unwrap()
            .logical_date,
        "2032-02-29"
    );
}

#[test]
fn explicit_year_is_preserved_and_slash_components_are_normalized() {
    assert_eq!(
        resolve_date_input(" 2025/09/06 ", "2026-09-16"),
        Ok(ResolvedDateInput {
            logical_date: "2025-09-06".to_owned(),
            display_value: "2025/9/6".to_owned(),
        })
    );
}

#[test]
fn invalid_formats_and_calendar_dates_are_rejected() {
    for input in ["", "9-16", "2026-9-16", "26/9/16", "2026/9/16/1", "９/１６"] {
        assert_eq!(
            resolve_date_input(input, "2026-09-16"),
            Err(DateInputError::InvalidFormat),
            "input: {input}"
        );
    }

    for input in ["13/1", "2/30", "2026/2/29"] {
        assert_eq!(
            resolve_date_input(input, "2026-09-16"),
            Err(DateInputError::InvalidDate),
            "input: {input}"
        );
    }
}

#[test]
fn invalid_current_date_and_output_overflow_are_distinguished() {
    assert_eq!(
        resolve_date_input("9/16", "2026/9/16"),
        Err(DateInputError::InvalidCurrentLogicalDate)
    );
    assert_eq!(
        resolve_date_input("1/1", "9999-12-31"),
        Err(DateInputError::DateOverflow)
    );
}

#[test]
fn input_state_keeps_normalized_success_and_clears_stale_errors_on_edit_or_clear() {
    let mut state = DateInputState::default();
    state.edit("bad".to_owned());
    assert_eq!(state.submit("2026-09-16"), None);
    assert_eq!(state.error(), Some(DateInputError::InvalidFormat));

    state.edit("9/16".to_owned());
    assert_eq!(state.error(), None);
    assert_eq!(state.submit("2026-09-16"), Some("2026-09-16".to_owned()));
    assert_eq!(state.text(), "2026/9/16");

    state.clear();
    assert_eq!(state.text(), "");
    assert_eq!(state.error(), None);
}

#[test]
fn whitespace_only_input_does_not_submit_or_report_an_error() {
    let mut state = DateInputState::default();
    state.edit("   ".to_owned());

    assert_eq!(state.submit("2026-09-16"), None);
    assert_eq!(state.error(), None);
}
