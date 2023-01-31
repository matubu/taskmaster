use std::{collections::HashMap, process::Child};

use yaml_rust::Yaml;

macro_rules! get_required (
    ($yaml:ident, $key:tt, $convert:ident) => (
        $yaml.remove(&Yaml::String($key.to_owned()))
            .expect(concat!("convert a ", $key)).$convert()
            .expect(concat!($key, "convert as ", stringify!($convert))).to_owned()
    )
);

macro_rules! get_optional (
    ($yaml:ident, $key:tt, $convert:ident, $default:expr) => (
        if let Some(value) = $yaml.remove(&Yaml::String($key.to_owned())) {
            value.$convert().expect(concat!($key, "convert as ", stringify!($convert))).to_owned()
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
    // TODO avoid respawn when command does not exit
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
                    println!("\"{name}\"[{i}] exited with status code {code}", name = self.options.argv[0], i = i, code = status.code().unwrap());
                } else {
                    println!("\"{name}\"[{i}] exited with status code {code}", name = self.options.argv[0], i = i, code = status.code().unwrap());
                }
                self.processes.remove(i);
            } else {
                i += 1;
            }
        }

        for _ in self.processes.len()..self.options.numprocs as usize {
            self.spawn_single();
        }
    }
}

struct TaskFile {
    name: String,
    tasks: HashMap<String, Task>,
}

impl TaskFile {
    fn from_yaml(absolute_path: &str) -> TaskFile {
        let mut task_file = TaskFile {
            name: absolute_path.to_owned(),
            tasks: HashMap::new(),
        };

        let config_file = std::fs::read_to_string(absolute_path)
            .expect("Could not read config file.");

        let config = yaml_rust::YamlLoader::load_from_str(config_file.as_str())
            .expect("Could not parse config file.");

        for doc in config {
            let programs = doc["programs"].as_hash().expect("convert a list of programs.");
            
            for (key, value) in programs {
                let name = key.as_str().expect("Expect a program name.");
                let mut program = value.as_hash().expect("convert a program.").clone();

                let cmd = get_required!(program, "cmd", as_str);
                let numprocs = get_optional!(program, "numprocs", as_i64, 1);

                for key in program.keys() {
                    eprintln!("\x1b[93m[Warning]\x1b[0m the {} value was ignored for \"{}\"", key.as_str().unwrap(), name);
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

        task_file
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

    fn load(&mut self, absolute_path: &str) {
        let mut new_task_file = TaskFile::from_yaml(absolute_path);

        if let Some(task_file) = self.tasks_files.get_mut(absolute_path) {
            task_file.update(new_task_file);
        } else {
            new_task_file.start();
            self.tasks_files.insert(new_task_file.name.clone(), new_task_file);
        }
    }

    fn unload(&mut self, absolute_path: &str) {
        if let Some(mut deleted) = self.tasks_files.remove(absolute_path) {
            deleted.stop();
        }
    }

    fn health_check(&mut self) {
        for task_file in self.tasks_files.values_mut() {
            task_file.health_check();
        }
    }
}

fn main() {
    let mut tasks = TaskFiles::new();

    {
        tasks.load("configs/config.yaml");
    }

    loop {
        tasks.health_check();
    }
}
