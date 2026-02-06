use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, FixedOffset};
use clap::Parser;
use serde::Deserialize;

// ─── Constants ──────────────────────────────────────────────────────

const MAX_SNIPPET_LEN: usize = 200;
const DEFAULT_LIMIT: usize = 20;
const MAX_MATCHES_PER_SESSION: usize = 2;

// ─── CLI ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "search-sessions", about = "Search Claude Code or OpenClaw session history")]
struct Cli {
    /// Search query (words are ANDed together)
    query: Vec<String>,

    /// Search full message content (slower)
    #[arg(long)]
    deep: bool,

    /// Search OpenClaw sessions instead of Claude Code
    #[arg(long)]
    openclaw: bool,

    /// Maximum results to show
    #[arg(long, default_value_t = DEFAULT_LIMIT)]
    limit: usize,

    /// Filter to sessions from projects matching this substring
    #[arg(long)]
    project: Option<String>,

    /// OpenClaw agent to search (default: main)
    #[arg(long, default_value = "main")]
    agent: String,
}

// ─── Data Structures ────────────────────────────────────────────────

struct IndexMatch {
    session_id: String,
    project_path: String,
    first_prompt: String,
    summary: String,
    git_branch: String,
    created: String,
    modified: String,
    message_count: u64,
    matched_field: String,
    score: f64,
}

struct DeepMatch {
    session_id: String,
    project_path: String,
    message_type: String,
    snippet: String,
    timestamp: String,
    summary: Option<String>,
    first_prompt: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionIndex {
    #[serde(default)]
    original_path: String,
    #[serde(default)]
    entries: Vec<SessionIndexEntry>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SessionIndexEntry {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    first_prompt: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    message_count: u64,
    #[serde(default)]
    created: String,
    #[serde(default)]
    modified: String,
    #[serde(default)]
    git_branch: String,
    #[serde(default)]
    project_path: String,
}

/// OpenClaw session metadata extracted from session header
struct OpenClawSessionMeta {
    cwd: String,
    timestamp: String,
}

// ─── Helpers ────────────────────────────────────────────────────────

fn claude_projects_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".claude")
        .join("projects")
}

fn openclaw_sessions_dir(agent: &str) -> PathBuf {
    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".openclaw")
        .join("agents")
        .join(agent)
        .join("sessions")
}

fn format_date(iso_str: &str) -> String {
    if iso_str.is_empty() {
        return "unknown".to_string();
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(iso_str) {
        return dt.format("%Y-%m-%d %H:%M").to_string();
    }
    // Try with Z suffix normalization
    let normalized = iso_str.replace('Z', "+00:00");
    if let Ok(dt) = DateTime::<FixedOffset>::parse_from_rfc3339(&normalized) {
        return dt.format("%Y-%m-%d %H:%M").to_string();
    }
    // Fallback: return first 16 chars
    iso_str.chars().take(16).collect()
}

fn format_project_path(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if let Some(rest) = path.strip_prefix(home_str.as_ref()) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        s.chars().take(max_len).collect()
    }
}

// ─── Index Search (Claude Code only) ────────────────────────────────

fn find_all_index_files(base: &Path) -> Vec<PathBuf> {
    let pattern = format!("{}/*/sessions-index.json", base.display());
    let mut files: Vec<PathBuf> = glob::glob(&pattern)
        .unwrap_or_else(|_| panic!("Invalid glob pattern"))
        .filter_map(|r| r.ok())
        .collect();
    files.sort();
    files
}

fn load_index(path: &Path) -> (String, Vec<SessionIndexEntry>) {
    let data = match fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => return (String::new(), vec![]),
    };
    let index: SessionIndex = match serde_json::from_str(&data) {
        Ok(i) => i,
        Err(_) => return (String::new(), vec![]),
    };
    let original_path = if index.original_path.is_empty() {
        path.parent()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        index.original_path
    };
    (original_path, index.entries)
}

fn score_index_entry(entry: &SessionIndexEntry, query_terms: &[&str]) -> (f64, String) {
    let fields: &[(&str, &str, f64)] = &[
        ("summary", &entry.summary, 3.0),
        ("firstPrompt", &entry.first_prompt, 2.0),
        ("gitBranch", &entry.git_branch, 1.0),
        ("projectPath", &entry.project_path, 1.0),
    ];

    let mut total_score = 0.0;
    let mut best_field = String::new();
    let mut best_field_score = 0.0;

    for term in query_terms {
        let term_lower = term.to_lowercase();
        let mut term_found = false;

        for &(field_name, field_value, weight) in fields {
            if field_value.to_lowercase().contains(&term_lower) {
                term_found = true;
                total_score += weight;
                if weight > best_field_score {
                    best_field_score = weight;
                    best_field = field_name.to_string();
                }
            }
        }

        if !term_found {
            return (0.0, String::new());
        }
    }

    (total_score, best_field)
}

fn search_index(
    query: &str,
    project_filter: Option<&str>,
    base: &Path,
) -> Vec<IndexMatch> {
    let query_terms: Vec<&str> = query.split_whitespace().collect();
    let mut matches = Vec::new();

    for index_path in find_all_index_files(base) {
        let (original_path, entries) = load_index(&index_path);

        if let Some(filter) = project_filter {
            if !original_path.to_lowercase().contains(&filter.to_lowercase()) {
                continue;
            }
        }

        for entry in &entries {
            let (score, matched_field) = score_index_entry(entry, &query_terms);
            if score > 0.0 {
                matches.push(IndexMatch {
                    session_id: entry.session_id.clone(),
                    project_path: if entry.project_path.is_empty() {
                        original_path.clone()
                    } else {
                        entry.project_path.clone()
                    },
                    first_prompt: truncate(&entry.first_prompt, MAX_SNIPPET_LEN),
                    summary: entry.summary.clone(),
                    git_branch: entry.git_branch.clone(),
                    created: entry.created.clone(),
                    modified: entry.modified.clone(),
                    message_count: entry.message_count,
                    matched_field,
                    score,
                });
            }
        }
    }

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.modified.cmp(&a.modified))
    });

    matches
}

// ─── Deep Search ────────────────────────────────────────────────────

fn resolve_search_path(base: &Path, project_filter: Option<&str>) -> PathBuf {
    if let Some(filter) = project_filter {
        let filter_lower = filter.to_lowercase();
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                if entry.path().is_dir()
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&filter_lower)
                {
                    return entry.path();
                }
            }
        }
    }
    base.to_path_buf()
}

/// Extract text from Claude Code message format
/// Record has: {"type": "user"|"assistant", "message": {"content": ...}}
fn extract_text_claude(value: &serde_json::Value) -> String {
    let Some(message) = value.get("message") else {
        return String::new();
    };
    let Some(content) = message.get("content") else {
        return String::new();
    };

    extract_content_array(content)
}

/// Extract text from OpenClaw message format
/// Record has: {"type": "message", "message": {"role": "user"|"assistant", "content": ...}}
fn extract_text_openclaw(value: &serde_json::Value) -> (String, String) {
    let Some(message) = value.get("message") else {
        return (String::new(), String::new());
    };
    
    let role = message
        .get("role")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();
    
    let Some(content) = message.get("content") else {
        return (role, String::new());
    };

    (role, extract_content_array(content))
}

/// Shared content array extraction
fn extract_content_array(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut texts = Vec::new();
            for item in arr {
                if let Some(t) = item.get("type").and_then(|t| t.as_str()) {
                    match t {
                        "text" => {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                texts.push(text.to_string());
                            }
                        }
                        "tool_result" => {
                            if let Some(c) = item.get("content") {
                                texts.push(c.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            texts.join(" ")
        }
        _ => content.to_string(),
    }
}

fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn get_snippet(text: &str, query: &str, context_chars: usize) -> String {
    let text_lower = text.to_lowercase();
    let query_lower = query.to_lowercase();

    let mut idx = text_lower.find(&query_lower);
    if idx.is_none() {
        for term in query.split_whitespace() {
            idx = text_lower.find(&term.to_lowercase());
            if idx.is_some() {
                break;
            }
        }
    }

    let idx = match idx {
        Some(i) => i,
        None => return truncate(text, MAX_SNIPPET_LEN),
    };

    let start = idx.saturating_sub(context_chars);
    let end = (idx + query.len() + context_chars).min(text.len());

    // Ensure we don't split multi-byte chars
    let start = floor_char_boundary(text, start);
    let end = ceil_char_boundary(text, end);

    let snippet = &text[start..end];
    let mut result = String::new();
    if start > 0 {
        result.push_str("...");
    }
    result.push_str(snippet);
    if end < text.len() {
        result.push_str("...");
    }
    result
}

fn build_index_lookup(base: &Path) -> HashMap<String, SessionIndexEntry> {
    let mut lookup = HashMap::new();
    for index_path in find_all_index_files(base) {
        let (_original_path, entries) = load_index(&index_path);
        for entry in entries {
            if !entry.session_id.is_empty() {
                lookup.insert(entry.session_id.clone(), entry);
            }
        }
    }
    lookup
}

/// Parse a single ripgrep output line: /path/to/file.jsonl:LINE_NUM:json_content
fn parse_rg_line(line: &str) -> Option<(PathBuf, serde_json::Value)> {
    // Split on first two colons
    let first_colon = line.find(':')?;
    let path = PathBuf::from(&line[..first_colon]);
    let rest = &line[first_colon + 1..];
    let second_colon = rest.find(':')?;
    let json_str = &rest[second_colon + 1..];
    let value = serde_json::from_str(json_str).ok()?;
    Some((path, value))
}

/// Extract session ID from file path (OpenClaw: filename is session ID)
fn session_id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Pre-load OpenClaw session metadata by reading session headers from all JSONL files
fn load_openclaw_session_metadata(base: &Path) -> HashMap<String, OpenClawSessionMeta> {
    let mut metadata = HashMap::new();
    
    let Ok(entries) = fs::read_dir(base) else {
        return metadata;
    };
    
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().map_or(false, |e| e == "jsonl") {
            continue;
        }
        // Skip deleted sessions
        if path.to_string_lossy().contains(".deleted.") {
            continue;
        }
        
        let session_id = session_id_from_path(&path);
        if session_id.is_empty() {
            continue;
        }
        
        // Read first line to get session header
        if let Ok(content) = fs::read_to_string(&path) {
            if let Some(first_line) = content.lines().next() {
                if let Ok(record) = serde_json::from_str::<serde_json::Value>(first_line) {
                    if record.get("type").and_then(|t| t.as_str()) == Some("session") {
                        let cwd = record
                            .get("cwd")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .to_string();
                        let timestamp = record
                            .get("timestamp")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string();
                        metadata.insert(session_id, OpenClawSessionMeta { cwd, timestamp });
                    }
                }
            }
        }
    }
    
    metadata
}

/// Check if all query terms appear in the lowercased text
fn matches_all_terms(text_lower: &str, query_terms_lower: &[String]) -> bool {
    query_terms_lower.iter().all(|term| text_lower.contains(term))
}

fn search_deep_claude(
    query: &str,
    limit: usize,
    project_filter: Option<&str>,
    base: &Path,
) -> Vec<DeepMatch> {
    let search_path = resolve_search_path(base, project_filter);
    // Pre-lowercase query terms to avoid repeated allocations
    let query_terms_lower: Vec<String> = query
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .collect();
    let index_lookup = build_index_lookup(base);

    let output = Command::new("rg")
        .args([
            "--no-heading",
            "--line-number",
            "--ignore-case",
            "--glob",
            "*.jsonl",
            "--glob",
            "!**/subagents/**",
            "--glob",
            "!**/sessions-index.json",
            query,
        ])
        .arg(&search_path)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("ERROR: ripgrep (rg) not found. Install it: brew install ripgrep");
                std::process::exit(1);
            }
            eprintln!("ERROR: Failed to run ripgrep: {e}");
            return vec![];
        }
    };

    // rg returns exit code 1 for no matches, which is fine
    if !output.status.success() && output.status.code() != Some(1) {
        eprintln!("WARNING: ripgrep returned unexpected exit code: {:?}", output.status.code());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut matches = Vec::new();
    let mut seen_sessions: HashMap<String, usize> = HashMap::new();

    for line in stdout.lines() {
        if matches.len() >= limit {
            break;
        }

        let (_path, record) = match parse_rg_line(line) {
            Some(r) => r,
            None => continue,
        };

        let record_type = record
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        if record_type != "user" && record_type != "assistant" {
            continue;
        }

        let session_id = record
            .get("sessionId")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();

        let count = seen_sessions.entry(session_id.clone()).or_insert(0);
        if *count >= MAX_MATCHES_PER_SESSION {
            continue;
        }

        let text = extract_text_claude(&record);
        if text.is_empty() {
            continue;
        }

        // Lowercase text once, then check all terms
        let text_lower = text.to_lowercase();
        if !matches_all_terms(&text_lower, &query_terms_lower) {
            continue;
        }

        let snippet = get_snippet(&text, query, 80);

        let index_entry = index_lookup.get(&session_id);
        let project_path = record
            .get("cwd")
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| index_entry.map(|e| e.project_path.clone()))
            .unwrap_or_else(|| "unknown".to_string());

        let timestamp = record
            .get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        matches.push(DeepMatch {
            session_id: session_id.clone(),
            project_path,
            message_type: record_type.to_string(),
            snippet,
            timestamp,
            summary: index_entry.map(|e| e.summary.clone()),
            first_prompt: index_entry.map(|e| truncate(&e.first_prompt, 120)),
        });

        *count += 1;
    }

    matches
}

fn search_deep_openclaw(
    query: &str,
    limit: usize,
    base: &Path,
) -> Vec<DeepMatch> {
    // Pre-lowercase query terms to avoid repeated allocations
    let query_terms_lower: Vec<String> = query
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .collect();

    // Pre-load session metadata before searching
    let session_metadata = load_openclaw_session_metadata(base);

    let output = Command::new("rg")
        .args([
            "--no-heading",
            "--line-number",
            "--ignore-case",
            "--glob",
            "*.jsonl",
            "--glob",
            "!*.deleted.*",
            query,
        ])
        .arg(base)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("ERROR: ripgrep (rg) not found. Install it: brew install ripgrep");
                std::process::exit(1);
            }
            eprintln!("ERROR: Failed to run ripgrep: {e}");
            return vec![];
        }
    };

    // rg returns exit code 1 for no matches, which is fine
    if !output.status.success() && output.status.code() != Some(1) {
        eprintln!("WARNING: ripgrep returned unexpected exit code: {:?}", output.status.code());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut matches = Vec::new();
    let mut seen_sessions: HashMap<String, usize> = HashMap::new();

    for line in stdout.lines() {
        if matches.len() >= limit {
            break;
        }

        let (path, record) = match parse_rg_line(line) {
            Some(r) => r,
            None => continue,
        };

        let record_type = record
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        // Only process message records (skip session headers, tool calls, etc.)
        if record_type != "message" {
            continue;
        }

        let session_id = session_id_from_path(&path);

        let count = seen_sessions.entry(session_id.clone()).or_insert(0);
        if *count >= MAX_MATCHES_PER_SESSION {
            continue;
        }

        let (role, text) = extract_text_openclaw(&record);
        if text.is_empty() || (role != "user" && role != "assistant") {
            continue;
        }

        // Lowercase text once, then check all terms
        let text_lower = text.to_lowercase();
        if !matches_all_terms(&text_lower, &query_terms_lower) {
            continue;
        }

        let snippet = get_snippet(&text, query, 80);

        // Get timestamp from message, fall back to session metadata
        let timestamp = record
            .get("timestamp")
            .and_then(|t| t.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| session_metadata.get(&session_id).map(|m| m.timestamp.clone()))
            .unwrap_or_default();

        // Get cwd from session metadata (pre-loaded)
        let project_path = session_metadata
            .get(&session_id)
            .map(|m| m.cwd.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string());

        matches.push(DeepMatch {
            session_id: session_id.clone(),
            project_path,
            message_type: role,
            snippet,
            timestamp,
            summary: None,
            first_prompt: None,
        });

        *count += 1;
    }

    matches
}

// ─── Output Formatting ─────────────────────────────────────────────

fn print_index_results(matches: &[IndexMatch], query: &str, limit: usize) {
    let total = matches.len();
    let displayed = &matches[..total.min(limit)];

    let sep = "=".repeat(60);
    println!("\n{sep}");
    println!("  INDEX SEARCH: \"{query}\"");
    if total > limit {
        println!("  {total} matches found (showing top {limit})");
    } else {
        println!("  {total} matches found");
    }
    println!("{sep}\n");

    if displayed.is_empty() {
        println!("  No matches found in session metadata.");
        println!("  Tip: Try --deep to search full message content.\n");
        return;
    }

    for (i, m) in displayed.iter().enumerate() {
        let project_short = format_project_path(&m.project_path);
        let created = format_date(&m.created);

        let label = if m.summary.is_empty() {
            "(no summary)"
        } else {
            &m.summary
        };
        println!("  [{}] {}", i + 1, label);
        println!("      Project:  {project_short}");
        if !m.git_branch.is_empty() {
            println!("      Branch:   {}", m.git_branch);
        }
        println!("      Date:     {created}");
        println!("      Messages: {}", m.message_count);
        println!("      Matched:  {}", m.matched_field);
        if !m.first_prompt.is_empty() && m.matched_field != "firstPrompt" {
            let preview = truncate(&m.first_prompt, 100);
            let suffix = if m.first_prompt.len() > 100 {
                "..."
            } else {
                ""
            };
            println!("      Prompt:   {preview}{suffix}");
        }
        println!("      Session:  {}", m.session_id);
        println!();
    }

    println!("{sep}");
    println!("  Tip: Use --deep to search inside message content.");
    println!("{sep}\n");
}

fn print_deep_results(matches: &[DeepMatch], query: &str, limit: usize, is_openclaw: bool) {
    let total = matches.len();
    let displayed = &matches[..total.min(limit)];

    let sep = "=".repeat(60);
    let source = if is_openclaw { "OPENCLAW" } else { "CLAUDE CODE" };
    println!("\n{sep}");
    println!("  DEEP SEARCH ({source}): \"{query}\"");
    if total > limit {
        println!("  {total} matches found (showing top {limit})");
    } else {
        println!("  {total} matches found");
    }
    println!("{sep}\n");

    if displayed.is_empty() {
        println!("  No matches found in session message content.\n");
        return;
    }

    for (i, m) in displayed.iter().enumerate() {
        let project_short = format_project_path(&m.project_path);
        let ts = format_date(&m.timestamp);
        let role = if m.message_type == "user" {
            "USER"
        } else {
            "ASST"
        };

        let label = m
            .summary
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(m.first_prompt.as_deref().filter(|s| !s.is_empty()))
            .unwrap_or("(no summary)");

        println!("  [{}] [{}] {}", i + 1, role, label);
        println!("      Project:  {project_short}");
        println!("      Date:     {ts}");
        let clean_snippet: String = m.snippet.split_whitespace().collect::<Vec<_>>().join(" ");
        println!("      Snippet:  {clean_snippet}");
        println!("      Session:  {}", m.session_id);
        println!();
    }

    println!("{sep}\n");
}

// ─── Main ───────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    let query = cli.query.join(" ");
    if query.is_empty() {
        eprintln!("ERROR: No search query provided");
        std::process::exit(1);
    }

    if cli.openclaw {
        // OpenClaw mode
        let base = openclaw_sessions_dir(&cli.agent);
        if !base.exists() {
            eprintln!(
                "ERROR: OpenClaw sessions directory not found: {}",
                base.display()
            );
            eprintln!("       Make sure OpenClaw is installed and has session history.");
            std::process::exit(1);
        }

        // OpenClaw only supports deep search (no index files)
        if !cli.deep {
            eprintln!("NOTE: OpenClaw mode uses deep search by default (no index files).");
        }

        let matches = search_deep_openclaw(&query, cli.limit, &base);
        print_deep_results(&matches, &query, cli.limit, true);
    } else {
        // Claude Code mode
        let base = claude_projects_dir();
        if !base.exists() {
            eprintln!(
                "ERROR: Claude projects directory not found: {}",
                base.display()
            );
            std::process::exit(1);
        }

        let project_filter = cli.project.as_deref();

        if cli.deep {
            let matches = search_deep_claude(&query, cli.limit, project_filter, &base);
            print_deep_results(&matches, &query, cli.limit, false);
        } else {
            let matches = search_index(&query, project_filter, &base);
            print_index_results(&matches, &query, cli.limit);
        }
    }
}
