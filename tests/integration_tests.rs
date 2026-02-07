use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/search-sessions")
}

/// Build the binary before running tests
fn ensure_binary_built() {
    let status = Command::new("cargo")
        .args(["build"])
        .status()
        .expect("Failed to build binary");
    assert!(status.success(), "Binary build failed");
}

mod index_parsing {
    use super::*;

    #[test]
    fn test_sessions_index_parsing() {
        let index_path = fixtures_dir().join("sessions-index.json");
        let content = fs::read_to_string(&index_path).expect("Failed to read index file");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("Failed to parse JSON");

        assert!(parsed["entries"].is_array());
        assert_eq!(parsed["entries"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["entries"][0]["sessionId"], "test-session-1");
        assert_eq!(
            parsed["entries"][0]["summary"],
            "Discussing Kubernetes RBAC configuration"
        );
    }
}

mod claude_session_parsing {
    use super::*;

    #[test]
    fn test_claude_session_format() {
        let session_path = fixtures_dir().join("claude-session.jsonl");
        let content = fs::read_to_string(&session_path).expect("Failed to read session file");

        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 5);

        // Parse first message (summary)
        let summary: serde_json::Value =
            serde_json::from_str(lines[0]).expect("Failed to parse summary");
        assert_eq!(summary["type"], "summary");

        // Parse user message
        let user_msg: serde_json::Value =
            serde_json::from_str(lines[1]).expect("Failed to parse user message");
        assert_eq!(user_msg["type"], "user");
        assert_eq!(user_msg["sessionId"], "test-session-1");

        // Parse assistant message
        let asst_msg: serde_json::Value =
            serde_json::from_str(lines[2]).expect("Failed to parse assistant message");
        assert_eq!(asst_msg["type"], "assistant");
    }

    #[test]
    fn test_claude_message_content_extraction() {
        let session_path = fixtures_dir().join("claude-session.jsonl");
        let content = fs::read_to_string(&session_path).expect("Failed to read session file");

        let lines: Vec<&str> = content.lines().collect();
        let user_msg: serde_json::Value = serde_json::from_str(lines[1]).expect("Failed to parse");

        let message = &user_msg["message"];
        let content_arr = message["content"]
            .as_array()
            .expect("Content should be array");
        assert_eq!(content_arr.len(), 1);
        assert_eq!(content_arr[0]["type"], "text");
        assert!(content_arr[0]["text"].as_str().unwrap().contains("RBAC"));
    }
}

mod openclaw_session_parsing {
    use super::*;

    #[test]
    fn test_openclaw_session_format() {
        let session_path = fixtures_dir().join("openclaw-session.jsonl");
        let content = fs::read_to_string(&session_path).expect("Failed to read session file");

        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 5);

        // Parse session header
        let header: serde_json::Value =
            serde_json::from_str(lines[0]).expect("Failed to parse header");
        assert_eq!(header["type"], "session");
        assert_eq!(header["id"], "test-openclaw-1");
        assert_eq!(header["cwd"], "/home/user/projects/myapp");

        // Parse message
        let msg: serde_json::Value =
            serde_json::from_str(lines[1]).expect("Failed to parse message");
        assert_eq!(msg["type"], "message");
        assert_eq!(msg["message"]["role"], "user");
    }

    #[test]
    fn test_openclaw_message_content_extraction() {
        let session_path = fixtures_dir().join("openclaw-session.jsonl");
        let content = fs::read_to_string(&session_path).expect("Failed to read session file");

        let lines: Vec<&str> = content.lines().collect();
        let msg: serde_json::Value = serde_json::from_str(lines[1]).expect("Failed to parse");

        let role = msg["message"]["role"]
            .as_str()
            .expect("Role should be string");
        assert_eq!(role, "user");

        let content_arr = msg["message"]["content"]
            .as_array()
            .expect("Content should be array");
        assert!(
            content_arr[0]["text"]
                .as_str()
                .unwrap()
                .contains("security audit")
        );
    }
}

mod cli_integration {
    use super::*;

    #[test]
    fn test_help_flag() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .arg("--help")
            .output()
            .expect("Failed to run binary");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Search Claude Code or OpenClaw session history"));
        assert!(stdout.contains("--openclaw"));
        assert!(stdout.contains("--deep"));
        assert!(stdout.contains("--limit"));
    }

    #[test]
    fn test_empty_query_error() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .output()
            .expect("Failed to run binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("No search query provided") || stderr.contains("error"));
    }

    #[test]
    fn test_missing_directory_error() {
        ensure_binary_built();

        // This will fail because ~/.claude/projects doesn't exist in CI
        let output = Command::new(binary_path())
            .args(["test", "query"])
            .output()
            .expect("Failed to run binary");

        // Should either work (if dir exists) or show helpful error
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Either finds results or shows error about missing directory
        assert!(
            stdout.contains("matches found")
                || stderr.contains("not found")
                || stderr.contains("ERROR")
        );
    }
}

mod query_matching {
    use super::*;

    #[test]
    fn test_and_semantics_in_fixtures() {
        // Verify our test data contains expected terms for AND matching
        let claude_content = fs::read_to_string(fixtures_dir().join("claude-session.jsonl"))
            .expect("Failed to read");
        let openclaw_content = fs::read_to_string(fixtures_dir().join("openclaw-session.jsonl"))
            .expect("Failed to read");

        // Claude session should have both "Kubernetes" and "RBAC"
        assert!(claude_content.contains("Kubernetes"));
        assert!(claude_content.contains("RBAC"));

        // OpenClaw session should have both "security" and "audit"
        assert!(openclaw_content.contains("security"));
        assert!(openclaw_content.contains("audit"));
    }
}
