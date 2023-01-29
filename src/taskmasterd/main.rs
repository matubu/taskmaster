extern crate yaml_rust;

fn main() {
    let config_file = std::fs::read_to_string("configs/config.yaml")
        .expect("Could not read config file.");

    let config = yaml_rust::YamlLoader::load_from_str(config_file.as_str())
        .expect("Could not parse config file.");

    println!("{:?}", config);
}
