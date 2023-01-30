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

fn main() {
    let config_file = std::fs::read_to_string("configs/config.yaml")
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
                println!("\x1b[93m[Warning]\x1b[0m the {} value was ignored in {}", key.as_str().unwrap(), name);
            }

            let argv = cmd.split_whitespace().collect::<Vec<&str>>();
            for i in 1..=numprocs {
                match std::process::Command::new(argv[0])
                    .args(&argv[1..])
                    .spawn() {
                    Ok(child) => {
                        println!("{}: {}", i, child.id());
                    }
                    Err(err) => {
                        println!("\x1b[91m[Error]\x1b[0m Failed to launch \"{name}\" ({i}): {err}");
                    }
                }
            }
        }
    }
}
