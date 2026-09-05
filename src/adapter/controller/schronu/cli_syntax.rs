#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CliLexErrorKind {
    UnterminatedSingleQuote,
    UnterminatedDoubleQuote,
    TrailingBackslash,
}

impl CliLexErrorKind {
    pub(super) const fn reason(self) -> &'static str {
        match self {
            Self::UnterminatedSingleQuote => "single quoteが閉じられていません",
            Self::UnterminatedDoubleQuote => "double quoteが閉じられていません",
            Self::TrailingBackslash => "末尾のbackslashにescape対象がありません",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CliLexError {
    kind: CliLexErrorKind,
}

impl CliLexError {
    fn new(kind: CliLexErrorKind) -> Self {
        Self { kind }
    }

    pub(super) const fn kind(self) -> CliLexErrorKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Quote {
    Single,
    Double,
}

pub(super) fn tokenize(input: &str) -> Result<Vec<String>, CliLexError> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut token_started = false;
    let mut quote = None;
    let mut characters = input.chars();

    while let Some(character) = characters.next() {
        match quote {
            Some(Quote::Single) => {
                if character == '\'' {
                    quote = None;
                } else {
                    token.push(character);
                }
            }
            Some(Quote::Double) => match character {
                '"' => quote = None,
                '\\' => token.push(
                    characters
                        .next()
                        .ok_or_else(|| CliLexError::new(CliLexErrorKind::TrailingBackslash))?,
                ),
                _ => token.push(character),
            },
            None => match character {
                '\'' => {
                    token_started = true;
                    quote = Some(Quote::Single);
                }
                '"' => {
                    token_started = true;
                    quote = Some(Quote::Double);
                }
                '\\' => {
                    token_started = true;
                    token.push(
                        characters
                            .next()
                            .ok_or_else(|| CliLexError::new(CliLexErrorKind::TrailingBackslash))?,
                    );
                }
                _ if character.is_whitespace() => {
                    if token_started {
                        tokens.push(std::mem::take(&mut token));
                        token_started = false;
                    }
                }
                _ => {
                    token_started = true;
                    token.push(character);
                }
            },
        }
    }

    match quote {
        Some(Quote::Single) => {
            return Err(CliLexError::new(CliLexErrorKind::UnterminatedSingleQuote));
        }
        Some(Quote::Double) => {
            return Err(CliLexError::new(CliLexErrorKind::UnterminatedDoubleQuote));
        }
        None => {}
    }

    if token_started {
        tokens.push(token);
    }
    Ok(tokens)
}
