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

struct TaskFile {
    name: String,
    tasks: HashMap<String, Task>,
}

impl TaskFile {
    fn new(name: String) -> TaskFile {
        TaskFile {
            name,
            tasks: HashMap::new(),
        }
    }

    fn add_task(&mut self, name: &str, task: Task) {
        if self.tasks.insert(name.to_owned(), task).is_some() {
            eprintln!("\x1b[93m[Warning]\x1b[0m duplicate key: \"{name}\"");
        }
    }
}

fn load_yaml(absolute_path: &str) -> TaskFile {
    let mut task_file = TaskFile::new(absolute_path.to_owned());

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
            // for i in 0..numprocs {
            //     match std::process::Command::new(argv[0])
            //         .args(&argv[1..])
            //         .spawn() {
            //         Ok(child) => {
            //             println!("\"{name}\"[{i}]: {}", child.id());
            //         }
            //         Err(err) => {
            //             println!("\x1b[91m[Error]\x1b[0m Failed to launch \"{name}\"[{i}]: {err}");
            //         }
            //     }
            // }
        }
    }

    task_file
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

    fn add_task_file(&mut self, task_file: TaskFile) {
        // TODO Insert and destroy running processes
        self.tasks_files.insert(task_file.name.clone(), task_file);
    }
}

fn main() {
    let mut tasks = TaskFiles::new();

    {
        let task_file = load_yaml("configs/config.yaml");
        tasks.add_task_file(task_file);
    }

    // let config_file = std::fs::read_to_string("configs/config.yaml")
    //     .expect("Could not read config file.");

    // let config = yaml_rust::YamlLoader::load_from_str(config_file.as_str())
    //     .expect("Could not parse config file.");

    // for doc in config {
    //     let programs = doc["programs"].as_hash().expect("convert a list of programs.");
        
    //     for (key, value) in programs {
    //         let name = key.as_str().expect("Expect a program name.");
    //         let mut program = value.as_hash().expect("convert a program.").clone();

    //         let cmd = get_required!(program, "cmd", as_str);
    //         let numprocs = get_optional!(program, "numprocs", as_i64, 1);

    //         for key in program.keys() {
    //             eprintln!("\x1b[93m[Warning]\x1b[0m the {} value was ignored for \"{}\"", key.as_str().unwrap(), name);
    //         }

    //         let argv = cmd.split_whitespace().collect::<Vec<&str>>();
    //         for i in 0..numprocs {
    //             match std::process::Command::new(argv[0])
    //                 .args(&argv[1..])
    //                 .spawn() {
    //                 Ok(child) => {
    //                     println!("\"{name}\"[{i}]: {}", child.id());
    //                 }
    //                 Err(err) => {
    //                     eprintln!("\x1b[91m[Error]\x1b[0m Failed to launch \"{name}\"[{i}]: {err}");
    //                 }
    //             }
    //         }
    //     }
    // }
}
