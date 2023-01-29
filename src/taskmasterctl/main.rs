extern crate rustyline;

use rustyline::{
	highlight::Highlighter,
	completion::Completer,
	hint::Hinter,
	validate::{self, Validator},
	line_buffer::LineBuffer, Helper
};

use std::borrow::Cow::{self, Owned};

/*
Commands:
- help

- global? status
- global? start
- global? stop
- global? restart

- list [all|program|processes|configs]
- load
- unload
- reload

- log
*/

// Completer + Hinter + Highlighter + Validator,

struct TaskMasterHelper {}

impl Completer for TaskMasterHelper {
	type Candidate = String;

	fn complete(
			&self, // FIXME should be `&mut self`
			line: &str,
			pos: usize,
			ctx: &rustyline::Context<'_>,
		) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
		let candidates = vec![
			"help".to_owned(),
			"global? status".to_owned(),
		];
		Ok((pos, candidates))
	}
	fn update(&self, line: &mut LineBuffer, start: usize, elected: &str) {
		let end = line.pos();
		line.replace(start..end, elected);
	}
}

impl Hinter for TaskMasterHelper {
	type Hint = String;

	fn hint(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
		Some("hint".to_owned())
	}
}

impl Highlighter for TaskMasterHelper {
	fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
		Owned("\x1b[95m".to_owned() + line + "\x1b[0m")
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

impl Validator for TaskMasterHelper {
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

impl Helper for TaskMasterHelper {}

fn main() {
	let helper = TaskMasterHelper { };
	let mut rl = rustyline::Editor::<TaskMasterHelper>::new().unwrap();
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
