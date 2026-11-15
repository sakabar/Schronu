pub(super) fn format_unsigned(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

// First matching minimum wins. Keep the bands in descending order and end at 0.
const PROBABILITY_COLOR_BANDS: &[(u8, &str)] = &[(50, "var(--red)"), (0, "var(--green-dark)")];

pub(super) fn probability_color(percent: Option<u8>) -> Option<&'static str> {
    let percent = percent?;
    PROBABILITY_COLOR_BANDS
        .iter()
        .find(|&&(minimum, _)| percent >= minimum)
        .map(|&(_, color)| color)
}
