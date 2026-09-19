use super::source::fixture_modules;
use std::collections::BTreeMap;

fn violations(_modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    Vec::new()
}

fn fixture(entry: &str) -> BTreeMap<String, syn::File> {
    fixture_modules("mod runtime; mod command; mod handler;", &[
        ("command.rs", "pub(super) fn decode_line(line: &str) -> Result<Command, Error> { todo!() } pub(super) fn decode_args(args: &[String]) -> Result<Command, Error> { todo!() }"),
        ("handler.rs", "pub(super) fn unified<C: CommandContext>(command: &Command, context: &mut C) -> CommandOutcome { todo!() } pub(super) fn component(command: &Command) -> CommandOutcome { todo!() }"),
        ("runtime.rs", &format!("use super::command::{{Command, CommandKind, decode_line, decode_args}}; use super::handler::{{unified, component}};
            fn coordinate(command: &Command) -> Result<(), CommandError> {{ unified(command, context); Ok(()) }}
            fn terminal(repository: &mut dyn TaskRepositoryTrait, input: &str) -> InteractiveCommandExecution {{ let command = decode_line(input); {entry} }}
            fn argv(repository: &mut dyn TaskRepositoryTrait, input: &[String]) -> Result<(), RunError> {{ let command = decode_args(input); coordinate(&command); Ok(()) }}")),
    ])
}

#[test]
fn entry_cannot_bypass_the_shared_typed_dispatcher() {
    assert!(!violations(&fixture("component(&command);")).is_empty());
}
