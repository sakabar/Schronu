#![cfg(feature = "benchmarking")]
#![allow(dead_code)]

#[path = "support/scheduling_fixture.rs"]
mod scheduling_fixture;
#[path = "support/scheduling_harness.rs"]
mod scheduling_harness;

use scheduling_fixture::{FixtureSize, SchedulingFixture};
use scheduling_harness::SchedulingRepository;
use schronu::adapter::mcp::McpServer;
use serde_json::json;
use std::fs;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[test]
fn mcp_list_tasksのtypicalとstress_pageは128kib以内で速やかに返る() {
    for size in [FixtureSize::Typical, FixtureSize::Stress] {
        let fixture = SchedulingFixture::build(size).unwrap();
        let summary = fixture.summary().unwrap();
        let repository = SchedulingRepository::new(fixture.projects, fixture.now);
        let storage_directory = std::env::temp_dir().join(format!(
            "schronu-mcp-pagination-benchmark-{}",
            Uuid::new_v4()
        ));
        fs::create_dir(&storage_directory).unwrap();
        let mut server = McpServer::with_storage_directory(repository, &storage_directory);
        server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "initialize",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "pagination-benchmark", "version": "1.0"}
                }
            }))
            .unwrap();
        assert!(server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }))
            .is_none());

        let started = Instant::now();
        let response = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": "first-page",
                "method": "tools/call",
                "params": {"name": "list_tasks", "arguments": {}}
            }))
            .unwrap();
        let elapsed = started.elapsed();
        let content = &response["result"]["structuredContent"];

        assert!(summary.tasks > content["tasks"].as_array().unwrap().len());
        assert_eq!(content["tasks"].as_array().unwrap().len(), 100);
        assert!(content["next_cursor"].is_string());
        assert!(serde_json::to_vec(&response).unwrap().len() <= 128 * 1024);
        assert!(
            elapsed <= Duration::from_secs(2),
            "{size:?} MCP list_tasks response exceeded wall limit: {elapsed:?}"
        );
        fs::remove_dir_all(storage_directory).unwrap();
    }
}
