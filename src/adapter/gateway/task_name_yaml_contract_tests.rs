use super::*;

fn task_yaml(name: &str, children: Vec<Yaml>) -> Yaml {
    let mut task = LinkedHashMap::new();
    task.insert(
        Yaml::String("name".to_string()),
        Yaml::String(name.to_string()),
    );
    if !children.is_empty() {
        task.insert(Yaml::String("children".to_string()), Yaml::Array(children));
    }
    Yaml::Hash(task)
}

#[test]
fn strict_yamlは許可されたrootとchildのtask名を原文どおり保持する() {
    let root_name = "  日本語  'single' \"double\" \\ root  ";
    let child_name = "  子  '引用' \"quoted\" \\ child  ";
    let yaml = task_yaml(root_name, vec![task_yaml(child_name, vec![])]);

    let root = yaml_to_task(&yaml, Local::now()).unwrap();
    let child = root.get_children().unwrap().remove(0);

    assert_eq!(root.get_name().unwrap(), root_name);
    assert_eq!(child.get_name().unwrap(), child_name);
}

#[test]
fn strict_yamlは全unicode_controlをroot名でpath付き拒否する() {
    for control in all_unicode_controls() {
        let name = format!("root{control}name");
        let actual = yaml_to_task(&task_yaml(&name, vec![]), Local::now()).unwrap_err();

        assert_eq!(
            actual.to_string(),
            "cannot convert project YAML to task: project.name: must not contain control characters",
            "U+{:04X}",
            u32::from(control)
        );
    }
}

#[test]
fn strict_yamlは全unicode_controlをchild名でpath付き拒否する() {
    for control in all_unicode_controls() {
        let name = format!("child{control}name");
        let yaml = task_yaml("root", vec![task_yaml(&name, vec![])]);
        let actual = yaml_to_task(&yaml, Local::now()).unwrap_err();

        assert_eq!(
            actual.to_string(),
            "cannot convert project YAML to task: project.children[0].name: must not contain control characters",
            "U+{:04X}",
            u32::from(control)
        );
    }
}

fn all_unicode_controls() -> impl Iterator<Item = char> {
    (0..=char::MAX as u32)
        .filter_map(char::from_u32)
        .filter(|character| character.is_control())
}
