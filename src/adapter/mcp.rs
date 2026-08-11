#[cfg(test)]
mod tests {
    use super::McpServer;
    use crate::adapter::gateway::task_repository::TaskRepository;
    use serde_json::json;

    #[test]
    fn initialize_server情報とtool能力を返す() {
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
    fn initialize_非対応version要求にはserver対応versionを返す() {
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
    fn request_未知methodにはmethod_not_foundを返す() {
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
    fn notification_未知methodには応答しない() {
        let mut server = McpServer::new(TaskRepository::new(""));
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/unknown"
        });

        assert_eq!(server.handle_request(notification), None);
    }
}
