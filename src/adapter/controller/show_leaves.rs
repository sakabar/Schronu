use schronu::adapter::gateway::yaml::yaml_to_task;
use schronu::entity::task::extract_leaves;
use schronu::entity::task::Task;
use yaml_rust::{Yaml, YamlLoader};

fn main() {
    let s = "
project:
  name: 'parent-task'
  children:
    - name: 'child-task-1'
      children:
        - name: 'grandchild-task-1'
    - name: 'child-task-2'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let project_yaml: &Yaml = &docs[0]["project"];

    // println!("{:?}", project_yaml);

    let project: Task = yaml_to_task(project_yaml);
    let leaves = extract_leaves(&project);

    println!("{:?}", leaves);
}
