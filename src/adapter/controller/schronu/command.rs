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
        task_id: Uuid,
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

    pub(super) fn command(&self) -> &'static str {
        self.command
    }

    pub(super) fn field(&self) -> &'static str {
        self.field
    }

    pub(super) fn reason(&self) -> &'static str {
        self.reason
    }

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

    match name {
        "tuck" | "伏" | "t" => parse_tuck_away(&arguments, mode),
        "予" | "estimate" | "es" => parse_estimate(&arguments),
        "見" | "focus" | "fc" => parse_focus(&arguments),
        "揃" | "arrange" | "arr" => parse_arrange(&arguments),
        "全" | "all" => Ok(Command::ShowAll {
            pattern: arguments.first().cloned(),
        }),
        "後" | "defer" if arguments.len() == 2 => Ok(Command::Defer {
            amount: parse_i64(
                &arguments[0],
                "後",
                "amount",
                "整数で指定してください",
                "後 <量> <単位>",
            )?,
            unit: arguments[1].to_lowercase(),
        }),
        _ => match known_command(name) {
            Some((CommandKind::FocusHighest | CommandKind::FocusLowest, _))
                if mode == ParseMode::NonInteractive =>
            {
                Ok(Command::ShowAll {
                    pattern: Some(name.to_string()),
                })
            }
            Some((kind, canonical_name)) => parse_action(kind, canonical_name, &arguments),
            None if input.starts_with('0') => Ok(Command::Noop),
            None => Ok(Command::ShowAll {
                pattern: Some(name.to_string()),
            }),
        },
    }
}

fn parse_tuck_away(arguments: &[String], mode: ParseMode) -> Result<Command, CommandParseError> {
    require_count(arguments, 0, 0, "tuck", "tuck")?;
    if mode == ParseMode::NonInteractive {
        return Err(parse_error("tuck", "mode", "対話モード専用です", "tuck"));
    }
    Ok(Command::TuckAway)
}

fn parse_estimate(arguments: &[String]) -> Result<Command, CommandParseError> {
    let value = required_argument(arguments, "予", "estimated_work_minutes", "予 <分>")?;
    Ok(Command::Estimate {
        minutes: parse_i64(
            value,
            "予",
            "estimated_work_minutes",
            "整数で指定してください",
            "予 <分>",
        )?,
    })
}

fn parse_focus(arguments: &[String]) -> Result<Command, CommandParseError> {
    let value = required_argument(arguments, "見", "task_id", "見 <task_id>")?;
    Ok(Command::Focus {
        task_id: Uuid::parse_str(value)
            .map_err(|_| parse_error("見", "task_id", "UUIDで指定してください", "見 <task_id>"))?,
    })
}

fn parse_arrange(arguments: &[String]) -> Result<Command, CommandParseError> {
    let value = required_argument(arguments, "揃", "estimated_work_minutes", "揃 <分> [全]")?;
    Ok(Command::Arrange {
        minutes: parse_i64(
            value,
            "揃",
            "estimated_work_minutes",
            "整数で指定してください",
            "揃 <分> [全]",
        )?,
        includes_zero_estimate: arguments
            .get(1)
            .is_some_and(|argument| matches!(argument.as_str(), "全" | "all")),
    })
}

fn parse_action(
    kind: CommandKind,
    canonical_name: &'static str,
    arguments: &[String],
) -> Result<Command, CommandParseError> {
    let action = match kind {
        CommandKind::NewProject | CommandKind::HobbyProject | CommandKind::UnplannedProject => {
            let name = required_argument(
                arguments,
                canonical_name,
                "task_name",
                "<command> <name> [minutes]",
            )?;
            CommandAction::NewProject {
                kind,
                canonical_name,
                name: name.to_string(),
                estimated_minutes: optional_i64(
                    arguments.get(1),
                    canonical_name,
                    "estimated_work_minutes",
                    "<command> <name> [minutes]",
                )?,
            }
        }
        CommandKind::Sequential => {
            if arguments.len() < 4 {
                return Err(parse_error(
                    "連",
                    "arguments",
                    "名前、見積、開始番号、終了番号が必要です",
                    "連 <name> <minutes> <begin> <end> [suffix]",
                ));
            }
            CommandAction::Sequential {
                name: arguments[0].clone(),
                estimated_minutes: parse_integer_field(
                    &arguments[1],
                    "連",
                    "estimated_work_minutes",
                    "連 <name> <minutes> <begin> <end> [suffix]",
                )?,
                begin_index: parse_integer_field(
                    &arguments[2],
                    "連",
                    "begin_index",
                    "連 <name> <minutes> <begin> <end> [suffix]",
                )?,
                end_index: parse_integer_field(
                    &arguments[3],
                    "連",
                    "end_index",
                    "連 <name> <minutes> <begin> <end> [suffix]",
                )?,
                suffix: arguments.get(4).cloned(),
            }
        }
        CommandKind::Repeat => {
            if arguments.len() != 5 {
                return Err(parse_error(
                    "繰",
                    "arguments",
                    "名前、見積、曜日、開始時刻、締切時刻が必要です",
                    "繰 <name> <minutes> <day> <start> <deadline>",
                ));
            }
            CommandAction::Repeat {
                name: arguments[0].clone(),
                estimated_minutes: parse_integer_field(
                    &arguments[1],
                    "繰",
                    "estimated_work_minutes",
                    "繰 <name> <minutes> <day> <start> <deadline>",
                )?,
                day: arguments[2].clone(),
                start_time: arguments[3].clone(),
                deadline_time: arguments[4].clone(),
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
        CommandKind::Pick => {
            let value = required_argument(arguments, "選", "task_id", "選 <task_id>")?;
            CommandAction::Pick {
                task_id: Uuid::parse_str(value).map_err(|_| {
                    parse_error("選", "task_id", "UUIDで指定してください", "選 <task_id>")
                })?,
            }
        }
        CommandKind::NextUp => {
            let name = required_argument(arguments, "上", "task_name", "上 <name> [minutes]")?;
            CommandAction::TaskWithEstimate {
                kind,
                canonical_name,
                name: name.to_string(),
                estimated_minutes: optional_i64(
                    arguments.get(1),
                    "上",
                    "estimated_work_minutes",
                    "上 <name> [minutes]",
                )?,
            }
        }
        CommandKind::Breakdown => CommandAction::TaskNames {
            names: arguments.to_vec(),
        },
        CommandKind::Split => {
            if arguments.len() != 2 {
                return Err(parse_error(
                    "割",
                    "arguments",
                    "分数とtask名が必要です",
                    "割 <minutes> <name>",
                ));
            }
            CommandAction::Split {
                minutes: parse_integer_field(
                    &arguments[0],
                    "割",
                    "minutes",
                    "割 <minutes> <name>",
                )?,
                name: arguments[1].clone(),
            }
        }
        CommandKind::Deadline => CommandAction::StringValue {
            kind,
            canonical_name,
            value: required_argument(arguments, canonical_name, "deadline", "〆 <日付または時刻>")?
                .to_string(),
        },
        CommandKind::Category => CommandAction::StringValue {
            kind,
            canonical_name,
            value: required_argument(arguments, canonical_name, "category", "類 <カテゴリ>")?
                .to_string(),
        },
        CommandKind::Actual | CommandKind::Priority => CommandAction::IntegerValue {
            kind,
            canonical_name,
            value: parse_integer_field(
                required_argument(arguments, canonical_name, "value", "<command> <integer>")?,
                canonical_name,
                "value",
                "<command> <integer>",
            )?,
        },
        CommandKind::Work => CommandAction::OptionalInteger {
            kind,
            canonical_name,
            value: optional_i64(
                arguments.first(),
                "働",
                "actual_work_minutes",
                "働 [minutes]",
            )?,
        },
        CommandKind::Escape => CommandAction::Escape {
            defer_expression: (!arguments.is_empty()).then(|| arguments.to_vec()),
        },
        CommandKind::Extrude => CommandAction::Extrude {
            step_days: arguments
                .first()
                .map(|value| value.parse::<u16>().unwrap_or(1)),
        },
        CommandKind::Clear | CommandKind::Gather => CommandAction::ClearOrGather {
            kind,
            canonical_name,
            values: arguments.to_vec(),
        },
        CommandKind::Finish => CommandAction::Finish {
            values: arguments.to_vec(),
        },
        CommandKind::FocusHighest => {
            require_count(arguments, 0, 0, canonical_name, "高")?;
            CommandAction::FocusMode {
                kind,
                canonical_name,
                recent_days: None,
            }
        }
        CommandKind::FocusLowest => {
            require_count(arguments, 0, 1, canonical_name, "低 [days]")?;
            let recent_days = arguments
                .first()
                .map(|value| {
                    if !value.chars().all(|character| character.is_ascii_digit()) {
                        return Err(parse_error(
                            canonical_name,
                            "recent_days",
                            "0以上の整数で指定してください",
                            "低 [days]",
                        ));
                    }
                    parse_i64(
                        value,
                        canonical_name,
                        "recent_days",
                        "0以上の整数で指定してください",
                        "低 [days]",
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
    command: &'static str,
    field: &'static str,
    usage: &'static str,
) -> Result<&'a str, CommandParseError> {
    arguments
        .first()
        .map(String::as_str)
        .ok_or_else(|| parse_error(command, field, "値が必要です", usage))
}

fn optional_i64(
    value: Option<&String>,
    command: &'static str,
    field: &'static str,
    usage: &'static str,
) -> Result<Option<i64>, CommandParseError> {
    value
        .map(|value| parse_integer_field(value, command, field, usage))
        .transpose()
}

fn parse_integer_field(
    value: &str,
    command: &'static str,
    field: &'static str,
    usage: &'static str,
) -> Result<i64, CommandParseError> {
    parse_i64(value, command, field, "整数で指定してください", usage)
}

fn parse_i64(
    value: &str,
    command: &'static str,
    field: &'static str,
    reason: &'static str,
    usage: &'static str,
) -> Result<i64, CommandParseError> {
    value
        .parse()
        .map_err(|_| parse_error(command, field, reason, usage))
}

fn require_count(
    arguments: &[String],
    minimum: usize,
    maximum: usize,
    command: &'static str,
    usage: &'static str,
) -> Result<(), CommandParseError> {
    if (minimum..=maximum).contains(&arguments.len()) {
        Ok(())
    } else {
        Err(parse_error(
            command,
            "arguments",
            "引数の個数が正しくありません",
            usage,
        ))
    }
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

fn known_command(name: &str) -> Option<(CommandKind, &'static str)> {
    let command = match name {
        "新" | "new" => (CommandKind::NewProject, "新"),
        "遊" | "hobby" => (CommandKind::HobbyProject, "遊"),
        "突" | "unplanned" => (CommandKind::UnplannedProject, "突"),
        "連" | "sequential" | "seq" => (CommandKind::Sequential, "連"),
        "繰" | "repeat" => (CommandKind::Repeat, "繰"),
        "約" | "appointment" => (CommandKind::Appointment, "約"),
        "始" | "start" => (CommandKind::Start, "始"),
        "樹" | "tree" => (CommandKind::Tree, "樹"),
        "条" | "祖" | "ancestor" | "anc" => (CommandKind::Ancestor, "条"),
        "根" | "root" => (CommandKind::Root, "根"),
        "葉" | "leaves" | "leaf" | "lf" => (CommandKind::Leaves, "葉"),
        "尾" => (CommandKind::Tail, "尾"),
        "今" | "today" => (CommandKind::Today, "今"),
        "単" | "non_repetitive" => (CommandKind::NonRepetitive, "単"),
        "暦" | "cal" => (CommandKind::Calendar, "暦"),
        "帯" | "band" => (CommandKind::Band, "帯"),
        "選" | "pick" => (CommandKind::Pick, "選"),
        "開" | "open" | "op" => (CommandKind::Open, "開"),
        "黒" | "obs" => (CommandKind::Obsidian, "黒"),
        "外" | "unfocus" | "ufc" => (CommandKind::Unfocus, "外"),
        "親" | "parent" => (CommandKind::Parent, "親"),
        "子" | "children" | "ch" => (CommandKind::Children, "子"),
        "深" | "deep" | "deepest" => (CommandKind::Deepest, "深"),
        "上" | "nextup" | "nu" => (CommandKind::NextUp, "上"),
        "下" | "breakdown" | "bd" => (CommandKind::Breakdown, "下"),
        "割" | "split" | "sp" => (CommandKind::Split, "割"),
        "待" | "wait" => (CommandKind::Wait, "待"),
        "〆" | "締" | "deadline" => (CommandKind::Deadline, "〆"),
        "実" | "actual" | "ac" => (CommandKind::Actual, "実"),
        "重" | "priority" | "pr" => (CommandKind::Priority, "重"),
        "類" | "category" | "cat" => (CommandKind::Category, "類"),
        "働" | "work" | "wk" => (CommandKind::Work, "働"),
        "後" | "defer" => (CommandKind::Defer, "後"),
        "清" | "defer_all_frequent_routines" => (CommandKind::DeferRoutines, "清"),
        "逃" | "escape" | "esc" => (CommandKind::Escape, "逃"),
        "平" | "flatten" | "flat" => (CommandKind::Flatten, "平"),
        "詰" | "pack" => (CommandKind::Pack, "詰"),
        "押" | "extrude" => (CommandKind::Extrude, "押"),
        "空" | "clear" => (CommandKind::Clear, "空"),
        "集" | "gather" => (CommandKind::Gather, "集"),
        "終" | "finish" | "fin" => (CommandKind::Finish, "終"),
        "高" | "high" | "hi" | "highest" => (CommandKind::FocusHighest, "高"),
        "低" | "low" | "lo" | "lowest" => (CommandKind::FocusLowest, "低"),
        "検証" => (CommandKind::Verify, "検証"),
        _ => return None,
    };
    Some(command)
}
