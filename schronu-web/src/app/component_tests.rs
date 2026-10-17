#![cfg(feature = "server")]

include!("component_tests_part1.rs");
include!("component_tests_part2.rs");

#[path = "load_refresh_tests.rs"]
mod load_refresh_tests;

#[path = "read_load_refresh_tests.rs"]
mod read_load_refresh_tests;
