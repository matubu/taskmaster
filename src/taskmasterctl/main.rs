extern crate taskmastersocket;
use taskmastersocket::{TaskmasterDaemonRequest, TaskmasterDaemonResult};

mod highlighter;
use highlighter::{TaskmasterHighlighter};

use rustyline::{
	highlight::Highlighter,
	completion::{Completer, FilenameCompleter, self},
	hint::{Hinter},
	validate::{self, Validator},
	line_buffer::LineBuffer, Helper, config::Configurer
};

use std::borrow::Cow::{self, Owned};

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

struct TaskmasterHelper {
	highlighter: TaskmasterHighlighter,
	completion: FilenameCompleter
}

impl Completer for TaskmasterHelper {
	type Candidate = completion::Pair;

	fn complete(
			&self,
			line: &str,
			pos: usize,
			ctx: &rustyline::Context<'_>,
		) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
		self.completion.complete(line, pos, ctx)
	}
	fn update(&self, line: &mut LineBuffer, start: usize, elected: &str) {
		let end = line.pos();
		line.replace(start..end, elected);
	}
}

impl Hinter for TaskmasterHelper {
	type Hint = String;

	fn hint(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
		None
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
		use validate::ValidationResult::{Valid};

		Ok(Valid(None))
	}
}

impl Helper for TaskmasterHelper {}

fn main() {
	let helper = TaskmasterHelper {
		highlighter: TaskmasterHighlighter::new(),
		completion:  FilenameCompleter::new()
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

				let request = match line.as_str() {
					"status" => TaskmasterDaemonRequest::Status,
					"reload" => TaskmasterDaemonRequest::Reload,
					"restart" => TaskmasterDaemonRequest::Restart,
					_ => {
						let parts: Vec<&str> = line.split_whitespace().collect();
						if parts.len() < 2 {
							println!("Invalid command");
							continue;
						}
						match parts[0] {
							"start" => TaskmasterDaemonRequest::StartProgram(parts[1].to_owned()),
							"stop" => TaskmasterDaemonRequest::StopProgram(parts[1].to_owned()),
							"restart" => TaskmasterDaemonRequest::RestartProgram(parts[1].to_owned()),
							"load" => TaskmasterDaemonRequest::LoadFile(parts[1].to_owned()),
							"unload" => TaskmasterDaemonRequest::UnloadFile(parts[1].to_owned()),
							"reload" => TaskmasterDaemonRequest::ReloadFile(parts[1].to_owned()),
							_ => {
								println!("Invalid command");
								continue;
							}
						}
					}
				};
				
				// TODO: CHANGE THE PATH!!!
				let mut stream = UnixStream::connect("/path/to/socket").unwrap();
				bincode::serialize_into(&mut stream, &request).unwrap();
				stream.flush().unwrap();

				let response: TaskmasterDaemonResult = bincode::deserialize_from(&mut stream).unwrap();
				println!("Response: {:?}", response);
			},
			Err(err) => {
				println!("Error: {:?}", err);
				break
			}
		}
	}
}
