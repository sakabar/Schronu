use super::command::{
    parse_interactive_command, parse_non_interactive_command_tokens, Command, CommandParseError,
    ParseMode,
};

// Legacy string fixtures use this test-only adapter; product argv uses the token entry directly.
pub(super) fn parse_command(input: &str, mode: ParseMode) -> Result<Command, CommandParseError> {
    if input.trim().is_empty() || input.trim_start().starts_with('#') || input.starts_with('0') {
        return Ok(Command::Noop);
    }

    match mode {
        ParseMode::Interactive => parse_interactive_command(input),
        ParseMode::NonInteractive => {
            let tokens = input
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            parse_non_interactive_command_tokens(&tokens)
        }
    }
}
