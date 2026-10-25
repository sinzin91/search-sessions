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
        assert!(stdout.contains("Search Claude Code"));
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

mod date_range_cli {
    use super::*;

    #[test]
    fn test_help_shows_date_flags() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .arg("--help")
            .output()
            .expect("Failed to run binary");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("--since"), "Help should mention --since");
        assert!(stdout.contains("--until"), "Help should mention --until");
        assert!(stdout.contains("--date"), "Help should mention --date");
    }

    #[test]
    fn test_invalid_date_error() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .args(["test", "--since", "not-a-date"])
            .output()
            .expect("Failed to run binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Cannot parse date"),
            "Should show parse error, got: {stderr}"
        );
    }

    #[test]
    fn test_date_flag_accepted() {
        ensure_binary_built();

        // --date with a valid date should not error on date parsing
        // (may error on missing sessions dir, which is fine)
        let output = Command::new(binary_path())
            .args(["test", "--date", "2026-02-01"])
            .output()
            .expect("Failed to run binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("Cannot parse date"),
            "Valid date should not cause parse error"
        );
    }

    #[test]
    fn test_relative_date_accepted() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .args(["test", "--since", "yesterday"])
            .output()
            .expect("Failed to run binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("Cannot parse date"),
            "Relative date 'yesterday' should be accepted"
        );
    }

    #[test]
    fn test_days_ago_accepted() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .args(["test", "--since", "3 days ago"])
            .output()
            .expect("Failed to run binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("Cannot parse date"),
            "'3 days ago' should be accepted"
        );
    }

    #[test]
    fn test_last_week_accepted() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .args(["test", "--since", "last week"])
            .output()
            .expect("Failed to run binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("Cannot parse date"),
            "'last week' should be accepted"
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

mod obsidian_search {
    use super::*;

    fn obsidian_vault_dir() -> PathBuf {
        fixtures_dir().join("obsidian-vault")
    }

    #[test]
    fn test_obsidian_vault_fixture_exists() {
        let vault = obsidian_vault_dir();
        assert!(vault.exists(), "Obsidian vault fixture should exist");
        assert!(vault.is_dir(), "Obsidian vault should be a directory");
    }

    #[test]
    fn test_obsidian_vault_has_markdown_files() {
        let vault = obsidian_vault_dir();
        let daily = vault.join("Daily Notes.md");
        let projects = vault.join("Projects.md");

        assert!(daily.exists(), "Daily Notes.md should exist");
        assert!(projects.exists(), "Projects.md should exist");
    }

    #[test]
    fn test_obsidian_vault_content() {
        let daily_content = fs::read_to_string(obsidian_vault_dir().join("Daily Notes.md"))
            .expect("Failed to read");
        let projects_content =
            fs::read_to_string(obsidian_vault_dir().join("Projects.md")).expect("Failed to read");

        // Verify searchable content
        assert!(daily_content.contains("authentication"));
        assert!(daily_content.contains("OAuth2"));
        assert!(daily_content.contains("Kubernetes"));
        assert!(daily_content.contains("Redis"));

        assert!(projects_content.contains("Obsidian"));
        assert!(projects_content.contains("RBAC"));
    }

    #[test]
    fn test_help_shows_obsidian_flag() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .arg("--help")
            .output()
            .expect("Failed to run binary");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("--obsidian"),
            "Help should mention --obsidian flag"
        );
    }

    #[test]
    fn test_obsidian_search_finds_results() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .args([
                "--obsidian",
                obsidian_vault_dir().to_str().unwrap(),
                "authentication",
            ])
            .output()
            .expect("Failed to run binary");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("OBSIDIAN SEARCH"),
            "Should show Obsidian search header, got: {stdout}"
        );
        assert!(
            stdout.contains("matches found"),
            "Should find matches, got: {stdout}"
        );
    }

    #[test]
    fn test_obsidian_search_and_semantics() {
        ensure_binary_built();

        // Search for "Kubernetes RBAC" - both terms must match
        let output = Command::new(binary_path())
            .args([
                "--obsidian",
                obsidian_vault_dir().to_str().unwrap(),
                "Kubernetes",
                "RBAC",
            ])
            .output()
            .expect("Failed to run binary");

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Should find in Projects.md where both terms appear
        assert!(
            stdout.contains("matches found"),
            "Should find AND matches, got: {stdout}"
        );
    }

    #[test]
    fn test_obsidian_missing_vault_error() {
        ensure_binary_built();

        let output = Command::new(binary_path())
            .args(["--obsidian", "/nonexistent/vault", "test"])
            .output()
            .expect("Failed to run binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("not found") || stderr.contains("ERROR"),
            "Should show error for missing vault, got: {stderr}"
        );
    }

    #[test]
    fn test_obsidian_file_instead_of_directory_error() {
        ensure_binary_built();

        // Pass a file instead of a directory
        let file_path = obsidian_vault_dir().join("Daily Notes.md");
        let output = Command::new(binary_path())
            .args(["--obsidian", file_path.to_str().unwrap(), "test"])
            .output()
            .expect("Failed to run binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("not a directory") || stderr.contains("ERROR"),
            "Should show error for file instead of directory, got: {stderr}"
        );
    }
}

mod custom_title_cli {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};

    const PROJECT_DIR: &str = "-home-user-projects-zephyr";

    fn index_entry(session_id: &str, summary: &str, modified: &str) -> serde_json::Value {
        serde_json::json!({
            "sessionId": session_id,
            "firstPrompt": "zephyr widget",
            "summary": summary,
            "messageCount": 2,
            "created": modified,
            "modified": modified,
            "gitBranch": "main",
            "projectPath": "/home/user/projects/zephyr"
        })
    }

    /// Lay out a fake `$HOME/.claude/projects/<project>/` with an index and
    /// the given session transcripts copied from fixtures.
    fn fake_home(entries: &[serde_json::Value], sessions: &[(&str, &str)]) -> tempfile::TempDir {
        let home = tempfile::tempdir().expect("Failed to create temp HOME");
        let project = home.path().join(".claude/projects").join(PROJECT_DIR);
        fs::create_dir_all(&project).unwrap();

        let index = serde_json::json!({
            "originalPath": "/home/user/projects/zephyr",
            "entries": entries,
        });
        fs::write(project.join("sessions-index.json"), index.to_string()).unwrap();

        for (session_id, fixture) in sessions {
            fs::copy(
                fixtures_dir().join(fixture),
                project.join(format!("{session_id}.jsonl")),
            )
            .unwrap();
        }
        home
    }

    fn default_home() -> tempfile::TempDir {
        fake_home(
            &[
                index_entry(
                    "titled-session",
                    "Zephyr widget setup",
                    "2026-03-01T09:00:30Z",
                ),
                index_entry(
                    "untitled-session",
                    "Zephyr widget teardown",
                    "2026-03-02T09:00:30Z",
                ),
            ],
            &[
                ("titled-session", "custom-title-session.jsonl"),
                ("untitled-session", "untitled-session.jsonl"),
            ],
        )
    }

    fn run(home: &Path, args: &[&str]) -> String {
        run_with_path(home, args, &std::env::var_os("PATH").unwrap_or_default())
    }

    fn run_with_path(home: &Path, args: &[&str], path: &std::ffi::OsStr) -> String {
        ensure_binary_built();
        let output = Command::new(binary_path())
            .args(args)
            .env("HOME", home)
            .env("PATH", path)
            .output()
            .expect("Failed to run binary");
        assert!(output.status.success(), "binary failed: {output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// The `Label:` line printed directly under the result whose header contains `header`.
    /// Panics if no such result was printed, so `None` always means "no label".
    fn label_for<'a>(stdout: &'a str, header: &str) -> Option<&'a str> {
        let mut lines = stdout.lines().skip_while(|l| !l.contains(header));
        lines
            .next()
            .unwrap_or_else(|| panic!("no result {header:?} in: {stdout}"));
        lines
            .next()
            .and_then(|l| l.trim().strip_prefix("Label:"))
            .map(str::trim)
    }

    #[test]
    fn test_index_search_shows_last_custom_title() {
        let home = default_home();
        let stdout = run(home.path(), &["zephyr", "widget"]);

        assert!(stdout.contains("2 matches found"), "got: {stdout}");
        assert_eq!(
            label_for(&stdout, "Zephyr widget setup"),
            Some("Final title"),
            "got: {stdout}"
        );
        assert_eq!(label_for(&stdout, "Zephyr widget teardown"), None);
        assert!(!stdout.contains("First title"));
        assert!(!stdout.contains("Decoy title"));
    }

    #[test]
    fn test_deep_search_shows_last_custom_title() {
        let home = default_home();
        let stdout = run(home.path(), &["--deep", "configured"]);

        assert!(stdout.contains("1 matches found"), "got: {stdout}");
        assert_eq!(
            label_for(&stdout, "Zephyr widget setup"),
            Some("Final title"),
            "got: {stdout}"
        );
        assert!(!stdout.contains("First title"));
        assert!(!stdout.contains("Decoy title"));
    }

    #[test]
    fn test_index_search_ignores_session_id_outside_project_dir() {
        let home = fake_home(
            &[index_entry(
                "../-other-project/titled-session",
                "Zephyr widget setup",
                "2026-03-01T09:00:30Z",
            )],
            &[],
        );
        let other = home.path().join(".claude/projects/-other-project");
        fs::create_dir_all(&other).unwrap();
        fs::copy(
            fixtures_dir().join("custom-title-session.jsonl"),
            other.join("titled-session.jsonl"),
        )
        .unwrap();

        let stdout = run(home.path(), &["zephyr", "widget"]);

        assert_eq!(label_for(&stdout, "Zephyr widget setup"), None);
    }

    #[test]
    fn test_custom_title_survives_invalid_utf8_line() {
        let home = default_home();
        let transcript = home
            .path()
            .join(".claude/projects")
            .join(PROJECT_DIR)
            .join("titled-session.jsonl");
        let mut bytes = b"{\"type\":\"user\",\"text\":\"\xff\xfe\"}\n".to_vec();
        bytes.extend(fs::read(&transcript).unwrap());
        fs::write(&transcript, bytes).unwrap();

        let stdout = run(home.path(), &["zephyr", "widget"]);

        assert_eq!(
            label_for(&stdout, "Zephyr widget setup"),
            Some("Final title"),
            "got: {stdout}"
        );
    }

    /// An empty PATH hides ripgrep, forcing the pure-Rust deep search
    #[test]
    fn test_deep_search_rust_fallback_shows_last_custom_title() {
        let home = default_home();
        let empty_path = tempfile::tempdir().unwrap();
        let stdout = run_with_path(
            home.path(),
            &["--deep", "configured"],
            empty_path.path().as_os_str(),
        );

        assert!(stdout.contains("1 matches found"), "got: {stdout}");
        assert_eq!(
            label_for(&stdout, "Zephyr widget setup"),
            Some("Final title"),
            "got: {stdout}"
        );
    }

    #[test]
    fn test_deep_search_session_without_title_has_no_label() {
        let home = default_home();
        let stdout = run(home.path(), &["--deep", "torn", "down"]);

        assert!(stdout.contains("1 matches found"), "got: {stdout}");
        assert_eq!(label_for(&stdout, "Zephyr widget teardown"), None);
        assert!(!stdout.contains("Label:"), "got: {stdout}");
    }

    /// Index mode must only open transcripts for the results it displays.
    /// The non-displayed session's transcript is a FIFO with no writer, so
    /// opening it by path would block the binary forever.
    #[cfg(unix)]
    #[test]
    fn test_index_search_does_not_read_undisplayed_transcripts() {
        let home = fake_home(
            &[
                index_entry(
                    "titled-session",
                    "Zephyr widget setup",
                    "2026-03-02T09:00:30Z",
                ),
                index_entry(
                    "fifo-session",
                    "Zephyr widget setup",
                    "2026-03-01T09:00:30Z",
                ),
            ],
            &[("titled-session", "custom-title-session.jsonl")],
        );
        let fifo = home
            .path()
            .join(".claude/projects")
            .join(PROJECT_DIR)
            .join("fifo-session.jsonl");
        let status = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(status.success(), "mkfifo failed");

        ensure_binary_built();
        let mut child = Command::new(binary_path())
            .args(["--limit", "1", "zephyr", "widget"])
            .env("HOME", home.path())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to run binary");

        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                child.kill().unwrap();
                panic!("index search blocked opening an undisplayed transcript");
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("showing top 1"), "got: {stdout}");
        assert_eq!(
            label_for(&stdout, "Zephyr widget setup"),
            Some("Final title"),
            "got: {stdout}"
        );
    }
}
