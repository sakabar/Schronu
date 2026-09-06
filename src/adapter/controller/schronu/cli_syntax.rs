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

pub(super) struct CliTokenization {
    pub(super) tokens: Result<Vec<String>, CliLexError>,
    pub(super) prefix_tokens: Vec<String>,
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
    tokenize_with_prefix(input).tokens
}

pub(super) fn tokenize_with_prefix(input: &str) -> CliTokenization {
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
                '\\' => {
                    let Some(escaped) = characters.next() else {
                        return failed_tokenization(
                            tokens,
                            token,
                            token_started,
                            CliLexErrorKind::TrailingBackslash,
                        );
                    };
                    token.push(escaped);
                }
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
                    let Some(escaped) = characters.next() else {
                        return failed_tokenization(
                            tokens,
                            token,
                            token_started,
                            CliLexErrorKind::TrailingBackslash,
                        );
                    };
                    token.push(escaped);
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
            return failed_tokenization(
                tokens,
                token,
                token_started,
                CliLexErrorKind::UnterminatedSingleQuote,
            );
        }
        Some(Quote::Double) => {
            return failed_tokenization(
                tokens,
                token,
                token_started,
                CliLexErrorKind::UnterminatedDoubleQuote,
            );
        }
        None => {}
    }

    if token_started {
        tokens.push(token);
    }
    CliTokenization {
        prefix_tokens: tokens.clone(),
        tokens: Ok(tokens),
    }
}

fn failed_tokenization(
    mut tokens: Vec<String>,
    token: String,
    token_started: bool,
    kind: CliLexErrorKind,
) -> CliTokenization {
    if token_started {
        tokens.push(token);
    }
    CliTokenization {
        prefix_tokens: tokens,
        tokens: Err(CliLexError::new(kind)),
    }
}
