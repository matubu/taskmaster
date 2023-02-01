extern crate taskmastersocket;
use taskmastersocket::{TaskmasterDaemonRequest, TaskmasterDaemonResult};

use std::{collections::HashMap, process::Child, fs::File, os::unix::{net::{UnixListener, UnixStream}, process}, thread, io::Write, sync::{Mutex, Arc, MutexGuard}, time::{Duration, Instant}, fmt::format};

use daemonize::Daemonize;
use yaml_rust::Yaml;

macro_rules! get_required (
	($yaml:ident, $key:tt, $convert:ident) => (
		$yaml.remove(&Yaml::String($key.to_owned()))
			.ok_or(concat!("convert a ", $key))?.$convert()
			.ok_or(concat!($key, "convert as ", stringify!($convert)))?.to_owned()
	)
);

macro_rules! get_optional (
	($yaml:ident, $key:tt, $convert:ident, $default:expr) => (
		if let Some(value) = $yaml.remove(&Yaml::String($key.to_owned())) {
			value.$convert().ok_or(concat!($key, "convert as ", stringify!($convert)))?.to_owned()
		} else {
			$default
		}
	)
);

#[derive(PartialEq, Clone, Debug)]
struct TaskOptions {
	argv: Vec<String>,
	numprocs: i64,
}

enum ExitStatus {
	NotRunning,
	Running{since: Instant, pid: u32},
	LaunchFailed{at: Instant, err: String},
	Exited{at: Instant, code: i32},
	Stopped{at: Instant},
	Killed{at: Instant},
}

struct Process {
	process: Option<Child>,
	created_at: Instant,
	retries_count: u64,
	current_status: ExitStatus
}

impl Process {
	fn new() -> Process {
		Process {
			process: None,
			created_at: Instant::now(),
			retries_count: 0,
			current_status: ExitStatus::NotRunning
		}
	}

	fn spawn(&mut self, opts: &TaskOptions) {
		match std::process::Command::new(&opts.argv[0])
			.args(&opts.argv[1..])
			.spawn() {
			Ok(child) => {
				println!("\"{name}\" started", name = opts.argv[0]);
				self.current_status = ExitStatus::Running{since: Instant::now(), pid: child.id()};
				self.process = Some(child);
			},
			Err(e) => {
				eprintln!("\"{name}\" failed to start: {e}", name = opts.argv[0]);
				self.current_status = ExitStatus::LaunchFailed{at: Instant::now(), err: e.to_string()};
			}
		}
	}

	fn start(&mut self, opts: &TaskOptions) {
		if self.process.is_some() {
			return;
		}

		self.spawn(opts);

		self.retries_count = 0;
	}

	fn stop(&mut self) {
		if let Some(child) = &mut self.process {
			child.kill();
			self.process = None;
			self.current_status = ExitStatus::Killed{at: Instant::now()};
		}
	}

	fn health_check(&mut self, opts: &TaskOptions) {
		if let Some(child) = &mut self.process {
			if let Ok(Some(status)) = child.try_wait() {
				self.retries_count += 1;
				if let Some(code) = status.code() {
					self.current_status = ExitStatus::Exited{at: Instant::now(), code};
				}
				self.spawn(opts);
			}
		}
	}

	fn status(&self) -> String {
		(match &self.current_status {
			ExitStatus::NotRunning => format!("\x1b[90mNot running"),
			ExitStatus::Running{since, pid} => format!("\x1b[92mRunning (started {}s ago with pid {pid})", since.elapsed().as_secs()),
			ExitStatus::LaunchFailed{at, err} => format!("\x1b[91mLaunch failed ({}s ago): {err}", at.elapsed().as_secs()),
			ExitStatus::Exited{at, code} => format!("\x1b[91mExited ({}s ago) with code {code}", at.elapsed().as_secs()),
			ExitStatus::Stopped{at} => format!("\x1b[93mStopped ({}s ago)", at.elapsed().as_secs()),
			ExitStatus::Killed{at} => format!("\x1b[93mKilled ({}s ago)", at.elapsed().as_secs()),
		}) + &format!("\x1b[90m (created {}s ago, {} retries)\x1b[0m", self.created_at.elapsed().as_secs(), self.retries_count)
	}
}

struct Task {
	options: TaskOptions,
	processes: Vec<Process>,
}

impl Task {
	fn clone_options(&self) -> Task {
		Task {
			options: self.options.clone(),
			processes: Vec::new(),
		}
	}

	fn start(&mut self) {
		while self.processes.len() < self.options.numprocs as usize {
			self.processes.push(Process::new());
		}

		for process in &mut self.processes {
			process.start(&self.options);
		}
	}

	fn stop(&mut self) {
		for process in &mut self.processes {
			process.stop();
		}
	}

	fn update(&mut self, options: TaskOptions) {
		if self.options == options {
			return;
		}

		self.stop();
		self.options = options;
		self.start();
	}

	fn health_check(&mut self) {
		for process in &mut self.processes {
			process.health_check(&self.options);
		}
	}
}

struct TaskFile {
	path: String,
	tasks: HashMap<String, Task>,
}

impl TaskFile {
	// TODO remove unwrap and expect
	fn from_yaml(path: &str) -> Result<TaskFile, &str> {
		let mut task_file = TaskFile {
			path: path.to_owned(),
			tasks: HashMap::new(),
		};

		let config_file = std::fs::read_to_string(path)
			.map_err(|_| "Could not open file.")?;

		let config = yaml_rust::YamlLoader::load_from_str(config_file.as_str())
			.map_err(|_| "Could not parse config file.")?;

		for doc in config {
			if let Some(programs) = doc["programs"].as_hash() {
				for (key, value) in programs {
					let name = key.as_str()
						.ok_or("Expect a program name.")?;
					let mut program = value.as_hash()
						.ok_or("convert a program.")?.clone();

					let cmd = get_required!(program, "cmd", as_str);
					let numprocs = get_optional!(program, "numprocs", as_i64, 1);

					for key in program.keys() {
						eprintln!("\x1b[93m[Warning]\x1b[0m the {} value was ignored for \"{}\"", key.as_str().ok_or("Failed to convert to string")?, name);
					}

					let argv = cmd.split_whitespace().collect::<Vec<&str>>()
							.iter().map(|s| (*s).to_owned()).collect();

					task_file.add_task(name, Task {
						options: TaskOptions {
							argv: argv,
							numprocs: numprocs,
						},
						processes: Vec::new(),
					});
				}
			}
		}

		Ok(task_file)
	}

	fn add_task(&mut self, name: &str, task: Task) {
		if self.tasks.contains_key(name) {
			eprintln!("\x1b[93m[Warning]\x1b[0m duplicate program: \"{name}\"");
		} else {
			self.tasks.insert(name.to_owned(), task);
		}
	}

	fn start(&mut self) {
		for task in self.tasks.values_mut() {
			task.start();
		}
	}

	fn stop(&mut self) {
		for task in self.tasks.values_mut() {
			task.stop();
		}
	}

	fn update(&mut self, updated_task_file: TaskFile) {
		let mut new_tasks = HashMap::new();

		for (name, task) in updated_task_file.tasks.iter() {
			if let Some(mut old_task) = self.tasks.remove(name) {
				old_task.update(task.options.clone());
				new_tasks.insert(name.to_owned(), old_task);
			} else {
				new_tasks.insert(name.to_owned(), task.clone_options());
			}
		}

		for task in self.tasks.values_mut() {
			task.stop();
		}

		self.tasks = new_tasks;
	}

	fn reload(&mut self) {
		if let Ok(task_file) = TaskFile::from_yaml(&self.path) {
			self.update(task_file);
		}
	}

	fn health_check(&mut self) {
		for task in self.tasks.values_mut() {
			task.health_check();
		}
	}
}

struct TaskFiles {
	tasks_files: HashMap<String, TaskFile>
}

impl TaskFiles {
	fn new() -> TaskFiles {
		TaskFiles {
			tasks_files: HashMap::new(),
		}
	}

	fn load(&mut self, path: &str) {
		if let Ok(mut new_task_file) = TaskFile::from_yaml(path) {
			if let Some(task_file) = self.tasks_files.get_mut(path) {
				task_file.update(new_task_file);
			} else {
				new_task_file.start();
				self.tasks_files.insert(new_task_file.path.clone(), new_task_file);
			}
		}
	}

	fn unload(&mut self, path: &str) {
		if let Some(mut deleted) = self.tasks_files.remove(path) {
			deleted.stop();
		}
	}

	fn health_check(&mut self) {
		for task_file in self.tasks_files.values_mut() {
			task_file.health_check();
		}
	}
}

fn bind(path: &str) -> std::io::Result<UnixListener> {
	if let Err(err) = std::fs::remove_file(path) {
		if err.kind() != std::io::ErrorKind::NotFound {
			return Err(err);
		}
	}
	UnixListener::bind(path)
}

// TODO error handling load...
fn handle_client_request(tasks: &mut MutexGuard<TaskFiles>, req: TaskmasterDaemonRequest) -> TaskmasterDaemonResult {
	match req {
		TaskmasterDaemonRequest::Status => {
			if tasks.tasks_files.is_empty() {
				return TaskmasterDaemonResult::Ok("No tasks loaded yet".to_owned());
			}

			let mut status = String::new();
			for task_file in tasks.tasks_files.values() {
				if !status.is_empty() {
					status.push_str("\n");
				}
				status.push_str(&format!("{}:\n", task_file.path));
				for (name, task) in task_file.tasks.iter() {
					status.push_str(&format!("\n  {}:\n", name));
					for i in 0..task.processes.len() {
						status.push_str(&format!("    [{i}] -> {}\n", task.processes[i].status()));
					}
				}
			}
			TaskmasterDaemonResult::Raw(status)
		},
		TaskmasterDaemonRequest::Reload => {
			for task_file in tasks.tasks_files.values_mut() {
				task_file.reload();
			}
			TaskmasterDaemonResult::Ok("ok".to_owned())
		},
		TaskmasterDaemonRequest::Restart => {
			for task_file in tasks.tasks_files.values_mut() {
				task_file.stop();
				task_file.start();
			}
			TaskmasterDaemonResult::Ok("ok".to_owned())
		},
		TaskmasterDaemonRequest::LoadFile(path) => {
			tasks.load(&path);
			TaskmasterDaemonResult::Ok("ok".to_owned())
		},
		TaskmasterDaemonRequest::UnloadFile(path) => {
			tasks.unload(&path);
			TaskmasterDaemonResult::Ok("ok".to_owned())
		},
		_ => TaskmasterDaemonResult::Err("Not implemented".to_owned())
	}
}

fn main() {
	let listener = bind("/tmp/taskmasterd.sock").expect("Could not create unix socket");

	// TODO pid file ?
	// let stdout = File::create("/tmp/taskmasterd.out").unwrap();
	// let stderr = File::create("/tmp/taskmasterd.err").unwrap();
	// let daemonize = Daemonize::new()
	//     .stdout(stdout)
	//     .stderr(stderr);

	// daemonize.start().expect("Failed to daemonize");

	println!("Starting taskmasterd...");

	let tasks = Arc::new(Mutex::new(TaskFiles::new()));

	{
		let tasks = tasks.clone();
		thread::spawn(move || {
			println!("Starting health check loop...");

			loop {
				tasks.lock().unwrap().health_check();
				thread::sleep(Duration::from_nanos(500_000));
			}
		});
	}

	println!("Starting listener loop...");
	for stream in listener.incoming() {
		match stream {
			Ok(mut stream) => {
				let tasks = tasks.clone();
				thread::spawn(move || {
					loop {
						if let Ok(request) = bincode::deserialize_from::<&UnixStream, TaskmasterDaemonRequest>(&stream) {
							println!("read {:?}", request);

							let response = handle_client_request(
								&mut tasks.lock().unwrap(),
								request
							);

							bincode::serialize_into(&mut stream, &response).unwrap();
							stream.flush().unwrap();
						} else {
							if let Err(e) = stream.shutdown(std::net::Shutdown::Both) {
								eprintln!("Failed to shutdown stream: {}", e);
							}
							break ;
						}
					}
				});
			}
			Err(err) => {
				eprintln!("Failed to connect: {}", err);
				break;
			}
		}
	}
}
