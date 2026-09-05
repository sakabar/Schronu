#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskNameViolation {
    Blank,
    IntegerOnly,
    ControlCharacter,
}

impl TaskNameViolation {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Blank => "must not be blank",
            Self::IntegerOnly => "must not be an integer-only name",
            Self::ControlCharacter => "must not contain control characters",
        }
    }
}

pub(crate) fn validate(name: &str) -> Result<(), TaskNameViolation> {
    if name.is_empty() {
        return Err(TaskNameViolation::Blank);
    }
    if name.chars().any(char::is_control) {
        return Err(TaskNameViolation::ControlCharacter);
    }

    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err(TaskNameViolation::Blank);
    }
    if is_integer_only_name(trimmed_name) {
        return Err(TaskNameViolation::IntegerOnly);
    }
    Ok(())
}

fn is_integer_only_name(name: &str) -> bool {
    let digits = name
        .strip_prefix('+')
        .or_else(|| name.strip_prefix('-'))
        .unwrap_or(name);
    !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
}
