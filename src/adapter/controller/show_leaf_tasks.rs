use chrono::Local;
use schronu::adapter::gateway::yaml::yaml_to_task;
use schronu::application::show_leaf_tasks::show_leaf_tasks;
use schronu::entity::task::Task;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use walkdir::WalkDir;
use yaml_rust::{Yaml, YamlLoader};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        println!("{}", args.len());
        panic!("usage: {} <projects_dir>", &args[0]);
    }

    let projects_dir = &args[1];
    let mut projects = vec![];

    for entry in WalkDir::new(projects_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_name() == "project.yaml" {
            let mut file = File::open(entry.path()).unwrap();
            let mut text = String::new();
            file.read_to_string(&mut text).unwrap();

            match YamlLoader::load_from_str(text.as_str()) {
                Err(_) => {
                    panic!("Error occured in {:?}", entry.path());
                }
                Ok(docs) => {
                    let project_yaml: &Yaml = &docs[0]["project"];
                    let project: Task = yaml_to_task(project_yaml, Local::now());

                    projects.push(project);
                }
            }
        }
    }

    show_leaf_tasks(&projects);
}
