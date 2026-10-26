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

/// Claude Code splits transcripts across sibling config roots (~/.claude for
/// personal, ~/.claude-work or another overlay for work accounts). Searching
/// only ~/.claude returns nothing for the other accounts, which a user reads as
/// "that conversation does not exist" rather than "I did not look there".
mod multi_config_root {
    use super::*;

    fn write_session(root: &std::path::Path, project: &str, session: &str, text: &str) {
        let dir = root.join("projects").join(project);
        fs::create_dir_all(&dir).expect("create project dir");
        // cwd encodes to the project dir name, so deep hits get a Resume line
        let cwd = format!("/home/user/{}", project.trim_start_matches("-home-user-"));
        let line = format!(
            "{{\"type\":\"user\",\"sessionId\":\"{session}\",\
             \"cwd\":\"{cwd}\",\
             \"timestamp\":\"2026-02-01T10:00:00Z\",\
             \"message\":{{\"role\":\"user\",\"content\":\
             [{{\"type\":\"text\",\"text\":\"{text}\"}}]}}}}\n"
        );
        fs::write(dir.join(format!("{session}.jsonl")), line).expect("write session");
    }

    /// Builds a synthetic home with a personal and a work root. Returns the
    /// tempdir, which must stay alive for the duration of the test.
    fn fake_home() -> tempfile::TempDir {
        let home = tempfile::tempdir().expect("tempdir");
        write_session(
            &home.path().join(".claude"),
            "-home-user-personal",
            "11111111-1111-1111-1111-111111111111",
            "personal transcript about zebrafish migration",
        );
        write_session(
            &home.path().join(".claude-work"),
            "-home-user-work-api",
            "22222222-2222-2222-2222-222222222222",
            "work transcript about zebrafish migration",
        );
        home
    }

    fn search(home: &std::path::Path, args: &[&str]) -> String {
        search_with_config_dir(home, None, args)
    }

    fn search_with_config_dir(
        home: &std::path::Path,
        config_dir: Option<&std::path::Path>,
        args: &[&str],
    ) -> String {
        let mut cmd = Command::new(binary_path());
        cmd.args(args).env("HOME", home);
        match config_dir {
            Some(dir) => cmd.env("CLAUDE_CONFIG_DIR", dir),
            None => cmd.env_remove("CLAUDE_CONFIG_DIR"),
        };
        let output = cmd.output().expect("Failed to run binary");
        assert!(output.status.success(), "binary failed: {output:?}");
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    /// The `Resume:` line printed for `session_id`
    fn resume_for(stdout: &str, session_id: &str) -> String {
        stdout
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("Resume:") && l.ends_with(session_id))
            .unwrap_or_else(|| panic!("no Resume line for {session_id}:\n{stdout}"))
            .to_string()
    }

    #[test]
    fn deep_search_reaches_both_personal_and_work_roots() {
        ensure_binary_built();
        let home = fake_home();
        let stdout = search(home.path(), &["zebrafish", "--deep", "--limit", "10"]);

        assert!(
            stdout.contains("11111111-1111-1111-1111-111111111111"),
            "personal root missing from results:\n{stdout}"
        );
        assert!(
            stdout.contains("22222222-2222-2222-2222-222222222222"),
            "work root missing from results; searching only ~/.claude \
             silently hides an entire account:\n{stdout}"
        );
    }

    #[test]
    fn a_low_limit_does_not_let_one_root_starve_the_others() {
        ensure_binary_built();
        let home = fake_home();
        // Give the first root more hits than the limit, so a plain
        // concatenation would fill --limit 2 from ~/.claude alone.
        for session in [
            "55555555-5555-5555-5555-555555555555",
            "66666666-6666-6666-6666-666666666666",
        ] {
            write_session(
                &home.path().join(".claude"),
                "-home-user-personal",
                session,
                "another personal transcript about zebrafish migration",
            );
        }
        let stdout = search(home.path(), &["zebrafish", "--deep", "--limit", "2"]);

        // Which personal session comes first depends on readdir order, so
        // only count them; the work root must get the other slot.
        let personal: usize = [
            "11111111-1111-1111-1111-111111111111",
            "55555555-5555-5555-5555-555555555555",
            "66666666-6666-6666-6666-666666666666",
        ]
        .iter()
        .map(|id| hits_for(&stdout, id))
        .sum();
        assert!(
            personal == 1 && hits_for(&stdout, "22222222-2222-2222-2222-222222222222") == 1,
            "both roots should be represented at --limit 2:\n{stdout}"
        );
    }

    #[test]
    fn project_filter_matches_a_project_that_exists_only_in_the_work_root() {
        ensure_binary_built();
        let home = fake_home();
        let stdout = search(
            home.path(),
            &["zebrafish", "--deep", "--project", "work-api"],
        );

        assert!(
            stdout.contains("22222222-2222-2222-2222-222222222222"),
            "--project must resolve across every root, not just the first:\n{stdout}"
        );
        assert!(
            !stdout.contains("11111111-1111-1111-1111-111111111111"),
            "--project must still exclude non-matching projects; a root with no \
             match must be skipped, not searched wholesale:\n{stdout}"
        );
    }

    /// Number of results printed for `session_id` (one `Session:` line each)
    fn hits_for(stdout: &str, session_id: &str) -> usize {
        stdout
            .lines()
            .filter(|l| l.trim() == format!("Session:  {session_id}"))
            .count()
    }

    fn write_index(root: &std::path::Path, project: &str, session: &str, summary: &str) {
        write_index_modified(root, project, session, summary, "2026-02-01T10:00:00Z");
    }

    fn write_index_modified(
        root: &std::path::Path,
        project: &str,
        session: &str,
        summary: &str,
        modified: &str,
    ) {
        let dir = root.join("projects").join(project);
        fs::create_dir_all(&dir).expect("create project dir");
        let index = serde_json::json!({
            "originalPath": format!("/home/user/{project}"),
            "entries": [{
                "sessionId": session,
                "firstPrompt": summary,
                "summary": summary,
                "messageCount": 1,
                "created": modified,
                "modified": modified,
                "projectPath": format!("/home/user/{project}"),
            }],
        });
        fs::write(dir.join("sessions-index.json"), index.to_string()).expect("write index");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_root_is_searched_once() {
        ensure_binary_built();
        let home = fake_home();
        std::os::unix::fs::symlink(
            home.path().join(".claude"),
            home.path().join(".claude-current"),
        )
        .expect("symlink root");

        let stdout = search(home.path(), &["zebrafish", "--deep", "--limit", "10"]);

        assert_eq!(
            hits_for(&stdout, "11111111-1111-1111-1111-111111111111"),
            1,
            "a symlinked alias of ~/.claude must not double its hits:\n{stdout}"
        );
    }

    #[test]
    fn a_backup_root_does_not_duplicate_sessions() {
        ensure_binary_built();
        let home = fake_home();
        for root in [".claude", ".claude.bak"] {
            write_session(
                &home.path().join(root),
                "-home-user-personal",
                "33333333-3333-3333-3333-333333333333",
                "backed up transcript about axolotl",
            );
            write_index(
                &home.path().join(root),
                "-home-user-personal",
                "33333333-3333-3333-3333-333333333333",
                "axolotl tank setup",
            );
        }

        // An identical copy made with `cp -r` is newer than the original;
        // it must still defer to ~/.claude.
        backdate(
            &home
                .path()
                .join(".claude/projects/-home-user-personal")
                .join("33333333-3333-3333-3333-333333333333.jsonl"),
        );

        for args in [
            &["axolotl", "--limit", "10"][..],
            &["axolotl", "--deep", "--limit", "10"][..],
        ] {
            let stdout = search(home.path(), args);
            let id = "33333333-3333-3333-3333-333333333333";
            assert_eq!(
                hits_for(&stdout, id),
                1,
                "a session must be reported once across roots:\n{stdout}"
            );
            let resume = resume_for(&stdout, id);
            assert!(
                !resume.contains("CLAUDE_CONFIG_DIR"),
                "the backup must not own the session, got: {resume}"
            );
        }
    }

    #[test]
    fn resume_command_targets_the_root_the_session_came_from() {
        ensure_binary_built();
        let home = fake_home();
        write_index(
            &home.path().join(".claude"),
            "-home-user-personal",
            "11111111-1111-1111-1111-111111111111",
            "zebrafish migration plan",
        );
        write_index(
            &home.path().join(".claude-work"),
            "-home-user-work-api",
            "22222222-2222-2222-2222-222222222222",
            "zebrafish migration plan",
        );

        for args in [
            &["zebrafish", "--limit", "10"][..],
            &["zebrafish", "--deep", "--limit", "10"][..],
        ] {
            let stdout = search(home.path(), args);
            let personal = resume_for(&stdout, "11111111-1111-1111-1111-111111111111");
            assert!(
                personal.contains("&& claude -r ") && !personal.contains("CLAUDE_CONFIG_DIR"),
                "the default root needs no override, got: {personal}"
            );
            assert!(
                resume_for(&stdout, "22222222-2222-2222-2222-222222222222")
                    .contains("&& CLAUDE_CONFIG_DIR=$HOME/.claude-work claude -r "),
                "a work-root session must resume with its own config dir:\n{stdout}"
            );
        }
    }

    #[test]
    fn claude_config_dir_outside_home_is_searched_and_resumed_without_override() {
        ensure_binary_built();
        let home = fake_home();
        let elsewhere = tempfile::tempdir().expect("tempdir");
        write_session(
            elsewhere.path(),
            "-home-user-ops",
            "44444444-4444-4444-4444-444444444444",
            "ops transcript about zebrafish migration",
        );

        let stdout = search_with_config_dir(
            home.path(),
            Some(elsewhere.path()),
            &["zebrafish", "--deep"],
        );

        for id in [
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        ] {
            assert_eq!(hits_for(&stdout, id), 1, "missing {id}:\n{stdout}");
        }
        assert_eq!(
            hits_for(&stdout, "44444444-4444-4444-4444-444444444444"),
            1,
            "CLAUDE_CONFIG_DIR outside $HOME must be searched:\n{stdout}"
        );
        let configured = resume_for(&stdout, "44444444-4444-4444-4444-444444444444");
        assert!(
            !configured.contains("CLAUDE_CONFIG_DIR"),
            "claude already reads $CLAUDE_CONFIG_DIR, got: {configured}"
        );
        assert!(
            resume_for(&stdout, "11111111-1111-1111-1111-111111111111")
                .contains("CLAUDE_CONFIG_DIR=$HOME/.claude claude -r "),
            "with CLAUDE_CONFIG_DIR set elsewhere, ~/.claude needs an override:\n{stdout}"
        );
    }

    #[test]
    fn claude_config_dir_pointing_at_a_home_root_is_not_searched_twice() {
        ensure_binary_built();
        let home = fake_home();
        let stdout = search_with_config_dir(
            home.path(),
            // Non-canonical spelling of ~/.claude-work
            Some(&home.path().join(".claude").join("..").join(".claude-work")),
            &["zebrafish", "--deep"],
        );

        assert_eq!(
            hits_for(&stdout, "22222222-2222-2222-2222-222222222222"),
            1,
            "{stdout}"
        );
    }

    #[test]
    fn index_hits_are_ranked_across_roots_not_grouped_by_root() {
        ensure_binary_built();
        let home = fake_home();
        write_index_modified(
            &home.path().join(".claude"),
            "-home-user-personal",
            "11111111-1111-1111-1111-111111111111",
            "zebrafish migration plan",
            "2026-02-01T10:00:00Z",
        );
        write_index_modified(
            &home.path().join(".claude-work"),
            "-home-user-work-api",
            "22222222-2222-2222-2222-222222222222",
            "zebrafish migration plan",
            "2026-02-05T10:00:00Z",
        );

        // Equal scores, so the newer work-root hit must outrank ~/.claude's
        let stdout = search(home.path(), &["zebrafish", "--limit", "1"]);

        assert!(stdout.contains("2 matches found"), "{stdout}");
        assert_eq!(
            hits_for(&stdout, "22222222-2222-2222-2222-222222222222"),
            1,
            "the newest hit should be shown first:\n{stdout}"
        );
    }

    #[test]
    fn project_filter_searches_every_matching_worktree_dir() {
        ensure_binary_built();
        let home = fake_home();
        write_session(
            &home.path().join(".claude-work"),
            "-home-user-work-api--claude-worktrees-feature-x",
            "77777777-7777-7777-7777-777777777777",
            "worktree transcript about zebrafish migration",
        );

        let stdout = search(
            home.path(),
            &["zebrafish", "--deep", "--project", "work-api"],
        );

        for id in [
            "22222222-2222-2222-2222-222222222222",
            "77777777-7777-7777-7777-777777777777",
        ] {
            assert_eq!(hits_for(&stdout, id), 1, "missing {id}:\n{stdout}");
        }
    }

    /// Make a transcript older than its copies, the way `cp -r` (without -p)
    /// leaves the original behind the backup it made
    fn backdate(path: &std::path::Path) {
        let day_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400);
        fs::File::options()
            .write(true)
            .open(path)
            .and_then(|f| f.set_modified(day_ago))
            .expect("backdate transcript");
    }

    /// A live session copied to ~/.claude.bak with `cp -r`, plus one session
    /// only the backup has.
    fn home_with_stale_backup(shared_text: &str) -> tempfile::TempDir {
        let home = fake_home();
        let shared = "88888888-8888-8888-8888-888888888888";
        for root in [".claude", ".claude.bak"] {
            write_session(
                &home.path().join(root),
                "-home-user-shared",
                shared,
                shared_text,
            );
        }
        backdate(
            &home
                .path()
                .join(".claude/projects/-home-user-shared")
                .join(format!("{shared}.jsonl")),
        );
        write_session(
            &home.path().join(".claude.bak"),
            "-home-user-shared",
            "99999999-9999-9999-9999-999999999999",
            "only in the backup: zebrafish migration",
        );
        home
    }

    #[test]
    fn a_stale_backup_does_not_take_over_a_live_session() {
        ensure_binary_built();
        // ~/.claude-backup is searched before ~/.claude-work and holds only
        // the stale copy, so it reaches the session first whatever order
        // the files are read in. The live copy has grown since, but has the
        // older mtime, as after `cp -r`; the longer copy must still win.
        let home = tempfile::tempdir().expect("tempdir");
        let shared = "88888888-8888-8888-8888-888888888888";
        for root in [".claude-backup", ".claude-work"] {
            write_session(
                &home.path().join(root),
                "-home-user-shared",
                shared,
                "shared transcript about zebrafish migration",
            );
            write_index(
                &home.path().join(root),
                "-home-user-shared",
                shared,
                "zebrafish migration plan",
            );
        }
        let live = home
            .path()
            .join(".claude-work/projects/-home-user-shared")
            .join(format!("{shared}.jsonl"));
        let line = fs::read_to_string(&live).unwrap();
        let later_turn = "{\"type\":\"system\",\"content\":\"a later turn\"}\n";
        fs::write(&live, format!("{line}{later_turn}")).unwrap();
        backdate(&live);

        for args in [
            &["zebrafish", "--limit", "10"][..],
            &["zebrafish", "--deep", "--limit", "10"][..],
        ] {
            let stdout = search(home.path(), args);
            assert_eq!(hits_for(&stdout, shared), 1, "{stdout}");
            let resume = resume_for(&stdout, shared);
            assert!(
                resume.contains("CLAUDE_CONFIG_DIR=$HOME/.claude-work claude -r "),
                "the longer, live copy must own the session, got: {resume}"
            );
        }
    }

    #[test]
    fn stale_backup_copies_do_not_use_up_the_backup_roots_limit() {
        ensure_binary_built();
        let home = home_with_stale_backup("shared transcript about zebrafish migration");
        let shared_file = |root: &str| {
            home.path()
                .join(root)
                .join("projects/-home-user-shared/88888888-8888-8888-8888-888888888888.jsonl")
        };
        // Two matching lines, so the stale copy alone could fill --limit 2
        for root in [".claude", ".claude.bak"] {
            let line = fs::read_to_string(shared_file(root)).unwrap();
            fs::write(shared_file(root), format!("{line}{line}")).unwrap();
        }
        backdate(&shared_file(".claude"));

        let stdout = search(
            home.path(),
            &["zebrafish", "--deep", "--project", "shared", "--limit", "2"],
        );

        assert_eq!(
            hits_for(&stdout, "99999999-9999-9999-9999-999999999999"),
            1,
            "the backup's own session must not be crowded out by its stale copy:\n{stdout}"
        );
    }

    #[test]
    fn resume_command_quotes_a_config_dir_with_a_space() {
        ensure_binary_built();
        let home = tempfile::tempdir().expect("tempdir");
        write_session(
            &home.path().join(".claude work"),
            "-home-user-spaced",
            "12121212-1212-1212-1212-121212121212",
            "spaced transcript about zebrafish migration",
        );

        let stdout = search(home.path(), &["zebrafish", "--deep"]);

        assert!(
            resume_for(&stdout, "12121212-1212-1212-1212-121212121212")
                .contains("CLAUDE_CONFIG_DIR=$HOME/'.claude work' claude -r "),
            "{stdout}"
        );
    }

    #[test]
    fn a_match_only_in_a_shorter_copy_is_still_shown_and_resumes_the_owner() {
        ensure_binary_built();
        // The copies diverged: only the shorter ~/.claude copy mentions the
        // query, the longer ~/.claude.bak copy doesn't. The match must not be
        // dropped just because the other root owns the session.
        let home = tempfile::tempdir().expect("tempdir");
        let id = "abababab-abab-abab-abab-abababababab";
        write_session(
            &home.path().join(".claude"),
            "-home-user-forked",
            id,
            "forked transcript about zebrafish migration",
        );
        write_session(
            &home.path().join(".claude.bak"),
            "-home-user-forked",
            id,
            "a much longer transcript that went on to talk about something else",
        );

        let stdout = search(home.path(), &["zebrafish", "--deep"]);

        assert_eq!(hits_for(&stdout, id), 1, "{stdout}");
        assert!(
            resume_for(&stdout, id).contains("CLAUDE_CONFIG_DIR=$HOME/.claude.bak claude -r "),
            "resume must target the owning (longer) copy:\n{stdout}"
        );
    }

    #[test]
    fn an_index_hit_found_only_in_a_backup_resumes_the_live_copy() {
        ensure_binary_built();
        // Only the backup's frozen index still mentions the query; the live
        // session has since moved on and grown.
        let home = tempfile::tempdir().expect("tempdir");
        let id = "cdcdcdcd-cdcd-cdcd-cdcd-cdcdcdcdcdcd";
        write_index(
            &home.path().join(".claude.bak"),
            "-home-user-moved",
            id,
            "axolotl tank setup",
        );
        write_session(
            &home.path().join(".claude.bak"),
            "-home-user-moved",
            id,
            "short",
        );
        write_index(
            &home.path().join(".claude"),
            "-home-user-moved",
            id,
            "aquarium lighting",
        );
        write_session(
            &home.path().join(".claude"),
            "-home-user-moved",
            id,
            "a longer live transcript that kept going",
        );

        let stdout = search(home.path(), &["axolotl"]);

        assert_eq!(hits_for(&stdout, id), 1, "{stdout}");
        let resume = resume_for(&stdout, id);
        assert!(
            !resume.contains("CLAUDE_CONFIG_DIR"),
            "resume must continue the live ~/.claude copy, got: {resume}"
        );
    }

    #[test]
    fn a_relative_claude_config_dir_is_spelled_out_in_the_resume_command() {
        ensure_binary_built();
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let id = "efefefef-efef-efef-efef-efefefefefef";
        write_session(
            &cwd.path().join("cfg"),
            "-home-user-rel",
            id,
            "relative transcript about zebrafish migration",
        );

        let output = Command::new(binary_path())
            .args(["zebrafish", "--deep"])
            .current_dir(cwd.path())
            .env("HOME", home.path())
            .env("CLAUDE_CONFIG_DIR", "cfg")
            .output()
            .expect("Failed to run binary");
        assert!(output.status.success(), "binary failed: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);

        // `cd` runs first, so a bare `cfg` would no longer resolve
        let absolute = fs::canonicalize(cwd.path()).unwrap().join("cfg");
        assert!(
            resume_for(&stdout, id).contains(&format!(
                "CLAUDE_CONFIG_DIR={} claude -r ",
                absolute.display()
            )),
            "{stdout}"
        );
    }

    #[test]
    fn resume_command_quotes_a_project_dir_with_a_space() {
        ensure_binary_built();
        let home = fake_home();
        write_index(
            &home.path().join(".claude"),
            "my app",
            "3c3c3c3c-3c3c-3c3c-3c3c-3c3c3c3c3c3c",
            "zebrafish migration plan",
        );

        let stdout = search(home.path(), &["zebrafish"]);

        assert!(
            resume_for(&stdout, "3c3c3c3c-3c3c-3c3c-3c3c-3c3c3c3c3c3c")
                .starts_with("Resume:   cd '/home/user/my app' && claude -r "),
            "{stdout}"
        );
    }

    #[test]
    fn a_stale_index_entry_resumes_the_root_that_has_the_transcript() {
        ensure_binary_built();
        // Only the backup's index lists the session; only ~/.claude still
        // has its transcript, so that is where claude can resume it.
        let home = tempfile::tempdir().expect("tempdir");
        let id = "5a5a5a5a-5a5a-5a5a-5a5a-5a5a5a5a5a5a";
        write_index(
            &home.path().join(".claude.bak"),
            "-home-user-moved",
            id,
            "axolotl tank setup",
        );
        write_session(
            &home.path().join(".claude"),
            "-home-user-moved",
            id,
            "the only transcript",
        );

        let stdout = search(home.path(), &["axolotl"]);

        let resume = resume_for(&stdout, id);
        assert!(
            !resume.contains("CLAUDE_CONFIG_DIR"),
            "resume must target the root holding the transcript, got: {resume}"
        );
    }

    #[test]
    fn resume_command_keeps_a_bare_home_dir_unquoted() {
        ensure_binary_built();
        let home = tempfile::tempdir().expect("tempdir");
        let id = "7e7e7e7e-7e7e-7e7e-7e7e-7e7e7e7e7e7e";
        let dir = home.path().join(".claude/projects/-home");
        fs::create_dir_all(&dir).unwrap();
        let index = serde_json::json!({
            "originalPath": home.path(),
            "entries": [{
                "sessionId": id,
                "firstPrompt": "zebrafish",
                "summary": "zebrafish migration plan",
                "messageCount": 1,
                "created": "2026-02-01T10:00:00Z",
                "modified": "2026-02-01T10:00:00Z",
                "projectPath": home.path(),
            }],
        });
        fs::write(dir.join("sessions-index.json"), index.to_string()).unwrap();

        let stdout = search(home.path(), &["zebrafish"]);

        assert!(
            resume_for(&stdout, id).starts_with("Resume:   cd ~ && claude -r "),
            "{stdout}"
        );
    }

    #[test]
    fn many_shadowed_copies_do_not_stop_a_backup_before_its_own_sessions() {
        ensure_binary_built();
        // Eight matching lines from sessions ~/.claude owns, more than the
        // backup may keep at --limit 2, plus one session only it has.
        let home = tempfile::tempdir().expect("tempdir");
        for n in 0..4 {
            let id = format!("{n}{n}{n}{n}{n}{n}{n}{n}-0000-0000-0000-000000000000");
            for root in [".claude", ".claude.bak"] {
                write_session(
                    &home.path().join(root),
                    "-home-user-many",
                    &id,
                    "zebrafish migration",
                );
                let file = home
                    .path()
                    .join(root)
                    .join("projects/-home-user-many")
                    .join(format!("{id}.jsonl"));
                let line = fs::read_to_string(&file).unwrap();
                fs::write(&file, format!("{line}{line}")).unwrap();
            }
        }
        write_session(
            &home.path().join(".claude.bak"),
            "-home-user-many",
            "99999999-9999-9999-9999-999999999999",
            "only in the backup: zebrafish migration",
        );

        let stdout = search(home.path(), &["zebrafish", "--deep", "--limit", "2"]);

        assert_eq!(
            hits_for(&stdout, "99999999-9999-9999-9999-999999999999"),
            1,
            "{stdout}"
        );
    }

    #[test]
    fn an_index_hit_from_a_backup_shows_the_owners_custom_title() {
        ensure_binary_built();
        let home = tempfile::tempdir().expect("tempdir");
        let id = "1e1e1e1e-1e1e-1e1e-1e1e-1e1e1e1e1e1e";
        write_index(
            &home.path().join(".claude.bak"),
            "-home-user-titled",
            id,
            "axolotl tank setup",
        );
        write_session(
            &home.path().join(".claude"),
            "-home-user-titled",
            id,
            "live transcript",
        );
        let live = home
            .path()
            .join(".claude/projects/-home-user-titled")
            .join(format!("{id}.jsonl"));
        let mut text = fs::read_to_string(&live).unwrap();
        text.push_str(&format!(
            "{{\"type\":\"custom-title\",\"customTitle\":\"Live title\",\"sessionId\":\"{id}\"}}\n"
        ));
        fs::write(&live, text).unwrap();

        let stdout = search(home.path(), &["axolotl"]);

        assert!(stdout.contains("Label:    Live title"), "{stdout}");
    }

    #[test]
    fn resume_command_quotes_an_odd_session_id() {
        ensure_binary_built();
        let home = tempfile::tempdir().expect("tempdir");
        write_index(
            &home.path().join(".claude"),
            "-home-user-odd",
            "odd;id",
            "zebrafish plan",
        );

        let stdout = search(home.path(), &["zebrafish"]);

        assert!(
            stdout.contains("&& claude -r 'odd;id'"),
            "a session ID with shell syntax must be quoted:\n{stdout}"
        );
    }

    #[test]
    fn a_nested_transcript_copy_resumes_the_longer_copy() {
        ensure_binary_built();
        // ~/.claude-backup is searched first, but ~/.claude-work's copy of
        // the nested transcript is longer, so it owns the session.
        let home = tempfile::tempdir().expect("tempdir");
        let id = "4d4d4d4d-4d4d-4d4d-4d4d-4d4d4d4d4d4d";
        for (root, extra) in [
            (".claude-backup", ""),
            (".claude-work", " and a later turn"),
        ] {
            let dir = home
                .path()
                .join(root)
                .join("projects/-home-user-nested/archive");
            fs::create_dir_all(&dir).unwrap();
            let line = format!(
                "{{\"type\":\"user\",\"sessionId\":\"{id}\",\"cwd\":\"/home/user/nested\",\
                 \"timestamp\":\"2026-02-01T10:00:00Z\",\"message\":{{\"role\":\"user\",\
                 \"content\":[{{\"type\":\"text\",\"text\":\"zebrafish migration{extra}\"}}]}}}}\n"
            );
            fs::write(dir.join(format!("{id}.jsonl")), line).unwrap();
        }

        let stdout = search(home.path(), &["zebrafish", "--deep"]);

        assert!(
            resume_for(&stdout, id).contains("CLAUDE_CONFIG_DIR=$HOME/.claude-work claude -r "),
            "{stdout}"
        );
    }
}

/// `--format jsonl` is a public schema: each row names the mode that produced
/// it (`kind`) and where its record lives, so a caller can open or resume it
/// without searching again.
mod jsonl_output {
    use super::*;
    use std::path::Path;
    use std::process::Stdio;

    const PERSONAL: &str = "11111111-1111-1111-1111-111111111111";
    const WORK: &str = "22222222-2222-2222-2222-222222222222";

    fn record(session: &str, kind: &str, uuid: Option<&str>, text: &str) -> String {
        let mut record = serde_json::json!({
            "type": kind,
            "sessionId": session,
            "cwd": "/home/user/app",
            "timestamp": "2026-02-01T10:00:00Z",
            "message": {"role": kind, "content": [{"type": "text", "text": text}]},
        });
        if let Some(uuid) = uuid {
            record["uuid"] = uuid.into();
        }
        format!("{record}\n")
    }

    /// A personal session whose match is on line 3 with a uuid, and a work
    /// session whose matching record has no uuid
    fn fake_home() -> tempfile::TempDir {
        let home = tempfile::tempdir().expect("tempdir");
        let personal = home.path().join(".claude/projects/-home-user-app");
        fs::create_dir_all(&personal).unwrap();
        fs::write(
            personal.join(format!("{PERSONAL}.jsonl")),
            [
                record(PERSONAL, "user", Some("u-1"), "set up the tank"),
                record(PERSONAL, "assistant", Some("u-2"), "the tank is ready"),
                record(PERSONAL, "user", Some("u-3"), "zebrafish migration plan"),
            ]
            .concat(),
        )
        .unwrap();
        let work = home.path().join(".claude-work/projects/-home-user-app");
        fs::create_dir_all(&work).unwrap();
        fs::write(
            work.join(format!("{WORK}.jsonl")),
            record(WORK, "user", None, "zebrafish migration at work"),
        )
        .unwrap();
        home
    }

    /// Run the binary with `HOME=home`; `path_env` replaces PATH, so an empty
    /// directory hides ripgrep and forces the Rust fallback
    fn run(home: &Path, args: &[&str], path_env: Option<&Path>) -> Vec<serde_json::Value> {
        let mut cmd = Command::new(binary_path());
        cmd.args(args)
            .args(["--format", "jsonl"])
            .env("HOME", home)
            .env_remove("CLAUDE_CONFIG_DIR");
        if let Some(path) = path_env {
            cmd.env("PATH", path);
        }
        let output = cmd.output().expect("Failed to run binary");
        assert!(output.status.success(), "binary failed: {output:?}");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| serde_json::from_str(line).expect("each line must be one JSON object"))
            .collect()
    }

    fn row_for<'a>(rows: &'a [serde_json::Value], session: &str) -> &'a serde_json::Value {
        rows.iter()
            .find(|row| row["session_id"] == session)
            .unwrap_or_else(|| panic!("no row for {session}: {rows:?}"))
    }

    fn path_str(path: PathBuf) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn deep_row_names_the_exact_record_with_and_without_ripgrep() {
        ensure_binary_built();
        let home = fake_home();
        let no_rg = tempfile::tempdir().expect("tempdir");
        let args = ["zebrafish", "--deep"];

        let with_rg = run(home.path(), &args, None);
        let without_rg = run(home.path(), &args, Some(no_rg.path()));

        for rows in [&with_rg, &without_rg] {
            let row = row_for(rows, PERSONAL);
            assert_eq!(row["kind"], "deep");
            assert_eq!(row["record_line"], 3, "{row}");
            assert_eq!(row["record_uuid"], "u-3", "{row}");
            assert_eq!(row["message_type"], "user");
            assert_eq!(
                row["transcript_path"],
                path_str(
                    home.path()
                        .join(format!(".claude/projects/-home-user-app/{PERSONAL}.jsonl"))
                )
            );
            assert_eq!(row["config_root"], path_str(home.path().join(".claude")));
            assert!(row.get("sessions_root").is_none(), "{row}");
        }
    }

    #[test]
    fn deep_row_names_its_own_config_root_and_a_null_uuid_when_absent() {
        ensure_binary_built();
        let home = fake_home();
        let rows = run(home.path(), &["zebrafish", "--deep"], None);

        let row = row_for(&rows, WORK);
        assert_eq!(
            row["config_root"],
            path_str(home.path().join(".claude-work"))
        );
        assert!(row["record_uuid"].is_null(), "{row}");
    }

    #[test]
    fn index_row_names_its_config_root_and_transcript_and_keeps_the_full_prompt() {
        ensure_binary_built();
        let home = fake_home();
        let prompt = format!("zebrafish {}", "x".repeat(300));
        fs::write(
            home.path()
                .join(".claude-work/projects/-home-user-app/sessions-index.json"),
            serde_json::json!({
                "originalPath": "/home/user/app",
                "entries": [{
                    "sessionId": WORK,
                    "firstPrompt": prompt,
                    "summary": "zebrafish plan",
                    "messageCount": 1,
                    "created": "2026-02-01T10:00:00Z",
                    "modified": "2026-02-01T10:00:00Z",
                    "projectPath": "/home/user/app",
                }],
            })
            .to_string(),
        )
        .unwrap();

        let rows = run(home.path(), &["zebrafish"], None);

        let row = row_for(&rows, WORK);
        assert_eq!(row["kind"], "index");
        assert_eq!(
            row["config_root"],
            path_str(home.path().join(".claude-work"))
        );
        assert_eq!(
            row["transcript_path"],
            path_str(
                home.path()
                    .join(format!(".claude-work/projects/-home-user-app/{WORK}.jsonl"))
            )
        );
        assert_eq!(row["first_prompt"], prompt, "JSON output must not truncate");
        assert!(row.get("config_dir").is_none(), "{row}");
    }

    #[test]
    fn openclaw_row_names_its_sessions_root() {
        ensure_binary_built();
        let home = tempfile::tempdir().expect("tempdir");
        let sessions = home.path().join(".openclaw/agents/main/sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::copy(
            fixtures_dir().join("openclaw-session.jsonl"),
            sessions.join("test-openclaw-1.jsonl"),
        )
        .unwrap();

        let rows = run(home.path(), &["configure", "--openclaw"], None);

        let row = row_for(&rows, "test-openclaw-1");
        assert_eq!(row["kind"], "openclaw");
        assert_eq!(row["sessions_root"], path_str(sessions));
        assert!(row.get("config_root").is_none(), "{row}");
        assert_eq!(row["record_line"], 2);
        assert!(row["record_uuid"].is_null(), "{row}");
    }

    #[test]
    fn obsidian_row_names_the_file_and_line() {
        ensure_binary_built();
        let home = tempfile::tempdir().expect("tempdir");
        let vault = fixtures_dir().join("obsidian-vault");

        let rows = run(
            home.path(),
            &["RBAC", "--obsidian", vault.to_str().unwrap()],
            None,
        );

        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0]["kind"], "obsidian");
        assert_eq!(rows[0]["file_path"], path_str(vault.join("Projects.md")));
        assert_eq!(rows[0]["line_number"], 15);
    }

    #[test]
    fn a_reader_closing_the_pipe_early_is_not_a_crash() {
        ensure_binary_built();
        let home = fake_home();
        let mut child = Command::new(binary_path())
            .args(["zebrafish", "--deep", "--format", "jsonl"])
            .env("HOME", home.path())
            .env_remove("CLAUDE_CONFIG_DIR")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to run binary");
        // Close the read end before the first row is written, like `| head -0`
        drop(child.stdout.take());
        let output = child.wait_with_output().expect("wait");

        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
    }
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
