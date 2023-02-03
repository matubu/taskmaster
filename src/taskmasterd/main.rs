extern crate taskmastersocket;
use taskmastersocket::{TaskmasterDaemonRequest, TaskmasterDaemonResult};

use std::{collections::{HashMap, HashSet}, process::Child, fs::File, os::unix::{net::{UnixListener, UnixStream}}, thread, io::Write, sync::{Mutex, Arc, MutexGuard}, time::{Duration, Instant}};

use daemonize::Daemonize;

macro_rules! get_required (
	($yaml:ident, $key:tt, $convert:ident) => (
		$yaml[$key].$convert()
			.ok_or(concat!($key, " is required and need to be ", stringify!($convert)))?.to_owned()
	)
);

macro_rules! get_optional (
	($yaml:ident, $key:tt, $convert:ident, $default:expr) => (
		$yaml[$key].$convert()
			.unwrap_or($default)
	)
);

#[derive(PartialEq, Clone, Debug)]
enum TaskOptionAutoRestart {
	Always,
	Never,
	Unexpected(HashSet<i32>)
}

#[derive(PartialEq, Clone, Debug)]
struct TaskOptions {
	argv: Vec<String>,
	numprocs: u64,
	autostart: bool,
	autorestart: TaskOptionAutoRestart,
	starttime_sec: u64,
	retries: u64,
	stopsignal: libc::c_int,
	stoptime_sec: u64,
	stdout: Option<String>,
	stderr: Option<String>,
	env: HashMap<String, String>,
	workingdir: Option<String>,
	umask: u16,
}

enum ExitStatus {
	NotRunning,
	LaunchFailed{at: Instant, err: String},
	
	Running{since: Instant, pid: u32},
	
	Stopping{at: Instant},
	
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
		let mut _spawn = || -> Result<(), String> {
			let mut process = std::process::Command::new(&opts.argv[0]);

			process.args(&opts.argv[1..]);

			if let Some(stdout) = &opts.stdout {
				process.stdout(File::create(stdout).map_err(|_| "Could not create stdout file")?);
			}
			if let Some(stderr) = &opts.stderr {
				process.stderr(File::create(stderr).map_err(|_| "Could not create stderr file")?);
			}
			process.envs(&opts.env);
			if let Some(workingdir) = &opts.workingdir {
				process.current_dir(workingdir);
			}
			unsafe { libc::umask(opts.umask) };

			match process.spawn() {
				Ok(child) => {
					self.current_status = ExitStatus::Running{since: Instant::now(), pid: child.id()};
					self.process = Some(child);
				},
				Err(e) => {
					return Err(e.to_string());
				}
			}

			Ok(())
		};

		if let Err(err) = _spawn() {
			self.current_status = ExitStatus::LaunchFailed{at: Instant::now(), err};
		}
	}

	fn start(&mut self, opts: &TaskOptions) {
		if self.process.is_some() {
			return;
		}

		self.spawn(opts);

		self.retries_count = 0;
	}

	fn graceful_stop(&mut self, stopsignal: libc::c_int) {
		if let Some(child) = &mut self.process {
			unsafe { libc::kill(child.id() as i32, stopsignal); }
			self.current_status = ExitStatus::Stopping{at: Instant::now()};
		}
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
				let mut restart = opts.autorestart == TaskOptionAutoRestart::Always;

				if let Some(code) = status.code() {
					self.current_status = ExitStatus::Exited{at: Instant::now(), code};
					if let TaskOptionAutoRestart::Unexpected(codes) = &opts.autorestart {
						restart = !codes.contains(&code);
					}
				} else {
					self.current_status = ExitStatus::Stopped{at: Instant::now()};
				}

				self.process = None;

				if !restart || self.retries_count >= opts.retries {
					return;
				}
				self.retries_count += 1;
				self.spawn(opts);
			} else if let ExitStatus::Stopping { at } = &self.current_status {
				if at.elapsed().as_secs() >= opts.stoptime_sec {
					self.stop();
				}
			}
		}
	}

	fn status(&self, opts: &TaskOptions) -> String {
		(match &self.current_status {
			ExitStatus::NotRunning => format!("\x1b[90mNot running"),
			ExitStatus::LaunchFailed{at, err} => format!("\x1b[91mLaunch failed ({}s ago): {err}", at.elapsed().as_secs()),
			ExitStatus::Running{since, pid} => {
				let since = since.elapsed().as_secs();
				format!("\x1b[92m{} (started {}s ago with pid {pid})",
					if since >= opts.starttime_sec { "Running" } else { "Starting..." },
					since)
			},
			ExitStatus::Stopping { at } => format!("\x1b[93mStopping... ({}s ago)", at.elapsed().as_secs()),
			ExitStatus::Exited{at, code} => format!("\x1b[91mExited ({}s ago) with code {code}", at.elapsed().as_secs()),
			ExitStatus::Stopped{at} => format!("\x1b[93mStopped ({}s ago)", at.elapsed().as_secs()),
			ExitStatus::Killed{at} => format!("\x1b[93mKilled ({}s ago)", at.elapsed().as_secs()),
		}) + &format!("\x1b[90m (created {}s ago, {} retries)\x1b[0m", self.created_at.elapsed().as_secs(), self.retries_count)
	}
}

struct Task {
	id: usize,
	options: TaskOptions,
	processes: Vec<Process>,
}

impl Task {
	fn new(options: TaskOptions) -> Task {
		static mut id: usize = 0;

		Task {
			id: unsafe { id += 1; id },
			options,
			processes: Vec::new()
		}
	}

	fn init(&mut self) {
		if self.options.autostart {
			self.start();
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

	fn graceful_stop(&mut self) {
		for process in &mut self.processes {
			process.graceful_stop(self.options.stopsignal);
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

	fn status(&self, ident: &str) -> String {
		let mut status = String::new();

		for i in 0..self.processes.len() {
			status.push_str(&format!("{ident}[{i}] -> {}\n", self.processes[i].status(&self.options)));
		}

		status
	}
}

struct TaskFile {
	path: String,
	tasks: HashMap<String, Task>,
}

fn parse_signal(sig: &str) -> Option<libc::c_int>{
	match sig {
		"HUP" => Some(1),
		"INT" => Some(2),
		"QUIT" => Some(3),
		"ILL" => Some(4),
		"TRAP" => Some(5),
		"ABRT" => Some(6),
		"EMT" => Some(7),
		"FPE" => Some(8),
		"KILL" => Some(9),
		"BUS" => Some(10),
		"SEGV" => Some(11),
		"SYS" => Some(12),
		"PIPE" => Some(13),
		"ALRM" => Some(14),
		"TERM" => Some(15),
		"URG" => Some(16),
		"STOP" => Some(17),
		"TSTP" => Some(18),
		"CONT" => Some(19),
		"CHLD" => Some(20),
		"TTIN" => Some(21),
		"TTOU" => Some(22),
		"IO" => Some(23),
		"XCPU" => Some(24),
		"XFSZ" => Some(25),
		"VTALRM" => Some(26),
		"PROF" => Some(27),
		"WINCH" => Some(28),
		"INFO" => Some(29),
		"USR1" => Some(30),
		"USR2" => Some(31),
		_ => None,
	}
}

impl TaskFile {
	// TODO remove unwrap and expect
	fn from_yaml(path: &str) -> Result<TaskFile, &str> {
		let mut task_file = TaskFile {
			path: path.to_owned(),
			tasks: HashMap::new(),
		};

		let config_file = std::fs::read_to_string(path)
			.map_err(|_| "Could not open file")?;

		let config = yaml_rust::YamlLoader::load_from_str(config_file.as_str())
			.map_err(|_| "Could not parse config file")?;

		for doc in config {
			if let Some(programs) = doc["programs"].as_hash() {
				for (key, value) in programs {
					let name = key.as_str()
						.ok_or("Expect a program name")?;

					let cmd = get_required!(value, "cmd", as_str);
					let argv = cmd.split_whitespace().collect::<Vec<&str>>()
						.iter().map(|s| (*s).to_owned()).collect();
					
					let exitcodes: HashSet<i32> = get_optional!(value, "exitcodes", as_vec, &Vec::new())
						.iter().filter_map(|v| {
							if let Some(n) = v.as_i64() {
								return Some(n as i32)
							}
							None
						}).collect::<HashSet<i32>>();
					let autorestart = match get_optional!(value, "autorestart", as_str, "always"){
						"always" => TaskOptionAutoRestart::Always,
						"unexpected" => TaskOptionAutoRestart::Unexpected(exitcodes),
						"never" => TaskOptionAutoRestart::Never,
						_ => return Err("Invalid autorestart value")
					};

					let env: HashMap<String, String> = value["env"].as_hash()
						.map(|h| h.iter().filter_map(|(k, v)| {
							if let (Some(a), Some(b)) = (k.as_str(), v.as_str()) {
								return Some((a.to_owned(), b.to_owned()))
							}
							None
						}).collect()).unwrap_or(HashMap::new());

					task_file.tasks.insert(name.to_owned(),
					Task::new(TaskOptions {
						argv,
						numprocs: get_optional!(value, "numprocs", as_i64, 1) as u64,
						autostart: get_optional!(value, "autostart", as_bool, true),
						autorestart,
						starttime_sec: get_optional!(value, "starttime", as_i64, 0) as u64,
						retries: get_optional!(value, "retries", as_i64, 8) as u64,
						stopsignal: parse_signal(get_optional!(value, "stopsignal", as_str, "TERM")).ok_or("Invalid stopsignal")?,
						stoptime_sec: get_optional!(value, "stoptime", as_i64, 0) as u64,
						stdout: value["stdout"].as_str().map(|s| s.to_owned()),
						stderr: value["stderr"].as_str().map(|s| s.to_owned()),
						env,
						workingdir: value["workingdir"].as_str().map(|s| s.to_owned()),
						umask: u16::from_str_radix(get_optional!(value, "umask", as_i64, 777).to_string().as_str(), 8).unwrap_or(0o777),
					}));
				}
			}
		}

		Ok(task_file)
	}

	fn init(&mut self) {
		for task in self.tasks.values_mut() {
			task.init();
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
				let mut new_task = Task::new(task.options.clone());
				new_task.init();
				new_tasks.insert(name.to_owned(), new_task);
			}
		}

		for task in self.tasks.values_mut() {
			task.stop();
		}

		self.tasks = new_tasks;
	}

	fn reload(&mut self) -> Result<(), String> {
		let task_file = TaskFile::from_yaml(&self.path)?;
		self.update(task_file);
		Ok(())
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

	fn load(&mut self, path: &str) -> Result<(), String> {
		match TaskFile::from_yaml(path) {
			Ok(mut new_task_file) => {
				if let Some(task_file) = self.tasks_files.get_mut(path) {
					task_file.update(new_task_file);
				} else {
					new_task_file.init();
					self.tasks_files.insert(new_task_file.path.clone(), new_task_file);
				}
				Ok(())
			}
			Err(err) => {
				Err(format!("Failed to load {}: {}", path, err))
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

	fn status(&self) -> String {
		let mut status = String::new();

		for task_file in self.tasks_files.values() {
			if !status.is_empty() {
				status.push_str("\n");
			}
			status.push_str(&format!("{}:\n", task_file.path));
			for (name, task) in task_file.tasks.iter() {
				status.push_str(&format!(
					"\n  {name} (id {}):\n{}",
					task.id,
					task.status("    ")
				));
			}
		}

		status
	}

	fn find_by_id(&mut self, id: usize) -> Option<&mut Task> {
		for (_, task_file) in &mut self.tasks_files {
			for (_, task) in &mut task_file.tasks {
				if task.id == id {
					return Some(task);
				}
			}
		}

		None
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

fn handle_client_request(tasks: &mut MutexGuard<TaskFiles>, req: TaskmasterDaemonRequest) -> TaskmasterDaemonResult {
	match req {
		TaskmasterDaemonRequest::Status => {
			if tasks.tasks_files.is_empty() {
				return TaskmasterDaemonResult::Ok("No tasks loaded yet".to_owned());
			}

			TaskmasterDaemonResult::Raw(tasks.status())
		},
		TaskmasterDaemonRequest::Reload => {
			let mut errors = String::new();

			for task_file in tasks.tasks_files.values_mut() {
				if let Err(err) = task_file.reload() {
					errors.push_str(format!("\n  - Failed to reload {}: {}", task_file.path, err).as_str());
				}
			}
			if errors.len() > 0 {
				TaskmasterDaemonResult::Err(errors)
			} else {
				TaskmasterDaemonResult::Success
			}
		},
		TaskmasterDaemonRequest::Restart => {
			for task_file in tasks.tasks_files.values_mut() {
				task_file.stop();
				task_file.start();
			}
			TaskmasterDaemonResult::Success
		},
		TaskmasterDaemonRequest::StartTask(id) => {
			if let Some(task) = tasks.find_by_id(id) {
				task.start();
				return TaskmasterDaemonResult::Success
			}
			TaskmasterDaemonResult::Err("Task not found".to_owned())
		}
		TaskmasterDaemonRequest::StopTask(id) => {
			if let Some(task) = tasks.find_by_id(id) {
				task.graceful_stop();
				return TaskmasterDaemonResult::Success
			}
			TaskmasterDaemonResult::Err("Task not found".to_owned())
		}
		TaskmasterDaemonRequest::RestartTask(id) => {
			if let Some(task) = tasks.find_by_id(id) {
				task.stop();
				task.start();
				return TaskmasterDaemonResult::Success
			}
			TaskmasterDaemonResult::Err("Task not found".to_owned())
		}
		TaskmasterDaemonRequest::InfoTask(id) => {
			if let Some(task) = tasks.find_by_id(id) {
				return TaskmasterDaemonResult::Raw(
					format!(
						"{:?}\n{}",
						task.options,
						task.status("  ")
					)
				)
			}
			TaskmasterDaemonResult::Err("Task not found".to_owned())
		}
		TaskmasterDaemonRequest::LoadFile(path) => {
			match tasks.load(&path) {
				Ok(_) => TaskmasterDaemonResult::Success,
				Err(err) => return TaskmasterDaemonResult::Err(err)
			}
		},
		TaskmasterDaemonRequest::UnloadFile(path) => {
			tasks.unload(&path);
			TaskmasterDaemonResult::Success
		},
		_ => TaskmasterDaemonResult::Err("Not implemented".to_owned())
	}
}

fn main() {
	let listener = bind("/tmp/taskmasterd.sock").expect("Could not create unix socket");

	// let stdout = File::create("/tmp/taskmasterd.out").unwrap();
	// let stderr = File::create("/tmp/taskmasterd.err").unwrap();
	// let daemonize = Daemonize::new()
	// 	.user("nobody")
	// 	.group("nogroup")
	// 	.stdout(stdout)
	// 	.stderr(stderr);

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
