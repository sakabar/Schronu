use super::test_support::*;
use super::McpServer;

#[test]
fn initializeはserver情報とtools能力を返す() {
    let mut server = McpServer::new(TaskRepository::new(""));
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0"}
        }
    });

    let response = server.handle_request(request).unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(response["result"]["serverInfo"]["name"], "schronu");
    assert_eq!(
        response["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        response["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
}

#[test]
fn initializeは非対応version要求にserver対応versionを返す() {
    let mut server = McpServer::new(TaskRepository::new(""));
    let request = json!({
        "jsonrpc": "2.0",
        "id": "initialize-unsupported-version",
        "method": "initialize",
        "params": {
            "protocolVersion": "2099-01-01",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0"}
        }
    });

    let response = server.handle_request(request).unwrap();

    assert_eq!(response["id"], "initialize-unsupported-version");
    assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn initializeとtools_listではrepository_clockを同期もloadもしない() {
    let repository = RecordingRepository::new(vec![]);
    let load_count = Rc::clone(&repository.load_count);
    let sync_clock_times = Rc::clone(&repository.sync_clock_times);
    let mut server = McpServer::new(repository);

    server.handle_request(initialize_request()).unwrap();
    server.handle_request(json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));
    server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "tools-list",
            "method": "tools/list"
        }))
        .unwrap();

    assert!(sync_clock_times.borrow().is_empty());
    assert_eq!(load_count.get(), 0);
}

#[test]
#[allow(non_snake_case)]
fn 初期化完了前のtools_callはUninitializedとInitializeRespondedの両方で拒否しrepository_clockを同期もloadもしない(
) {
    let repository = RecordingRepository::new(vec![]);
    let load_count = Rc::clone(&repository.load_count);
    let sync_clock_times = Rc::clone(&repository.sync_clock_times);
    let mut server = McpServer::new(repository);
    let request = tool_call_request("before-initialized", "list_tasks", json!({}));

    let uninitialized = server.handle_request(request.clone()).unwrap();
    assert_eq!(uninitialized["error"]["code"], -32002);
    assert_eq!(uninitialized["error"]["message"], "Server not initialized");
    assert!(sync_clock_times.borrow().is_empty());
    assert_eq!(load_count.get(), 0);

    server.handle_request(initialize_request()).unwrap();
    let initialize_responded = server.handle_request(request).unwrap();
    assert_eq!(initialize_responded["error"]["code"], -32002);
    assert_eq!(
        initialize_responded["error"]["message"],
        "Server not initialized"
    );
    assert!(sync_clock_times.borrow().is_empty());
    assert_eq!(load_count.get(), 0);
}

#[test]
fn json_rpc_requestは未知methodにmethod_not_foundを返す() {
    let mut server = McpServer::new(TaskRepository::new(""));
    let request = json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "unknown/method"
    });

    let response = server.handle_request(request).unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 99);
    assert_eq!(response["error"]["code"], -32601);
    assert_eq!(response["error"]["message"], "Method not found");
}

#[test]
fn json_rpc_requestのenvelope不正時も有効なidをerror応答へ引き継ぐ() {
    let cases = [
        ("non-object", json!([]), json!(null)),
        (
            "missing-jsonrpc-without-id",
            json!({"method": "initialize", "params": {}}),
            json!(null),
        ),
        (
            "initialize-without-id",
            json!({
                "jsonrpc": "2.0",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"}
                }
            }),
            json!(null),
        ),
        (
            "missing-jsonrpc",
            json!({"id": "missing-jsonrpc", "method": "initialize", "params": {}}),
            json!("missing-jsonrpc"),
        ),
        (
            "wrong-jsonrpc",
            json!({"jsonrpc": "1.0", "id": 1, "method": "initialize", "params": {}}),
            json!(1),
        ),
        (
            "non-string-jsonrpc",
            json!({"jsonrpc": 2, "id": 2, "method": "initialize", "params": {}}),
            json!(2),
        ),
        (
            "missing-method",
            json!({"jsonrpc": "2.0", "id": 3}),
            json!(3),
        ),
        (
            "non-string-method",
            json!({"jsonrpc": "2.0", "id": 4, "method": false}),
            json!(4),
        ),
        (
            "null-id",
            json!({"jsonrpc": "2.0", "id": null, "method": "initialize", "params": {}}),
            json!(null),
        ),
        (
            "boolean-id",
            json!({"jsonrpc": "2.0", "id": true, "method": "initialize", "params": {}}),
            json!(null),
        ),
        (
            "object-id",
            json!({"jsonrpc": "2.0", "id": {"invalid": true}, "method": "initialize", "params": {}}),
            json!(null),
        ),
    ];

    for (label, request, expected_id) in cases {
        let repository = RecordingRepository::new(vec![]);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = McpServer::new(repository);

        let response = server
            .handle_request(request)
            .unwrap_or_else(|| panic!("case {label} must receive an Invalid Request response"));

        assert_eq!(response["jsonrpc"], "2.0", "case: {label}");
        assert_eq!(response["id"], expected_id, "case: {label}");
        assert_eq!(response["error"]["code"], -32600, "case: {label}");
        assert_eq!(
            response["error"]["message"], "Invalid Request",
            "case: {label}"
        );
        assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
        assert_eq!(mutation_count.get(), 0, "case: {label}");
        assert_eq!(save_count.get(), 0, "case: {label}");

        let valid_initialize = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": format!("valid-after-{label}"),
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"}
                }
            }))
            .unwrap();
        assert_eq!(
            valid_initialize["result"]["protocolVersion"], "2025-06-18",
            "case: {label}"
        );
        assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
        assert_eq!(mutation_count.get(), 0, "case: {label}");
        assert_eq!(save_count.get(), 0, "case: {label}");
    }
}

#[test]
fn initializeの不正paramsをinvalid_paramsとして拒否しlifecycleを進めない() {
    let cases = [
        ("missing-params", None, "params", "required"),
        ("null-params", Some(json!(null)), "params", "object"),
        ("array-params", Some(json!([])), "params", "object"),
        (
            "missing-protocol-version",
            Some(json!({
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            })),
            "params.protocolVersion",
            "required",
        ),
        (
            "wrong-protocol-version-type",
            Some(json!({
                "protocolVersion": 1,
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            })),
            "params.protocolVersion",
            "string",
        ),
        (
            "missing-capabilities",
            Some(json!({
                "protocolVersion": "2025-06-18",
                "clientInfo": {"name": "test-client", "version": "1.0"}
            })),
            "params.capabilities",
            "required",
        ),
        (
            "wrong-capabilities-type",
            Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": [],
                "clientInfo": {"name": "test-client", "version": "1.0"}
            })),
            "params.capabilities",
            "object",
        ),
        (
            "missing-client-info",
            Some(json!({"protocolVersion": "2025-06-18", "capabilities": {}})),
            "params.clientInfo",
            "required",
        ),
        (
            "wrong-client-info-type",
            Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": []
            })),
            "params.clientInfo",
            "object",
        ),
        (
            "missing-client-name",
            Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"version": "1.0"}
            })),
            "params.clientInfo.name",
            "required",
        ),
        (
            "wrong-client-name-type",
            Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": false, "version": "1.0"}
            })),
            "params.clientInfo.name",
            "string",
        ),
        (
            "missing-client-version",
            Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client"}
            })),
            "params.clientInfo.version",
            "required",
        ),
        (
            "wrong-client-version-type",
            Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": 1}
            })),
            "params.clientInfo.version",
            "string",
        ),
        (
            "wrong-client-title-type",
            Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0", "title": 1}
            })),
            "params.clientInfo.title",
            "string",
        ),
    ];

    for (label, params, expected_field, expected_reason_token) in cases {
        let repository = RecordingRepository::new(vec![]);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = McpServer::new(repository);
        let mut request = json!({
            "jsonrpc": "2.0",
            "id": label,
            "method": "initialize"
        });
        if let Some(params) = params {
            request["params"] = params;
        }

        let response = server.handle_request(request).unwrap();

        assert_eq!(response["jsonrpc"], "2.0", "case: {label}");
        assert_eq!(response["id"], label, "case: {label}");
        assert_eq!(response["error"]["code"], -32602, "case: {label}");
        assert_eq!(
            response["error"]["message"], "Invalid params",
            "case: {label}"
        );
        assert_eq!(
            response["error"]["data"]["field"], expected_field,
            "case: {label}"
        );
        let reason = response["error"]["data"]["reason"]
            .as_str()
            .unwrap_or_else(|| panic!("case {label} must include a reason"));
        assert!(
            reason.to_ascii_lowercase().contains(expected_reason_token),
            "case: {label}, reason: {reason}"
        );
        assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
        assert_eq!(mutation_count.get(), 0, "case: {label}");
        assert_eq!(save_count.get(), 0, "case: {label}");

        assert_eq!(
            server.handle_request(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            })),
            None,
            "case: {label}"
        );
        let before_valid_initialize = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": format!("valid-after-{label}"),
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"}
                }
            }))
            .unwrap();
        assert_eq!(
            before_valid_initialize["result"]["protocolVersion"], "2025-06-18",
            "case: {label}"
        );
        assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
        assert_eq!(mutation_count.get(), 0, "case: {label}");
        assert_eq!(save_count.get(), 0, "case: {label}");
    }
}

#[test]
fn initializeの既知client_capabilityをschemaどおり検証する() {
    let cases = [
        (
            "wrong-roots-type",
            json!({"roots": []}),
            "params.capabilities.roots",
            "object",
        ),
        (
            "wrong-roots-list-changed-type",
            json!({"roots": {"listChanged": "yes"}}),
            "params.capabilities.roots.listChanged",
            "boolean",
        ),
        (
            "wrong-sampling-type",
            json!({"sampling": false}),
            "params.capabilities.sampling",
            "object",
        ),
        (
            "wrong-elicitation-type",
            json!({"elicitation": []}),
            "params.capabilities.elicitation",
            "object",
        ),
        (
            "wrong-experimental-type",
            json!({"experimental": []}),
            "params.capabilities.experimental",
            "object",
        ),
        (
            "wrong-experimental-capability-type",
            json!({"experimental": {"feature": true}}),
            "params.capabilities.experimental.feature",
            "object",
        ),
    ];

    for (label, capabilities, expected_field, expected_reason_token) in cases {
        let repository = RecordingRepository::new(vec![]);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = McpServer::new(repository);

        let response = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": label,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": capabilities,
                    "clientInfo": {"name": "test-client", "version": "1.0"}
                }
            }))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0", "case: {label}");
        assert_eq!(response["id"], label, "case: {label}");
        assert_eq!(response["error"]["code"], -32602, "case: {label}");
        assert_eq!(
            response["error"]["message"], "Invalid params",
            "case: {label}"
        );
        assert_eq!(
            response["error"]["data"]["field"], expected_field,
            "case: {label}"
        );
        let reason = response["error"]["data"]["reason"]
            .as_str()
            .unwrap_or_else(|| panic!("case {label} must include a reason"));
        assert!(
            reason.to_ascii_lowercase().contains(expected_reason_token),
            "case: {label}, reason: {reason}"
        );
        assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
        assert_eq!(mutation_count.get(), 0, "case: {label}");
        assert_eq!(save_count.get(), 0, "case: {label}");

        let valid_initialize = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": format!("valid-after-{label}"),
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"}
                }
            }))
            .unwrap();
        assert_eq!(
            valid_initialize["result"]["protocolVersion"], "2025-06-18",
            "case: {label}"
        );
        assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
        assert_eq!(mutation_count.get(), 0, "case: {label}");
        assert_eq!(save_count.get(), 0, "case: {label}");
    }
}

#[test]
fn initialize_paramsの_metaをobjectに限定する() {
    let repository = RecordingRepository::new(vec![]);
    let sync_clock_times = Rc::clone(&repository.sync_clock_times);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let save_count = Rc::clone(&repository.save_count);
    let mut server = McpServer::new(repository);

    let invalid_initialize = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "invalid-initialize-meta",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"},
                "_meta": false
            }
        }))
        .unwrap();
    assert_eq!(invalid_initialize["id"], "invalid-initialize-meta");
    assert_eq!(invalid_initialize["error"]["code"], -32602);
    assert_eq!(invalid_initialize["error"]["message"], "Invalid params");
    assert_eq!(invalid_initialize["error"]["data"]["field"], "params._meta");
    assert!(invalid_initialize["error"]["data"]["reason"]
        .as_str()
        .is_some_and(|reason| reason.to_ascii_lowercase().contains("object")));
    assert!(sync_clock_times.borrow().is_empty());
    assert_eq!(mutation_count.get(), 0);
    assert_eq!(save_count.get(), 0);

    let valid_initialize = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "valid-initialize-meta",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"},
                "_meta": {"trace": "test"},
                "vendorExtension": true
            }
        }))
        .unwrap();
    assert_eq!(valid_initialize["result"]["protocolVersion"], "2025-06-18");
    assert!(sync_clock_times.borrow().is_empty());
    assert_eq!(mutation_count.get(), 0);
    assert_eq!(save_count.get(), 0);
}

#[test]
#[allow(non_snake_case)]
fn initialize_paramsの_meta内progressTokenをstringまたはnumberに限定する() {
    let invalid_cases = [
        ("boolean", json!(false)),
        ("null", json!(null)),
        ("object", json!({})),
        ("array", json!([])),
    ];

    for (label, progress_token) in invalid_cases {
        let repository = RecordingRepository::new(vec![]);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = McpServer::new(repository);

        let response = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": format!("invalid-progress-token-{label}"),
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"},
                    "_meta": {"progressToken": progress_token}
                }
            }))
            .unwrap();

        assert_eq!(
            response["id"],
            format!("invalid-progress-token-{label}"),
            "case: {label}"
        );
        assert_eq!(response["error"]["code"], -32602, "case: {label}");
        assert_eq!(
            response["error"]["message"], "Invalid params",
            "case: {label}"
        );
        assert_eq!(
            response["error"]["data"]["field"], "params._meta.progressToken",
            "case: {label}"
        );
        let reason = response["error"]["data"]["reason"]
            .as_str()
            .unwrap_or_else(|| panic!("case {label} must include a reason"))
            .to_ascii_lowercase();
        assert!(reason.contains("string"), "case: {label}, reason: {reason}");
        assert!(reason.contains("number"), "case: {label}, reason: {reason}");
        assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
        assert_eq!(mutation_count.get(), 0, "case: {label}");
        assert_eq!(save_count.get(), 0, "case: {label}");

        let recovered = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": format!("valid-after-progress-token-{label}"),
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"},
                    "_meta": {"progressToken": "recovered", "vendorExtension": true}
                }
            }))
            .unwrap();
        assert_eq!(
            recovered["result"]["protocolVersion"], "2025-06-18",
            "case: {label}"
        );
        assert!(sync_clock_times.borrow().is_empty(), "case: {label}");
        assert_eq!(mutation_count.get(), 0, "case: {label}");
        assert_eq!(save_count.get(), 0, "case: {label}");
    }

    for (label, progress_token) in [("string", json!("token")), ("number", json!(1.5))] {
        let mut server = McpServer::new(TaskRepository::new(""));
        let response = server
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": format!("valid-progress-token-{label}"),
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-client", "version": "1.0"},
                    "_meta": {
                        "progressToken": progress_token,
                        "vendorExtension": {"enabled": true}
                    }
                }
            }))
            .unwrap();
        assert_eq!(
            response["result"]["protocolVersion"], "2025-06-18",
            "case: {label}"
        );
    }
}

#[test]
fn initialized通知はparamsの_metaをobjectに限定する() {
    let repository = RecordingRepository::new(vec![]);
    let sync_clock_times = Rc::clone(&repository.sync_clock_times);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let save_count = Rc::clone(&repository.save_count);
    let mut server = McpServer::new(repository);
    server.handle_request(initialize_request()).unwrap();

    assert_eq!(
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {"_meta": false}
        })),
        None
    );
    let before_valid_notification = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "before-valid-meta-notification",
            "method": "tools/list"
        }))
        .unwrap();
    assert_eq!(before_valid_notification["error"]["code"], -32002);

    assert_eq!(
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {
                "_meta": {"trace": "test"},
                "vendorExtension": true
            }
        })),
        None
    );
    let after_valid_notification = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "after-valid-meta-notification",
            "method": "tools/list"
        }))
        .unwrap();
    assert!(after_valid_notification["result"]["tools"].is_array());
    assert!(sync_clock_times.borrow().is_empty());
    assert_eq!(mutation_count.get(), 0);
    assert_eq!(save_count.get(), 0);
}

#[test]
#[allow(non_snake_case)]
fn initialized通知の_meta内progressTokenはbooleanも許容する() {
    let mut server = McpServer::new(TaskRepository::new(""));
    server.handle_request(initialize_request()).unwrap();

    assert_eq!(
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {
                "_meta": {
                    "progressToken": false,
                    "vendorExtension": [1, 2, 3]
                },
                "vendorExtension": true
            }
        })),
        None
    );
    let tools = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "tools-after-generic-notification-meta",
            "method": "tools/list"
        }))
        .unwrap();
    assert!(tools["result"]["tools"].is_array());
}

#[test]
fn 不正なinitialized通知ではlifecycleを進めない() {
    let mut server = McpServer::new(TaskRepository::new(""));
    server.handle_request(initialize_request()).unwrap();

    let malformed_notification = server
        .handle_request(json!({"method": "notifications/initialized"}))
        .expect("malformed notification must receive Invalid Request");
    assert_eq!(malformed_notification["jsonrpc"], "2.0");
    assert_eq!(malformed_notification["id"], serde_json::Value::Null);
    assert_eq!(malformed_notification["error"]["code"], -32600);
    assert_eq!(
        malformed_notification["error"]["message"],
        "Invalid Request"
    );

    assert_eq!(
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": []
        })),
        None
    );
    let before_valid_notification = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "before-valid-notification",
            "method": "tools/list"
        }))
        .unwrap();
    assert_eq!(before_valid_notification["error"]["code"], -32002);

    assert_eq!(
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {
                "_meta": {"trace": "test"},
                "vendorExtension": true
            }
        })),
        None
    );
    let after_valid_notification = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "after-valid-notification",
            "method": "tools/list"
        }))
        .unwrap();
    assert!(after_valid_notification["result"]["tools"].is_array());
}

#[test]
#[allow(non_snake_case)]
fn initializeはclientInfoとcapabilitiesの拡張fieldを許容する() {
    let mut server = McpServer::new(TaskRepository::new(""));

    let response = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "initialize-with-extensions",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {
                    "roots": {"listChanged": true, "vendorOption": true},
                    "sampling": {"vendorOption": true},
                    "elicitation": {"vendorOption": true},
                    "experimental": {"feature": {"enabled": true}},
                    "vendorCapability": true
                },
                "clientInfo": {
                    "name": "test-client",
                    "title": "Test Client",
                    "version": "1.0",
                    "vendorExtension": true
                },
                "_meta": {"trace": "test"},
                "vendorExtension": true
            }
        }))
        .unwrap();

    assert_eq!(response["id"], "initialize-with-extensions");
    assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn notification_未知methodには応答せずrepository_clockを同期もloadもしない() {
    let repository = RecordingRepository::new(vec![]);
    let load_count = Rc::clone(&repository.load_count);
    let sync_clock_times = Rc::clone(&repository.sync_clock_times);
    let mut server = McpServer::new(repository);
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/unknown"
    });

    assert_eq!(server.handle_request(notification), None);
    assert!(sync_clock_times.borrow().is_empty());
    assert_eq!(load_count.get(), 0);
}

#[test]
fn tools_list_initialized通知前は拒否する() {
    let mut server = McpServer::new(TaskRepository::new(""));
    let tools_list = json!({
        "jsonrpc": "2.0",
        "id": "before-initialized",
        "method": "tools/list"
    });

    let before_initialize = server.handle_request(tools_list.clone()).unwrap();
    assert_eq!(before_initialize["jsonrpc"], "2.0");
    assert_eq!(before_initialize["id"], "before-initialized");
    assert_eq!(before_initialize["error"]["code"], -32002);
    assert_eq!(
        before_initialize["error"]["message"],
        "Server not initialized"
    );

    assert_eq!(
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        })),
        None
    );
    let after_premature_notification = server.handle_request(tools_list.clone()).unwrap();
    assert_eq!(after_premature_notification["error"]["code"], -32002);

    server.handle_request(initialize_request()).unwrap();
    let before_notification = server.handle_request(tools_list).unwrap();
    assert_eq!(before_notification["error"]["code"], -32002);
}

#[test]
fn tools_list_initialized通知後に10個のtoolのschemaを返す() {
    let mut server = McpServer::new(TaskRepository::new(""));
    server.handle_request(initialize_request()).unwrap();
    assert_eq!(
        server.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        })),
        None
    );

    let response = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "tools-list",
            "method": "tools/list"
        }))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "tools-list");
    assert_eq!(
        response["result"]["tools"],
        json_fixture(
            include_str!("../../../tests/fixtures/mcp/tools-list.json"),
            &[]
        )
    );
    let tools = response["result"]["tools"].as_array().unwrap();
    let mut names = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    names.sort_unstable();
    let mut expected_names = vec![
        "get_focus",
        "get_task",
        "list_tasks",
        "get_schedule",
        "create_task",
        "breakdown_task",
        "defer_task",
        "defer_routine_task",
        "complete_task",
        "update_task",
    ];
    expected_names.sort_unstable();
    assert_eq!(names, expected_names);

    for tool in tools {
        assert!(!tool["description"].as_str().unwrap().is_empty());
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        for field in required_fields_for_tool(tool) {
            assert!(tool["inputSchema"]["properties"].get(field).is_some());
        }
    }

    assert_eq!(property_names(tools, "get_focus"), Vec::<&str>::new());
    assert_eq!(property_names(tools, "get_task"), vec!["task_id"]);
    assert_eq!(
        property_names(tools, "list_tasks"),
        vec!["categories", "period", "statuses"]
    );
    assert_eq!(property_names(tools, "get_schedule"), vec!["from", "until"]);
    assert_eq!(
        property_names(tools, "create_task"),
        vec!["estimated_work_minutes", "name", "pending_until"]
    );
    assert_eq!(
        property_names(tools, "breakdown_task"),
        vec!["names", "parent_id", "pending_until"]
    );
    assert_eq!(
        property_names(tools, "defer_task"),
        vec!["pending_until", "task_id"]
    );
    assert_eq!(property_names(tools, "defer_routine_task"), vec!["task_id"]);
    assert_eq!(
        property_names(tools, "complete_task"),
        vec!["additional_actual_work_seconds", "finished_at", "task_id"]
    );
    assert_eq!(
        property_names(tools, "update_task"),
        vec![
            "category",
            "deadline_time",
            "estimated_work_minutes",
            "task_id"
        ]
    );

    assert_eq!(required_fields(tools, "get_focus"), Vec::<&str>::new());
    assert_eq!(required_fields(tools, "get_task"), vec!["task_id"]);
    assert_eq!(required_fields(tools, "list_tasks"), Vec::<&str>::new());
    assert_eq!(required_fields(tools, "get_schedule"), Vec::<&str>::new());
    assert_eq!(required_fields(tools, "create_task"), vec!["name"]);
    assert_eq!(
        required_fields(tools, "breakdown_task"),
        vec!["names", "parent_id"]
    );
    assert_eq!(
        required_fields(tools, "defer_task"),
        vec!["pending_until", "task_id"]
    );
    assert_eq!(
        required_fields(tools, "defer_routine_task"),
        vec!["task_id"]
    );
    assert_eq!(required_fields(tools, "complete_task"), vec!["task_id"]);
    assert_eq!(required_fields(tools, "update_task"), vec!["task_id"]);

    assert_string_property(tools, "get_task", "task_id", Some("uuid"));
    assert_string_property(tools, "get_schedule", "from", Some("date"));
    assert_string_property(tools, "get_schedule", "until", Some("date"));
    assert_string_property(tools, "create_task", "name", None);
    assert_eq!(property(tools, "create_task", "name")["minLength"], 1);
    assert_non_negative_integer_property(tools, "create_task", "estimated_work_minutes");
    assert_string_property(tools, "create_task", "pending_until", Some("date-time"));
    assert_string_property(tools, "breakdown_task", "parent_id", Some("uuid"));
    let names_schema = property(tools, "breakdown_task", "names");
    assert_eq!(names_schema["type"], "array");
    assert_eq!(names_schema["items"]["type"], "string");
    assert_eq!(names_schema["items"]["minLength"], 1);
    assert_eq!(names_schema["minItems"], 1);
    assert_string_property(tools, "breakdown_task", "pending_until", Some("date-time"));
    assert_string_property(tools, "defer_task", "task_id", Some("uuid"));
    assert_string_property(tools, "defer_task", "pending_until", Some("date-time"));
    assert_string_property(tools, "defer_routine_task", "task_id", Some("uuid"));
    assert_string_property(tools, "complete_task", "task_id", Some("uuid"));
    assert_string_property(tools, "complete_task", "finished_at", Some("date-time"));
    assert_non_negative_integer_property(tools, "complete_task", "additional_actual_work_seconds");
    assert_eq!(
        property(tools, "complete_task", "additional_actual_work_seconds")["description"],
        "A non-negative number of seconds to add to the task's existing actual work. Omit it to add 0; the request fails if the resulting total overflows the supported integer range."
    );
    assert_string_property(tools, "update_task", "task_id", Some("uuid"));
    assert_non_negative_integer_property(tools, "update_task", "estimated_work_minutes");
    assert_nullable_string_property(tools, "update_task", "deadline_time", Some("date-time"));
    assert_nullable_string_property(tools, "update_task", "category", None);

    let period = property(tools, "list_tasks", "period");
    assert_eq!(period["type"], "object");
    assert_eq!(period["additionalProperties"], false);
    assert_eq!(
        sorted_strings(&period["required"]),
        vec!["field", "from", "until"]
    );
    assert_eq!(period["properties"]["field"]["type"], "string");
    assert_eq!(
        sorted_strings(&period["properties"]["field"]["enum"]),
        vec!["completed_at", "created_at", "deadline", "scheduled_start"]
    );
    assert_eq!(period["properties"]["from"]["type"], "string");
    assert_eq!(period["properties"]["from"]["format"], "date-time");
    assert_eq!(period["properties"]["until"]["type"], "string");
    assert_eq!(period["properties"]["until"]["format"], "date-time");
    let statuses = property(tools, "list_tasks", "statuses");
    assert_eq!(statuses["type"], "array");
    assert_eq!(statuses["items"]["type"], "string");
    assert_eq!(
        sorted_strings(&statuses["items"]["enum"]),
        vec!["done", "pending", "todo"]
    );
    let categories = property(tools, "list_tasks", "categories");
    assert_eq!(categories["type"], "array");
    assert_nullable_category_schema(&categories["items"]);
    assert_nullable_category_schema(property(tools, "update_task", "category"));

    let update_branches = tool(tools, "update_task")["inputSchema"]["anyOf"]
        .as_array()
        .unwrap();
    let mut update_fields = update_branches
        .iter()
        .map(|branch| sorted_strings(&branch["required"]))
        .collect::<Vec<_>>();
    update_fields.sort_unstable();
    assert_eq!(
        update_fields,
        vec![
            vec!["category"],
            vec!["deadline_time"],
            vec!["estimated_work_minutes"]
        ]
    );
}

#[test]
fn initialize_再送を拒否してlifecycleを維持する() {
    let mut server = McpServer::new(TaskRepository::new(""));
    server.handle_request(initialize_request()).unwrap();

    let before_initialized = server.handle_request(initialize_request()).unwrap();
    assert_eq!(before_initialized["jsonrpc"], "2.0");
    assert_eq!(before_initialized["id"], "initialize");
    assert_eq!(before_initialized["error"]["code"], -32600);
    assert_eq!(before_initialized["error"]["message"], "Invalid Request");

    server.handle_request(json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));
    let after_initialized = server.handle_request(initialize_request()).unwrap();
    assert_eq!(after_initialized["error"]["code"], -32600);

    let tools_list = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "after-reinitialize",
            "method": "tools/list"
        }))
        .unwrap();
    assert!(tools_list["result"]["tools"].is_array());
}
