mod highlighter;
use highlighter::{TaskmasterHighlighter};

use rustyline::{
	highlight::Highlighter,
	completion::{Completer, Candidate},
	hint::{Hinter, Hint},
	validate::{self, Validator},
	line_buffer::LineBuffer, Helper, config::Configurer
};

use std::borrow::Cow::{self, Owned};

#[derive(Debug, PartialEq, Eq)]
struct CommandHint {
	display: String,
	replace: String,
}

impl CommandHint {
	fn new(text: &str) -> CommandHint {
		CommandHint {
			display: text.into(),
			replace: text.into(),
		}
	}

	fn suffix(&self, strip_chars: usize) -> CommandHint {
		CommandHint {
			display: self.display.clone(),
			replace: self.display[strip_chars..].to_owned(),
		}
	}
}

impl Hint for CommandHint {
	fn display(&self) -> &str {
		&self.replace
	}

	fn completion(&self) -> Option<&str> {
		Some(&self.replace)
	}
}
impl Candidate for CommandHint {
	fn display(&self) -> &str {
		&self.display
	}

	fn replacement(&self) -> &str {
		&self.replace
	}
}

struct TaskmasterHelper {
	hints: Vec<CommandHint>,
	highlighter: TaskmasterHighlighter,
}

impl Completer for TaskmasterHelper {
	type Candidate = CommandHint;

	fn complete(
			&self, // FIXME should be `&mut self`
			line: &str,
			pos: usize,
			ctx: &rustyline::Context<'_>,
		) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
		let candidates: Vec<CommandHint> = self.hints
			.iter()
			.filter_map(|hint| {
				// expect hint after word complete, like redis cli, add condition:
				// line.ends_with(" ")
				if hint.display.starts_with(line) {
					Some(hint.suffix(pos))
				} else {
					None
				}
			})
			.collect();
		Ok((pos, candidates))
	}
	fn update(&self, line: &mut LineBuffer, start: usize, elected: &str) {
		let end = line.pos();
		line.replace(start..end, elected);
	}
}

impl Hinter for TaskmasterHelper {
	type Hint = CommandHint;

	fn hint(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> Option<CommandHint> {
		if line.is_empty() || pos < line.len() {
			return None;
		}

		self.hints
			.iter()
			.filter_map(|hint| {
				// expect hint after word complete, like redis cli, add condition:
				// line.ends_with(" ")
				if hint.display.starts_with(line) {
					Some(hint.suffix(pos))
				} else {
					None
				}
			})
			.next()
	}
}

impl Highlighter for TaskmasterHelper {
	fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
		Owned(self.highlighter.highlight(line))
	}

	fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
			&'s self,
			prompt: &'p str,
			default: bool,
		) -> Cow<'b, str> {
		Owned("\x1b[1;94m".to_owned() + prompt + "\x1b[0m")
	}

	fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
		Owned("\x1b[90m".to_owned() + hint + "\x1b[0m")
	}

	fn highlight_char(&self, _line: &str, _pos: usize) -> bool {
		true
	}
}

impl Validator for TaskmasterHelper {
	fn validate(&self, ctx: &mut validate::ValidationContext) -> rustyline::Result<validate::ValidationResult> {
		use validate::ValidationResult::{Incomplete, Invalid, Valid};

		let input = ctx.input();
		let result = if !input.starts_with("SELECT") {
			Invalid(Some("\x1b[91m < Expect: SELECT stmt\x1b[0m".to_owned()))
		} else if !input.ends_with(';') {
			Incomplete
		} else {
			Valid(None)
		};
		Ok(result)
	}
}

impl Helper for TaskmasterHelper {}

fn get_taskmaster_hints() -> Vec<CommandHint> {
	let mut set = Vec::new();

	set.push(CommandHint::new("help"));

	set.push(CommandHint::new("global [status|start|stop|restart]"));

	set.push(CommandHint::new("status <name>"));
	set.push(CommandHint::new("start <name>"));
	set.push(CommandHint::new("stop <name>"));
	set.push(CommandHint::new("restart <name>"));

	set.push(CommandHint::new("list [all|running|stopped|configs]"));

	set.push(CommandHint::new("load <config>"));
	set.push(CommandHint::new("unload <config>"));
	set.push(CommandHint::new("reload <config>"));

	set.push(CommandHint::new("logs <name>"));

	set
}

fn main() {
	let helper = TaskmasterHelper {
		hints:       get_taskmaster_hints(),
		highlighter: TaskmasterHighlighter::new(),
	};
	let mut rl = rustyline::Editor::<TaskmasterHelper>::new().unwrap();

	rl.set_completion_type(rustyline::CompletionType::List);
	rl.set_helper(Some(helper));

	loop {
		let readline = rl.readline("> ");

		match readline {
			Ok(line) => {
				rl.add_history_entry(line.as_str());
				println!("Line: {}", line);
			},
			Err(err) => {
				println!("Error: {:?}", err);
				break
			}
		}
	}
}
