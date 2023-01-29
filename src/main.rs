extern crate yaml_rust;

fn main() {
    let s =
"
foo:
    - list1
    - list2
bar:
    - 1
    - 2.0
";
    let docs = yaml_rust::YamlLoader::load_from_str(s).unwrap();

    println!("{:?}", docs);
}
