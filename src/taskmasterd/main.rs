extern crate taskmastersocket;
use taskmastersocket::{TaskmasterDaemonRequest, TaskmasterDaemonResult};

use std::{collections::HashMap, process::Child, fs::File, os::unix::net::{UnixListener, UnixStream}, thread, io::Write, sync::{Mutex, Arc, MutexGuard}, time::Duration};

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

#[derive(PartialEq, Debug)]
struct TaskOptions {
	argv: Vec<String>,
	numprocs: i64,
}

struct Task {
	options: TaskOptions,
	processes: Vec<Child>,
}

impl Task {
	fn spawn_single(&mut self) {
		match std::process::Command::new(&self.options.argv[0])
			.args(&self.options.argv[1..])
			.spawn() {
			Ok(child) => {
				println!("\"{name}\" started", name = self.options.argv[0]);
				self.processes.push(child);
			},
			Err(e) => {
				eprintln!("\"{name}\" failed to start: {e}", name = self.options.argv[0]);
			}
		}
	}

	fn start(&mut self) {
		println!("start task...");
		for i in 0..self.options.numprocs {
			self.spawn_single()
		}
	}

	fn stop(&mut self) {
		for child in &mut self.processes {
			child.kill().expect("Could not kill child process.");
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
		let mut i = 0;
		while i < self.processes.len() {
			let child = &mut self.processes[i];
			if let Ok(Some(status)) = child.try_wait() {
				if status.success() {
					println!("\"{name}\"[{i}] exited with status code {code}", name = self.options.argv[0], code = status.code().unwrap());
				} else {
					println!("\"{name}\"[{i}] exited with status code {code}", name = self.options.argv[0], code = status.code().unwrap());
				}
				self.processes.remove(i);
				self.spawn_single();
			}
			i += 1;
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
		// TODO smart update
		self.stop();
		self.tasks = updated_task_file.tasks;
		self.start();
	}

	// Check if their was any change in the file
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
	std::fs::remove_file(path)?;
	UnixListener::bind(path)
}

// TODO error handling load...
fn handle_client_request(tasks: &mut MutexGuard<TaskFiles>, req: TaskmasterDaemonRequest) -> TaskmasterDaemonResult {
	match req {
		TaskmasterDaemonRequest::Status => {
			let mut status = String::new();
			for task_file in tasks.tasks_files.values() {
				for (name, task) in task_file.tasks.iter() {
					status.push_str(&format!("{}: {}: {:?}\n", task_file.path, name, task.processes.iter().map(|p| p.id()).collect::<Vec<u32>>()));
				}
			}
			TaskmasterDaemonResult::Ok(status)
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
	// let config_path = "configs/config.yaml".try_resolve()
	//     .expect("Could not resolve config file path.").into_owned();

	// println!("Config file: {:?}", config_path);

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

	// TODO should be the client that load/unload the config file
	// tasks.load(config_path.to_str().unwrap());

	println!("Starting health check loop...");

	{
		let tasks = tasks.clone();
		thread::spawn(move || {
			loop {
				tasks.lock().unwrap().health_check();
				thread::sleep(Duration::from_nanos(500_000));
			}
		});
	}

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
