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

use std::{borrow::Cow::{self, Owned}, path::PathBuf, fs};

use std::io::{Write};
use std::os::unix::net::UnixStream;

enum Status {
	None,
	Success,
	Error,
}

struct TaskmasterHelper {
	highlighter: TaskmasterHighlighter,
	completion: FilenameCompleter,
	status: Status,
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
		match self.status {
			Status::None => Owned("\x1b[1;94m".to_owned() + prompt + "\x1b[0m"),
			Status::Success => Owned("\x1b[1;92m".to_owned() + prompt + "\x1b[0m"),
			Status::Error => Owned("\x1b[1;91m".to_owned() + prompt + "\x1b[0m"),
		}
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

fn resolve_path(path: &str) -> Result<String, &str> {
	let path = PathBuf::from(path);

	Ok(fs::canonicalize(&path).map_err(|_| "Could not resolve path")?
		.into_os_string().into_string().map_err(|_| "Could not resolve path")?
		.to_owned())
}

fn usage() {
	println!("Usage:");
	print!("{}", TaskmasterHighlighter::new().highlight(r#"
  status
  reload
  restart

  start <task-id>
  stop <task-id>
  restart <task-id>
  info <task-id>

  load <file>
  unload <file>
"#));
}

fn parse_line(line: &str) -> Result<TaskmasterDaemonRequest, &str> {
	Ok(match line {
		"status" => TaskmasterDaemonRequest::Status,
		"reload" => TaskmasterDaemonRequest::Reload,
		"restart" => TaskmasterDaemonRequest::Restart,
		_ => {
			let parts: Vec<&str> = line.split_whitespace().collect();
			if parts.len() < 2 {
				usage();
				return Err("Invalid command");
			}
			
			match parts[0] {
				"start" => TaskmasterDaemonRequest::StartTask(usize::from_str_radix(parts[1], 10).map_err(|_| "Argument should be an int")?),
				"stop" => TaskmasterDaemonRequest::StopTask(usize::from_str_radix(parts[1], 10).map_err(|_| "Argument should be an int")?),
				"restart" => TaskmasterDaemonRequest::RestartTask(usize::from_str_radix(parts[1], 10).map_err(|_| "Argument should be an int")?),
				"info" => TaskmasterDaemonRequest::InfoTask(usize::from_str_radix(parts[1], 10).map_err(|_| "Argument should be an int")?),
				"load" => TaskmasterDaemonRequest::LoadFile(resolve_path(parts[1])?),
				"unload" => TaskmasterDaemonRequest::UnloadFile(resolve_path(parts[1])?),
				_ => {
					usage();
					return Err("Invalid command");
				}
			}
		}
	})
}

fn main() {
	let mut stream = UnixStream::connect("/tmp/taskmasterd.sock")
		.expect("Could not connect to daemon");

	let helper = TaskmasterHelper {
		highlighter: TaskmasterHighlighter::new(),
		completion:  FilenameCompleter::new(),
		status: Status::None
	};
	let mut rl = rustyline::Editor::<TaskmasterHelper>::new().unwrap();

	rl.set_completion_type(rustyline::CompletionType::List);
	rl.set_helper(Some(helper));

	loop {
		let readline = rl.readline("$> ");

		match readline {
			Ok(line) => {
				rl.add_history_entry(line.as_str());

				rl.helper_mut().unwrap().status = Status::None;

				if line.is_empty() {
					continue;
				}

				match parse_line(line.as_str()) {
					Ok(request) => {
						bincode::serialize_into(&mut stream, &request).unwrap();
						stream.flush().unwrap();

						match bincode::deserialize_from::<&UnixStream, TaskmasterDaemonResult>(&mut stream).unwrap() {
							TaskmasterDaemonResult::Success => {
								println!("\x1b[92mSuccess\x1b[0m");
								rl.helper_mut().unwrap().status = Status::Success;
							}
							TaskmasterDaemonResult::Ok(s) => {
								println!("{s}");
								rl.helper_mut().unwrap().status = Status::Success;
							},
							TaskmasterDaemonResult::Raw(s) => {
								print!("{s}");
								rl.helper_mut().unwrap().status = Status::Success;
							},
							TaskmasterDaemonResult::Err(err) => {
								eprintln!("\x1b[91mError\x1b[0m: {err}");
								rl.helper_mut().unwrap().status = Status::Error;
							}
						}
					}
					Err(err) => {
						rl.helper_mut().unwrap().status = Status::Error;
						eprintln!("\x1b[91mError\x1b[0m: {err}")
					}
				}
			},
			Err(_) => {
				break
			}
		}
	}
}
