use regex::Regex;
use schronu::application::task_use_case::{estimated_work_seconds_from_minutes, ApplicationError};
use schronu::entity::datetime::parse_local_datetime;
use schronu::entity::task::{read_project_category, ProjectCategory};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ParseMode {
    Interactive,
    NonInteractive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CommandKind {
    Noop,
    NewProject,
    HobbyProject,
    UnplannedProject,
    Sequential,
    Repeat,
    Appointment,
    Start,
    Tree,
    Ancestor,
    Root,
    Leaves,
    ShowAll,
    Tail,
    Today,
    NonRepetitive,
    Calendar,
    Band,
    Focus,
    Pick,
    Open,
    Obsidian,
    Unfocus,
    Parent,
    Children,
    Deepest,
    NextUp,
    Breakdown,
    Split,
    Wait,
    Deadline,
    Estimate,
    Arrange,
    Actual,
    Priority,
    Category,
    Work,
    TuckAway,
    Defer,
    DeferRoutines,
    Escape,
    Flatten,
    Pack,
    Extrude,
    Clear,
    Gather,
    Finish,
    FocusHighest,
    FocusLowest,
    Verify,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CommandDefinition {
    kind: CommandKind,
    canonical_name: &'static str,
    usage: &'static str,
    minimum_arguments: usize,
    maximum_arguments: Option<usize>,
}

impl CommandDefinition {
    const fn new(
        kind: CommandKind,
        canonical_name: &'static str,
        usage: &'static str,
        minimum_arguments: usize,
        maximum_arguments: Option<usize>,
    ) -> Self {
        Self {
            kind,
            canonical_name,
            usage,
            minimum_arguments,
            maximum_arguments,
        }
    }

    fn validate_argument_count(self, arguments: &[String]) -> Result<(), CommandParseError> {
        let has_enough = arguments.len() >= self.minimum_arguments;
        let has_no_excess = self
            .maximum_arguments
            .is_none_or(|maximum| arguments.len() <= maximum);
        if has_enough && has_no_excess {
            Ok(())
        } else {
            Err(parse_error(
                self.canonical_name,
                "arguments",
                "引数の個数が正しくありません",
                self.usage,
            ))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Command {
    Noop,
    Estimate {
        minutes: i64,
    },
    Focus {
        task_id: Uuid,
    },
    Arrange {
        minutes: i64,
        includes_zero_estimate: bool,
    },
    TuckAway,
    Defer {
        amount: i64,
        unit: String,
    },
    ShowAll {
        pattern: Option<String>,
    },
    InteractiveShortcut(InteractiveShortcut),
    Action(CommandAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InteractiveShortcut {
    NextMorning,
    NextWeek,
    DeferRoutine,
    FiveYears,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CommandAction {
    NoArguments {
        kind: CommandKind,
        canonical_name: &'static str,
    },
    NewProject {
        kind: CommandKind,
        canonical_name: &'static str,
        name: String,
        estimated_minutes: Option<i64>,
    },
    Sequential {
        name: String,
        estimated_minutes: i64,
        begin_index: i64,
        end_index: i64,
        suffix: Option<String>,
    },
    Repeat {
        name: String,
        estimated_minutes: i64,
        day: String,
        start_time: String,
        deadline_time: String,
    },
    TimeExpression {
        kind: CommandKind,
        canonical_name: &'static str,
        values: Vec<String>,
    },
    OptionalPattern {
        kind: CommandKind,
        canonical_name: &'static str,
        pattern: Option<String>,
    },
    Pick {
        task_id: Option<Uuid>,
    },
    TaskWithEstimate {
        kind: CommandKind,
        canonical_name: &'static str,
        name: String,
        estimated_minutes: Option<i64>,
    },
    TaskNames {
        names: Vec<String>,
    },
    Split {
        minutes: i64,
        name: String,
    },
    StringValue {
        kind: CommandKind,
        canonical_name: &'static str,
        value: String,
    },
    IntegerValue {
        kind: CommandKind,
        canonical_name: &'static str,
        value: i64,
    },
    OptionalInteger {
        kind: CommandKind,
        canonical_name: &'static str,
        value: Option<i64>,
    },
    Escape {
        defer_expression: Option<Vec<String>>,
    },
    Extrude {
        step_days: Option<u16>,
    },
    ClearOrGather {
        kind: CommandKind,
        canonical_name: &'static str,
        values: Vec<String>,
    },
    Finish {
        values: Vec<String>,
    },
    FocusMode {
        kind: CommandKind,
        canonical_name: &'static str,
        recent_days: Option<i64>,
    },
}

impl Command {
    pub(super) fn kind(&self) -> CommandKind {
        match self {
            Self::Noop => CommandKind::Noop,
            Self::Estimate { .. } => CommandKind::Estimate,
            Self::Focus { .. } => CommandKind::Focus,
            Self::Arrange { .. } => CommandKind::Arrange,
            Self::TuckAway => CommandKind::TuckAway,
            Self::Defer { .. } => CommandKind::Defer,
            Self::ShowAll { .. } => CommandKind::ShowAll,
            Self::InteractiveShortcut(InteractiveShortcut::DeferRoutine) => {
                CommandKind::DeferRoutines
            }
            Self::InteractiveShortcut(_) => CommandKind::Defer,
            Self::Action(action) => action.kind(),
        }
    }
}

impl CommandAction {
    fn kind(&self) -> CommandKind {
        match self {
            Self::NoArguments { kind, .. }
            | Self::NewProject { kind, .. }
            | Self::TimeExpression { kind, .. }
            | Self::OptionalPattern { kind, .. }
            | Self::TaskWithEstimate { kind, .. }
            | Self::StringValue { kind, .. }
            | Self::IntegerValue { kind, .. }
            | Self::OptionalInteger { kind, .. }
            | Self::ClearOrGather { kind, .. }
            | Self::FocusMode { kind, .. } => *kind,
            Self::Sequential { .. } => CommandKind::Sequential,
            Self::Repeat { .. } => CommandKind::Repeat,
            Self::Pick { .. } => CommandKind::Pick,
            Self::TaskNames { .. } => CommandKind::Breakdown,
            Self::Split { .. } => CommandKind::Split,
            Self::Escape { .. } => CommandKind::Escape,
            Self::Extrude { .. } => CommandKind::Extrude,
            Self::Finish { .. } => CommandKind::Finish,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CommandParseError {
    command: &'static str,
    field: &'static str,
    reason: &'static str,
    usage: &'static str,
}

impl CommandParseError {
    pub(super) fn new(
        command: &'static str,
        field: &'static str,
        reason: &'static str,
        usage: &'static str,
    ) -> Self {
        Self {
            command,
            field,
            reason,
            usage,
        }
    }

    #[cfg(test)]
    pub(super) fn command(&self) -> &'static str {
        self.command
    }

    #[cfg(test)]
    pub(super) fn field(&self) -> &'static str {
        self.field
    }

    #[cfg(test)]
    pub(super) fn reason(&self) -> &'static str {
        self.reason
    }

    #[cfg(test)]
    pub(super) fn usage(&self) -> &'static str {
        self.usage
    }
}

impl std::fmt::Display for CommandParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "入力エラー: {}: {} (コマンド: {}, 使い方: {})",
            self.field, self.reason, self.command, self.usage
        )
    }
}

impl std::error::Error for CommandParseError {}

#[derive(Debug)]
pub(super) enum CommandValidationError {
    Parse(CommandParseError),
    Application(ApplicationError),
}

impl From<ApplicationError> for CommandValidationError {
    fn from(error: ApplicationError) -> Self {
        Self::Application(error)
    }
}

pub(super) fn parse_project_category_input(
    value: &str,
) -> Result<Option<ProjectCategory>, CommandParseError> {
    match value.to_lowercase().as_str() {
        "_" | "none" | "clear" => Ok(None),
        _ => read_project_category(value).map(Some).ok_or_else(|| {
            CommandParseError::new("類", "category", "カテゴリが不正です", "類 <カテゴリ>")
        }),
    }
}

pub(super) fn validate_command_input(command: &Command) -> Result<(), CommandValidationError> {
    match command {
        Command::Estimate { minutes } => {
            estimated_work_seconds_from_minutes(*minutes)?;
            Ok(())
        }
        Command::Action(CommandAction::StringValue {
            kind: CommandKind::Category,
            value,
            ..
        }) => {
            parse_project_category_input(value).map_err(CommandValidationError::Parse)?;
            Ok(())
        }
        Command::Action(CommandAction::StringValue {
            kind: CommandKind::Deadline,
            value,
            ..
        }) => {
            if value.starts_with('今')
                || value.starts_with('明')
                || matches!(
                    value.as_str(),
                    "消" | "月" | "火" | "水" | "木" | "金" | "土" | "日"
                )
                || Regex::new(r"^\d{1,2}/\d{1,2}$")
                    .expect("valid regex")
                    .is_match(value)
                || Regex::new(r"^\d{1,2}:\d{1,2}$")
                    .expect("valid regex")
                    .is_match(value)
                || parse_local_datetime(&format!("{} 23:59:59", value), "%Y/%m/%d %H:%M:%S").is_ok()
            {
                Ok(())
            } else {
                Err(CommandValidationError::Parse(CommandParseError::new(
                    "〆",
                    "deadline",
                    "日時が不正です",
                    "〆 <日付または時刻>",
                )))
            }
        }
        _ => Ok(()),
    }
}

pub(super) fn parse_command(input: &str, mode: ParseMode) -> Result<Command, CommandParseError> {
    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() || normalized.starts_with('#') {
        return Ok(Command::Noop);
    }

    if mode == ParseMode::Interactive {
        let shortcut = match normalized.as_str() {
            "t" => Some(Command::TuckAway),
            "h" => Some(Command::Defer {
                amount: 1,
                unit: "時間".to_string(),
            }),
            "D" => Some(Command::Defer {
                amount: 86_400,
                unit: "秒".to_string(),
            }),
            "d" => Some(Command::InteractiveShortcut(
                InteractiveShortcut::NextMorning,
            )),
            "w" => Some(Command::InteractiveShortcut(InteractiveShortcut::NextWeek)),
            "W" => Some(Command::InteractiveShortcut(
                InteractiveShortcut::DeferRoutine,
            )),
            "y" => Some(Command::InteractiveShortcut(InteractiveShortcut::FiveYears)),
            _ => None,
        };
        if let Some(shortcut) = shortcut {
            return Ok(shortcut);
        }
    }

    let tokens = normalized.split(' ').collect::<Vec<_>>();
    let name = tokens[0];
    let arguments = tokens[1..]
        .iter()
        .map(|argument| (*argument).to_string())
        .collect::<Vec<_>>();

    let Some(definition) = command_definition(name) else {
        return if input.starts_with('0') {
            Ok(Command::Noop)
        } else {
            Ok(Command::ShowAll {
                pattern: Some(name.to_string()),
            })
        };
    };

    if matches!(
        definition.kind,
        CommandKind::FocusHighest | CommandKind::FocusLowest
    ) && mode == ParseMode::NonInteractive
    {
        return Ok(Command::ShowAll {
            pattern: Some(name.to_string()),
        });
    }

    definition.validate_argument_count(&arguments)?;

    match definition.kind {
        CommandKind::TuckAway => parse_tuck_away(definition, mode),
        CommandKind::Estimate => parse_estimate(definition, &arguments),
        CommandKind::Focus => parse_focus(definition, &arguments),
        CommandKind::Arrange => parse_arrange(definition, &arguments),
        CommandKind::ShowAll => Ok(Command::ShowAll {
            pattern: arguments.first().cloned(),
        }),
        CommandKind::Defer if arguments.len() == 2 => Ok(Command::Defer {
            amount: parse_i64(
                &arguments[0],
                definition,
                "amount",
                "整数で指定してください",
            )?,
            unit: arguments[1].to_lowercase(),
        }),
        _ => parse_action(definition, &arguments),
    }
}

fn parse_tuck_away(
    definition: CommandDefinition,
    mode: ParseMode,
) -> Result<Command, CommandParseError> {
    if mode == ParseMode::NonInteractive {
        return Err(parse_error(
            definition.canonical_name,
            "mode",
            "対話モード専用です",
            definition.usage,
        ));
    }
    Ok(Command::TuckAway)
}

fn parse_estimate(
    definition: CommandDefinition,
    arguments: &[String],
) -> Result<Command, CommandParseError> {
    let value = required_argument(arguments, definition, "estimated_work_minutes")?;
    Ok(Command::Estimate {
        minutes: parse_i64(
            value,
            definition,
            "estimated_work_minutes",
            "整数で指定してください",
        )?,
    })
}

fn parse_focus(
    definition: CommandDefinition,
    arguments: &[String],
) -> Result<Command, CommandParseError> {
    let value = required_argument(arguments, definition, "task_id")?;
    Ok(Command::Focus {
        task_id: Uuid::parse_str(value).map_err(|_| {
            parse_error(
                definition.canonical_name,
                "task_id",
                "UUIDで指定してください",
                definition.usage,
            )
        })?,
    })
}

fn parse_arrange(
    definition: CommandDefinition,
    arguments: &[String],
) -> Result<Command, CommandParseError> {
    let value = required_argument(arguments, definition, "estimated_work_minutes")?;
    let minutes = parse_i64(
        value,
        definition,
        "estimated_work_minutes",
        "整数で指定してください",
    )?;
    let includes_zero_estimate = match arguments.get(1).map(String::as_str) {
        None => false,
        Some("全" | "all") => true,
        Some(_) => {
            return Err(parse_error(
                definition.canonical_name,
                "includes_zero_estimate",
                "全またはallで指定してください",
                definition.usage,
            ));
        }
    };
    Ok(Command::Arrange {
        minutes,
        includes_zero_estimate,
    })
}

fn parse_action(
    definition: CommandDefinition,
    arguments: &[String],
) -> Result<Command, CommandParseError> {
    let kind = definition.kind;
    let canonical_name = definition.canonical_name;
    let action = match kind {
        CommandKind::NewProject | CommandKind::HobbyProject | CommandKind::UnplannedProject => {
            let name = required_argument(arguments, definition, "task_name")?;
            CommandAction::NewProject {
                kind,
                canonical_name,
                name: name.to_string(),
                estimated_minutes: optional_i64(
                    arguments.get(1),
                    definition,
                    "estimated_work_minutes",
                )?,
            }
        }
        CommandKind::Sequential => {
            let [name, estimated_minutes, begin_index, end_index, suffix @ ..] = arguments else {
                unreachable!("sequential arity was validated before field parsing")
            };
            CommandAction::Sequential {
                name: name.clone(),
                estimated_minutes: parse_integer_field(
                    estimated_minutes,
                    definition,
                    "estimated_work_minutes",
                )?,
                begin_index: parse_integer_field(begin_index, definition, "begin_index")?,
                end_index: parse_integer_field(end_index, definition, "end_index")?,
                suffix: suffix.first().cloned(),
            }
        }
        CommandKind::Repeat => {
            let [name, estimated_minutes, day, start_time, deadline_time] = arguments else {
                unreachable!("repeat arity was validated before field parsing")
            };
            CommandAction::Repeat {
                name: name.clone(),
                estimated_minutes: parse_integer_field(
                    estimated_minutes,
                    definition,
                    "estimated_work_minutes",
                )?,
                day: day.clone(),
                start_time: start_time.clone(),
                deadline_time: deadline_time.clone(),
            }
        }
        CommandKind::Appointment | CommandKind::Start | CommandKind::Defer => {
            CommandAction::TimeExpression {
                kind,
                canonical_name,
                values: arguments.to_vec(),
            }
        }
        CommandKind::Tail => CommandAction::OptionalPattern {
            kind,
            canonical_name,
            pattern: arguments.first().cloned(),
        },
        CommandKind::Pick => CommandAction::Pick {
            task_id: arguments
                .first()
                .map(|value| {
                    Uuid::parse_str(value).map_err(|_| {
                        parse_error(
                            canonical_name,
                            "task_id",
                            "UUIDで指定してください",
                            definition.usage,
                        )
                    })
                })
                .transpose()?,
        },
        CommandKind::NextUp => {
            let name = required_argument(arguments, definition, "task_name")?;
            CommandAction::TaskWithEstimate {
                kind,
                canonical_name,
                name: name.to_string(),
                estimated_minutes: optional_i64(
                    arguments.get(1),
                    definition,
                    "estimated_work_minutes",
                )?,
            }
        }
        CommandKind::Breakdown => CommandAction::TaskNames {
            names: arguments.to_vec(),
        },
        CommandKind::Split => {
            let [minutes, name] = arguments else {
                unreachable!("split arity was validated before field parsing")
            };
            CommandAction::Split {
                minutes: parse_integer_field(minutes, definition, "minutes")?,
                name: name.clone(),
            }
        }
        CommandKind::Deadline => CommandAction::StringValue {
            kind,
            canonical_name,
            value: required_argument(arguments, definition, "deadline")?.to_string(),
        },
        CommandKind::Category => CommandAction::StringValue {
            kind,
            canonical_name,
            value: required_argument(arguments, definition, "category")?.to_string(),
        },
        CommandKind::Actual | CommandKind::Priority => CommandAction::IntegerValue {
            kind,
            canonical_name,
            value: parse_integer_field(
                required_argument(arguments, definition, "value")?,
                definition,
                "value",
            )?,
        },
        CommandKind::Work => CommandAction::OptionalInteger {
            kind,
            canonical_name,
            value: optional_i64(arguments.first(), definition, "actual_work_minutes")?,
        },
        CommandKind::Escape => CommandAction::Escape {
            defer_expression: (!arguments.is_empty()).then(|| arguments.to_vec()),
        },
        CommandKind::Extrude => CommandAction::Extrude {
            step_days: arguments
                .first()
                .map(|value| {
                    value.parse::<u16>().map_err(|_| {
                        parse_error(
                            definition.canonical_name,
                            "step_days",
                            "0以上65535以下の整数で指定してください",
                            definition.usage,
                        )
                    })
                })
                .transpose()?,
        },
        CommandKind::Clear | CommandKind::Gather => CommandAction::ClearOrGather {
            kind,
            canonical_name,
            values: arguments.to_vec(),
        },
        CommandKind::Finish => CommandAction::Finish {
            values: arguments.to_vec(),
        },
        CommandKind::FocusHighest => CommandAction::FocusMode {
            kind,
            canonical_name,
            recent_days: None,
        },
        CommandKind::FocusLowest => {
            let recent_days = arguments
                .first()
                .map(|value| {
                    if !value.chars().all(|character| character.is_ascii_digit()) {
                        return Err(parse_error(
                            canonical_name,
                            "recent_days",
                            "0以上の整数で指定してください",
                            definition.usage,
                        ));
                    }
                    parse_i64(
                        value,
                        definition,
                        "recent_days",
                        "0以上の整数で指定してください",
                    )
                })
                .transpose()?;
            CommandAction::FocusMode {
                kind,
                canonical_name,
                recent_days,
            }
        }
        CommandKind::Tree
        | CommandKind::Ancestor
        | CommandKind::Root
        | CommandKind::Leaves
        | CommandKind::Today
        | CommandKind::NonRepetitive
        | CommandKind::Calendar
        | CommandKind::Band
        | CommandKind::Open
        | CommandKind::Obsidian
        | CommandKind::Unfocus
        | CommandKind::Parent
        | CommandKind::Children
        | CommandKind::Deepest
        | CommandKind::Wait
        | CommandKind::DeferRoutines
        | CommandKind::Flatten
        | CommandKind::Pack
        | CommandKind::Verify => CommandAction::NoArguments {
            kind,
            canonical_name,
        },
        CommandKind::Noop
        | CommandKind::ShowAll
        | CommandKind::Focus
        | CommandKind::Estimate
        | CommandKind::Arrange
        | CommandKind::TuckAway => unreachable!("handled before action parsing"),
    };
    Ok(Command::Action(action))
}

fn required_argument<'a>(
    arguments: &'a [String],
    definition: CommandDefinition,
    field: &'static str,
) -> Result<&'a str, CommandParseError> {
    arguments.first().map(String::as_str).ok_or_else(|| {
        parse_error(
            definition.canonical_name,
            field,
            "値が必要です",
            definition.usage,
        )
    })
}

fn optional_i64(
    value: Option<&String>,
    definition: CommandDefinition,
    field: &'static str,
) -> Result<Option<i64>, CommandParseError> {
    value
        .map(|value| parse_integer_field(value, definition, field))
        .transpose()
}

fn parse_integer_field(
    value: &str,
    definition: CommandDefinition,
    field: &'static str,
) -> Result<i64, CommandParseError> {
    parse_i64(value, definition, field, "整数で指定してください")
}

fn parse_i64(
    value: &str,
    definition: CommandDefinition,
    field: &'static str,
    reason: &'static str,
) -> Result<i64, CommandParseError> {
    value
        .parse()
        .map_err(|_| parse_error(definition.canonical_name, field, reason, definition.usage))
}

fn parse_error(
    command: &'static str,
    field: &'static str,
    reason: &'static str,
    usage: &'static str,
) -> CommandParseError {
    CommandParseError {
        command,
        field,
        reason,
        usage,
    }
}

fn command_definition(name: &str) -> Option<CommandDefinition> {
    use CommandKind as Kind;

    let definition = match name {
        "伏" | "tuck" | "t" => CommandDefinition::new(Kind::TuckAway, "tuck", "tuck", 0, Some(0)),
        "新" | "new" => {
            CommandDefinition::new(Kind::NewProject, "新", "新 <name> [minutes]", 1, Some(2))
        }
        "遊" | "hobby" => {
            CommandDefinition::new(Kind::HobbyProject, "遊", "遊 <name> [minutes]", 1, Some(2))
        }
        "突" | "unplanned" => CommandDefinition::new(
            Kind::UnplannedProject,
            "突",
            "突 <name> [minutes]",
            1,
            Some(2),
        ),
        "連" | "sequential" | "seq" => CommandDefinition::new(
            Kind::Sequential,
            "連",
            "連 <name> <minutes> <begin> <end> [suffix]",
            4,
            Some(5),
        ),
        "繰" | "repeat" => CommandDefinition::new(
            Kind::Repeat,
            "繰",
            "繰 <name> <minutes> <day> <start> <deadline>",
            5,
            Some(5),
        ),
        "約" | "appointment" => {
            CommandDefinition::new(Kind::Appointment, "約", "約 <日付> [時刻]", 1, Some(2))
        }
        "始" | "start" => {
            CommandDefinition::new(Kind::Start, "始", "始 <日付> [時刻]", 1, Some(2))
        }
        "樹" | "tree" => CommandDefinition::new(Kind::Tree, "樹", "樹", 0, Some(0)),
        "条" | "祖" | "ancestor" | "anc" => {
            CommandDefinition::new(Kind::Ancestor, "条", "条", 0, Some(0))
        }
        "根" | "root" => CommandDefinition::new(Kind::Root, "根", "根", 0, Some(0)),
        "葉" | "leaves" | "leaf" | "lf" => {
            CommandDefinition::new(Kind::Leaves, "葉", "葉", 0, Some(0))
        }
        "全" | "all" => CommandDefinition::new(Kind::ShowAll, "全", "全 [pattern]", 0, Some(1)),
        "尾" => CommandDefinition::new(Kind::Tail, "尾", "尾 [pattern]", 0, Some(1)),
        "今" | "today" => CommandDefinition::new(Kind::Today, "今", "今", 0, Some(0)),
        "単" | "non_repetitive" => {
            CommandDefinition::new(Kind::NonRepetitive, "単", "単", 0, Some(0))
        }
        "暦" | "cal" => CommandDefinition::new(Kind::Calendar, "暦", "暦", 0, Some(0)),
        "帯" | "band" => CommandDefinition::new(Kind::Band, "帯", "帯", 0, Some(0)),
        "見" | "focus" | "fc" => {
            CommandDefinition::new(Kind::Focus, "見", "見 <task_id>", 1, Some(1))
        }
        "選" | "pick" => CommandDefinition::new(Kind::Pick, "選", "選 [task_id]", 0, Some(1)),
        "開" | "open" | "op" => CommandDefinition::new(Kind::Open, "開", "開", 0, Some(0)),
        "黒" | "obs" => CommandDefinition::new(Kind::Obsidian, "黒", "黒", 0, Some(0)),
        "外" | "unfocus" | "ufc" => CommandDefinition::new(Kind::Unfocus, "外", "外", 0, Some(0)),
        "親" | "parent" => CommandDefinition::new(Kind::Parent, "親", "親", 0, Some(0)),
        "子" | "children" | "ch" => CommandDefinition::new(Kind::Children, "子", "子", 0, Some(0)),
        "深" | "deep" | "deepest" => CommandDefinition::new(Kind::Deepest, "深", "深", 0, Some(0)),
        "上" | "nextup" | "nu" => {
            CommandDefinition::new(Kind::NextUp, "上", "上 <name> [minutes]", 1, Some(2))
        }
        "下" | "breakdown" | "bd" => {
            CommandDefinition::new(Kind::Breakdown, "下", "下 <name>...", 1, None)
        }
        "割" | "split" | "sp" => {
            CommandDefinition::new(Kind::Split, "割", "割 <minutes> <name>", 2, Some(2))
        }
        "待" | "wait" => CommandDefinition::new(Kind::Wait, "待", "待", 0, Some(0)),
        "〆" | "締" | "deadline" => {
            CommandDefinition::new(Kind::Deadline, "〆", "〆 <日付または時刻>", 1, Some(1))
        }
        "予" | "estimate" | "es" => {
            CommandDefinition::new(Kind::Estimate, "予", "予 <分>", 1, Some(1))
        }
        "揃" | "arrange" | "arr" => {
            CommandDefinition::new(Kind::Arrange, "揃", "揃 <分> [全]", 1, Some(2))
        }
        "実" | "actual" | "ac" => {
            CommandDefinition::new(Kind::Actual, "実", "実 <integer>", 1, Some(1))
        }
        "重" | "priority" | "pr" => {
            CommandDefinition::new(Kind::Priority, "重", "重 <integer>", 1, Some(1))
        }
        "類" | "category" | "cat" => {
            CommandDefinition::new(Kind::Category, "類", "類 <カテゴリ>", 1, Some(1))
        }
        "働" | "work" | "wk" => {
            CommandDefinition::new(Kind::Work, "働", "働 [minutes]", 0, Some(1))
        }
        "後" | "defer" => CommandDefinition::new(Kind::Defer, "後", "後 <量> [単位]", 1, Some(2)),
        "清" | "defer_all_frequent_routines" => {
            CommandDefinition::new(Kind::DeferRoutines, "清", "清", 0, Some(0))
        }
        "逃" | "escape" | "esc" => {
            CommandDefinition::new(Kind::Escape, "逃", "逃 [量] [単位]", 0, Some(2))
        }
        "平" | "flatten" | "flat" => CommandDefinition::new(Kind::Flatten, "平", "平", 0, Some(0)),
        "詰" | "pack" => CommandDefinition::new(Kind::Pack, "詰", "詰", 0, Some(0)),
        "押" | "extrude" => CommandDefinition::new(Kind::Extrude, "押", "押 [days]", 0, Some(1)),
        "空" | "clear" => {
            CommandDefinition::new(Kind::Clear, "空", "空 <日付> [時刻]", 1, Some(2))
        }
        "集" | "gather" => {
            CommandDefinition::new(Kind::Gather, "集", "集 <日付> [時刻]", 1, Some(2))
        }
        "終" | "finish" | "fin" => {
            CommandDefinition::new(Kind::Finish, "終", "終 [日付] [時刻]", 0, Some(2))
        }
        "高" | "high" | "hi" | "highest" => {
            CommandDefinition::new(Kind::FocusHighest, "高", "高", 0, Some(0))
        }
        "低" | "low" | "lo" | "lowest" => {
            CommandDefinition::new(Kind::FocusLowest, "低", "低 [days]", 0, Some(1))
        }
        "検証" => CommandDefinition::new(Kind::Verify, "検証", "検証", 0, Some(0)),
        _ => return None,
    };
    Some(definition)
}

#[cfg(test)]
pub(super) fn command_with_minimum_valid_arguments(command: &str) -> String {
    let arguments = match command {
        "新" | "new" | "遊" | "hobby" | "突" | "unplanned" => " project 15",
        "連" | "sequential" | "seq" => " task 15 1 2",
        "繰" | "repeat" => " task 15 月 09:00 10:00",
        "約" | "appointment" | "始" | "start" => " 今",
        "見" | "focus" | "fc" | "選" | "pick" => " 00000000-0000-0000-0000-000000000001",
        "上" | "nextup" | "nu" | "下" | "breakdown" | "bd" => " task 15",
        "割" | "split" | "sp" => " 15 child",
        "〆" | "締" | "deadline" => " 今",
        "予" | "estimate" | "es" | "揃" | "arrange" | "arr" | "実" | "actual" | "ac" | "重"
        | "priority" | "pr" | "働" | "work" | "wk" | "押" | "extrude" => " 15",
        "類" | "category" | "cat" => " 資",
        "後" | "defer" | "逃" | "escape" | "esc" => " 1 秒",
        "空" | "clear" | "集" | "gather" => " 明",
        "終" | "finish" | "fin" => " 今",
        _ => "",
    };
    format!("{command}{arguments}")
}

#[cfg(test)]
pub(super) fn representative_valid_commands() -> Vec<Command> {
    let names = [
        "新", "遊", "突", "連", "繰", "約", "始", "樹", "条", "根", "葉", "全", "尾", "今", "単",
        "暦", "帯", "見", "選", "開", "黒", "外", "親", "子", "深", "上", "下", "割", "待", "〆",
        "予", "揃", "実", "重", "類", "働", "後", "清", "逃", "平", "詰", "押", "空", "集", "終",
        "高", "低", "検証",
    ];
    let mut commands = vec![Command::Noop];
    commands.extend(names.into_iter().map(|name| {
        let mode = if matches!(name, "高" | "低") {
            ParseMode::Interactive
        } else {
            ParseMode::NonInteractive
        };
        parse_command(&command_with_minimum_valid_arguments(name), mode)
            .expect("representative command must parse")
    }));
    commands.extend(["t", "d", "w", "W", "y"].map(|shortcut| {
        parse_command(shortcut, ParseMode::Interactive)
            .expect("representative interactive shortcut must parse")
    }));
    commands
}
