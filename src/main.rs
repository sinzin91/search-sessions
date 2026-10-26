use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, FixedOffset, NaiveDate, TimeDelta, Utc};
use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};

// ─── Constants ──────────────────────────────────────────────────────

const MAX_SNIPPET_LEN: usize = 200;
const DEFAULT_LIMIT: usize = 20;
const MAX_MATCHES_PER_SESSION: usize = 2;

// ─── CLI ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "search-sessions",
    about = "Search Claude Code, OpenClaw session history, or Obsidian vaults"
)]
struct Cli {
    /// Search query (words are ANDed together)
    query: Vec<String>,

    /// Search full message content (slower)
    #[arg(long)]
    deep: bool,

    /// Search OpenClaw sessions instead of Claude Code
    #[arg(long)]
    openclaw: bool,

    /// Search an Obsidian vault directory
    #[arg(long)]
    obsidian: Option<PathBuf>,

    /// Maximum results to show
    #[arg(long, default_value_t = DEFAULT_LIMIT)]
    limit: usize,

    /// Filter to sessions from projects matching this substring
    #[arg(long)]
    project: Option<String>,

    /// OpenClaw agent to search (default: main)
    #[arg(long, default_value = "main")]
    agent: String,

    /// Filter results from this date/time (e.g. "today", "yesterday", "3 days ago", "last week", "2026-02-20")
    #[arg(long)]
    since: Option<String>,

    /// Filter results until this date/time (e.g. "today", "yesterday", "2026-02-24")
    #[arg(long)]
    until: Option<String>,

    /// Shorthand for --since <date> --until <date+1day> (e.g. "today", "2026-02-20")
    #[arg(long)]
    date: Option<String>,

    /// Output format (text for humans, jsonl for automation)
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Text,
    Jsonl,
}

// ─── Data Structures ────────────────────────────────────────────────

#[derive(Serialize)]
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
    #[serde(rename = "transcript_path", serialize_with = "lossy_path")]
    session_file: Option<PathBuf>,
    custom_title: Option<String>,
    /// Claude config dir the session resumes in
    config_root: String,
    /// Set when the session lives outside the root `claude` reads by default
    #[serde(skip)]
    config_dir: Option<PathBuf>,
}

#[derive(Serialize)]
struct DeepMatch {
    session_id: String,
    project_path: String,
    message_type: String,
    snippet: String,
    timestamp: String,
    summary: Option<String>,
    first_prompt: Option<String>,
    custom_title: Option<String>,
    /// Set when the session lives outside the root `claude` reads by default
    #[serde(skip)]
    config_dir: Option<PathBuf>,
    /// Transcript holding the matched record
    transcript_path: String,
    /// Claude config dir the session resumes in (Claude Code hits only)
    #[serde(skip_serializing_if = "Option::is_none")]
    config_root: Option<String>,
    /// OpenClaw sessions dir searched (OpenClaw hits only)
    #[serde(skip_serializing_if = "Option::is_none")]
    sessions_root: Option<String>,
    record_line: usize,
    record_uuid: Option<String>,
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

/// Obsidian search result
#[derive(Serialize)]
struct ObsidianMatch {
    file_path: PathBuf,
    file_name: String,
    snippet: String,
    line_number: usize,
}

// ─── Helpers ────────────────────────────────────────────────────────

/// Every Claude config root on this machine that holds a projects/ tree.
///
/// Claude Code supports account isolation via CLAUDE_CONFIG_DIR, so transcripts
/// are split across sibling roots: ~/.claude for personal, ~/.claude-work (or
/// any other overlay) for work. Searching only ~/.claude silently returns
/// nothing for half the corpus, which reads as "that conversation does not
/// exist" rather than "I did not look there".
///
/// Discovery is by shape, not by a hardcoded list: any ~/.claude* directory
/// with a projects/ subdirectory counts, so a new overlay is picked up without
/// a code change. CLAUDE_CONFIG_DIR is honoured explicitly in case it points
/// outside the home directory.
fn claude_projects_dirs() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    // Dedupe on the canonical path: a symlinked root (~/.claude-current ->
    // ~/.claude) or a non-canonical CLAUDE_CONFIG_DIR would otherwise scan
    // the same tree twice and print every hit twice.
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut push = |dir: PathBuf| {
        if dir.is_dir() && seen.insert(fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            roots.push(dir);
        }
    };

    if let Some(configured) = std::env::var_os("CLAUDE_CONFIG_DIR")
        && !configured.is_empty()
    {
        let configured = PathBuf::from(configured);
        push(
            std::path::absolute(&configured)
                .unwrap_or(configured)
                .join("projects"),
        );
    }

    if let Some(home) = dirs::home_dir() {
        push(home.join(".claude").join("projects"));

        if let Ok(entries) = fs::read_dir(&home) {
            let mut siblings: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(".claude"))
                })
                .map(|path| path.join("projects"))
                .filter(|path| path.is_dir())
                .collect();
            // read_dir order is filesystem-dependent; sort so results are
            // reproducible across runs and machines.
            siblings.sort();
            for sibling in siblings {
                push(sibling);
            }
        }
    }

    roots
}

/// Config root to pass as CLAUDE_CONFIG_DIR when resuming a session found
/// under `projects_dir`, or None if `claude` already reads that root.
///
/// `claude -r` only looks in its own config root ($CLAUDE_CONFIG_DIR, else
/// ~/.claude), so a session from any other root needs the override.
fn resume_config_dir(projects_dir: &Path) -> Option<PathBuf> {
    let root = projects_dir.parent()?;
    let default = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        // The resume command cds into the project first, which would make a
        // relative CLAUDE_CONFIG_DIR point somewhere else; spell it out.
        Some(dir) if !dir.is_empty() && Path::new(&dir).is_relative() => {
            return Some(root.to_path_buf());
        }
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()?.join(".claude"),
    };
    let canonical = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if canonical(root) == canonical(&default) {
        None
    } else {
        Some(root.to_path_buf())
    }
}

/// The copy-pasteable resume command printed under each Claude Code result
fn resume_command(project_short: &str, session_id: &str, config_dir: Option<&Path>) -> String {
    let cd = match project_short.strip_prefix("~/") {
        _ if project_short == "~" => "~".to_string(),
        Some(rest) => format!("~/{}", shell_quote(rest)),
        None => shell_quote(project_short),
    };
    match config_dir {
        Some(dir) => format!(
            "cd {cd} && CLAUDE_CONFIG_DIR={} claude -r {}",
            shell_path(dir),
            shell_quote(session_id)
        ),
        None => format!("cd {cd} && claude -r {}", shell_quote(session_id)),
    }
}

/// `s` as a single shell word, quoted only when it has to be
fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+:@%,".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// A path as a shell word that bash, zsh and fish all read back unchanged.
/// Paths under the home directory use $HOME, which (unlike ~ after `=`)
/// every one of those shells expands in a `VAR=value cmd` prefix.
fn shell_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(home) = dirs::home_dir()
        && let Some(rest) = path.strip_prefix(home.to_string_lossy().as_ref())
        && rest.starts_with('/')
    {
        return format!("$HOME/{}", shell_quote(&rest[1..]));
    }
    shell_quote(&path)
}

/// Which config root owns a session found in several, given each root's
/// (index, transcript length) for it: the longest copy, ties going to the
/// earlier (higher-priority) root.
///
/// A backup or copied root (~/.claude.bak) holds copies of live sessions.
/// Transcripts only grow, so a stale copy is never longer than the live
/// one, and an identical copy defers to root priority. Modification time
/// can't be used: `cp -r` gives the copy a newer one than the original.
fn session_owner(copies: &[(usize, u64)]) -> Option<usize> {
    copies
        .iter()
        .max_by(|(ra, la), (rb, lb)| la.cmp(lb).then(rb.cmp(ra)))
        .map(|&(root, _)| root)
}

/// Owner of every session with a transcript in any config root, for deep
/// search, which needs them before it searches.
///
/// Ownership never hides a match: it decides which root's hits are shown
/// when several roots match the same session, and which root the Resume
/// command targets, so resuming continues the live copy rather than a
/// backup's. A session with a single transcript is owned by its root even
/// if another root's stale index still lists it. Only sessions found in
/// more than one root are stat'ed.
fn session_owners(bases: &[PathBuf], project_filter: Option<&str>) -> HashMap<String, usize> {
    if bases.len() < 2 {
        return HashMap::new();
    }
    let mut copies: HashMap<String, Vec<(usize, PathBuf)>> = HashMap::new();
    for (root, base) in bases.iter().enumerate() {
        // Exactly the transcripts deep search reads (nested ones included,
        // subagents excluded), within the dirs --project selects.
        for path in resolve_search_paths(base, project_filter)
            .iter()
            .flat_map(|dir| find_jsonl_files(dir, true, false))
        {
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            copies.entry(id.to_string()).or_default().push((root, path));
        }
    }
    copies
        .into_iter()
        .filter_map(|(id, c)| {
            let owner = if c.iter().all(|(root, _)| *root == c[0].0) {
                Some(c[0].0)
            } else {
                let lens: Vec<(usize, u64)> = c
                    .iter()
                    .map(|(root, path)| (*root, fs::metadata(path).map_or(0, |m| m.len())))
                    .collect();
                session_owner(&lens)
            };
            owner.map(|root| (id, root))
        })
        .collect()
}

/// Owners of just the sessions index search matched. Each hit names its
/// transcript, and a copied root keeps the same project dir name, so this
/// checks that one path in every root instead of walking them all.
fn index_owners(bases: &[PathBuf], hits: &[(usize, IndexMatch)]) -> HashMap<String, usize> {
    if bases.len() < 2 {
        return HashMap::new();
    }
    let mut owners = HashMap::new();
    for (_, hit) in hits {
        if owners.contains_key(&hit.session_id) {
            continue;
        }
        let Some(project) = hit
            .session_file
            .as_deref()
            .and_then(Path::parent)
            .and_then(Path::file_name)
        else {
            continue;
        };
        let file_name = format!("{}.jsonl", hit.session_id);
        let copies: Vec<(usize, u64)> = bases
            .iter()
            .enumerate()
            .filter_map(|(root, base)| {
                fs::metadata(base.join(project).join(&file_name))
                    .ok()
                    .map(|m| (root, m.len()))
            })
            .collect();
        if let Some(owner) = session_owner(&copies) {
            owners.insert(hit.session_id.clone(), owner);
        }
    }
    owners
}

/// Keep one root's hits per session: the owner's if the owner matched,
/// otherwise the first root that did (a copy can hold a match the owner's
/// copy doesn't). `retarget(hit, root, owner)` points each kept hit at the
/// session's owner, which its Resume command targets.
fn pick_session_hits<T>(
    hits: Vec<(usize, T)>,
    session_id: impl Fn(&T) -> &str,
    owners: &HashMap<String, usize>,
    retarget: impl Fn(&mut T, usize, usize),
) -> Vec<(usize, T)> {
    let mut shown: HashMap<String, usize> = HashMap::new();
    for (root, hit) in &hits {
        let id = session_id(hit);
        let entry = shown.entry(id.to_string()).or_insert(*root);
        if owners.get(id) == Some(root) {
            *entry = *root;
        }
    }
    hits.into_iter()
        .filter(|(root, hit)| {
            let id = session_id(hit);
            id.is_empty() || shown.get(id) == Some(root)
        })
        .map(|(root, mut hit)| {
            let owner = owners.get(session_id(&hit)).copied().unwrap_or(root);
            retarget(&mut hit, root, owner);
            (root, hit)
        })
        .collect()
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
    // Handle trailing lowercase 'z' only
    if iso_str.ends_with('z') {
        let mut fixed = iso_str.to_string();
        fixed.pop();
        fixed.push('Z');
        if let Ok(dt) = DateTime::<FixedOffset>::parse_from_rfc3339(&fixed) {
            return dt.format("%Y-%m-%d %H:%M").to_string();
        }
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

// ─── Date Range Filtering ───────────────────────────────────────────

#[derive(Clone)]
struct DateRange {
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
}

impl DateRange {
    fn contains(&self, timestamp_str: &str) -> bool {
        if self.since.is_none() && self.until.is_none() {
            return true;
        }
        let Some(dt) = parse_timestamp(timestamp_str) else {
            return false; // Can't parse → exclude when filtering by date
        };
        if let Some(ref since) = self.since
            && dt < *since
        {
            return false;
        }
        if let Some(ref until) = self.until
            && dt >= *until
        {
            return false;
        }
        true
    }

    fn is_active(&self) -> bool {
        self.since.is_some() || self.until.is_some()
    }
}

/// Parse an ISO timestamp string into a UTC DateTime
fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    // Try RFC 3339 directly
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.to_utc());
    }
    // Handle lowercase 'z' suffix (e.g. "2026-02-24T12:00:00z")
    if s.ends_with('z') {
        let mut fixed = s.to_string();
        fixed.pop();
        fixed.push('Z');
        if let Ok(dt) = DateTime::parse_from_rfc3339(&fixed) {
            return Some(dt.to_utc());
        }
    }
    None
}

/// Map a unit string (singular) to a TimeDelta multiplier
fn unit_to_delta(unit: &str, n: i64) -> Option<TimeDelta> {
    match unit {
        "day" => Some(TimeDelta::days(n)),
        "week" => Some(TimeDelta::weeks(n)),
        "month" => Some(TimeDelta::days(n * 30)),
        _ => None,
    }
}

/// Start-of-day UTC for a NaiveDate
fn start_of_day(date: NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(0, 0, 0).unwrap().and_utc()
}

/// Try to parse "N units ago" from a lowercased string
fn try_parse_ago(s: &str, today: NaiveDate) -> Option<DateTime<Utc>> {
    let rest = s.strip_suffix(" ago")?;
    let mut parts = rest.split_whitespace();
    let n: i64 = parts.next()?.parse().ok()?;
    let unit = parts.next()?.trim_end_matches('s');
    if parts.next().is_some() {
        return None; // too many tokens
    }
    let delta = unit_to_delta(unit, n)?;
    Some(start_of_day(today - delta))
}

/// Try to parse "last week" / "last month" from a lowercased string
fn try_parse_last(s: &str, today: NaiveDate) -> Option<DateTime<Utc>> {
    let unit = s.strip_prefix("last ")?.trim_end_matches('s');
    let delta = unit_to_delta(unit, 1)?;
    Some(start_of_day(today - delta))
}

/// Result of parsing a human date string.
/// `date_only` is true when the input resolved to a whole day (not a precise time).
struct ParsedDate {
    dt: DateTime<Utc>,
    date_only: bool,
}

/// Parse a human-friendly date string into a UTC DateTime (start of day)
fn parse_human_date(s: &str) -> Result<ParsedDate, String> {
    let trimmed = s.trim();
    let lower = trimmed.to_lowercase();
    let today = Utc::now().date_naive();

    // Exact keywords → date-only
    if lower == "today" || lower == "now" {
        return Ok(ParsedDate {
            dt: start_of_day(today),
            date_only: true,
        });
    }
    if lower == "yesterday" {
        return Ok(ParsedDate {
            dt: start_of_day(today - TimeDelta::days(1)),
            date_only: true,
        });
    }

    // "N days/weeks/months ago" → date-only
    if let Some(dt) = try_parse_ago(&lower, today) {
        return Ok(ParsedDate {
            dt,
            date_only: true,
        });
    }

    // "last week" / "last month" → date-only
    if let Some(dt) = try_parse_last(&lower, today) {
        return Ok(ParsedDate {
            dt,
            date_only: true,
        });
    }

    // Absolute YYYY-MM-DD → date-only
    if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Ok(ParsedDate {
            dt: start_of_day(date),
            date_only: true,
        });
    }

    // Absolute ISO datetime → precise (NOT date-only)
    if let Some(dt) = parse_timestamp(trimmed) {
        return Ok(ParsedDate {
            dt,
            date_only: false,
        });
    }

    Err(format!(
        "Cannot parse date: '{trimmed}'. Try: today, yesterday, 3 days ago, last week, or YYYY-MM-DD"
    ))
}

/// Build a DateRange from CLI args
fn build_date_range(
    since: Option<&str>,
    until: Option<&str>,
    date: Option<&str>,
) -> Result<DateRange, String> {
    if let Some(d) = date {
        // --date is shorthand for a single day
        let parsed = parse_human_date(d)?;
        let end = parsed.dt + TimeDelta::days(1);
        return Ok(DateRange {
            since: Some(parsed.dt),
            until: Some(end),
        });
    }

    let since_dt = since
        .map(|s| parse_human_date(s).map(|p| p.dt))
        .transpose()?;
    let until_dt = until
        .map(|u| {
            let parsed = parse_human_date(u)?;
            // For date-only inputs ("2026-02-24", "yesterday"), add 1 day to include the full day.
            // For precise ISO datetimes ("2026-02-24T15:00:00Z"), use the exact time as-is.
            let boundary = if parsed.date_only {
                parsed.dt + TimeDelta::days(1)
            } else {
                parsed.dt
            };
            Ok::<_, String>(boundary)
        })
        .transpose()?;

    Ok(DateRange {
        since: since_dt,
        until: until_dt,
    })
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
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
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
    date_range: &DateRange,
) -> Vec<IndexMatch> {
    let query_terms: Vec<&str> = query.split_whitespace().collect();
    let mut matches = Vec::new();

    for index_path in find_all_index_files(base) {
        let (original_path, entries) = load_index(&index_path);

        if let Some(filter) = project_filter
            && !original_path
                .to_lowercase()
                .contains(&filter.to_lowercase())
        {
            continue;
        }

        for entry in &entries {
            let (score, matched_field) = score_index_entry(entry, &query_terms);
            if score > 0.0 {
                // Apply date range filter (check created or modified)
                if date_range.is_active() {
                    let in_range =
                        date_range.contains(&entry.created) || date_range.contains(&entry.modified);
                    if !in_range {
                        continue;
                    }
                }
                matches.push(IndexMatch {
                    session_id: entry.session_id.clone(),
                    project_path: if entry.project_path.is_empty() {
                        original_path.clone()
                    } else {
                        entry.project_path.clone()
                    },
                    first_prompt: entry.first_prompt.clone(),
                    summary: entry.summary.clone(),
                    git_branch: entry.git_branch.clone(),
                    created: entry.created.clone(),
                    modified: entry.modified.clone(),
                    message_count: entry.message_count,
                    matched_field,
                    score,
                    session_file: index_session_file(&index_path, &entry.session_id),
                    custom_title: None,
                    config_root: config_root_for_projects(base),
                    config_dir: None,
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

/// Directories to search within one config root for a given --project filter.
///
/// Returns EVERY matching project directory, not the first. One project
/// routinely has several encoded directories (a repo plus its worktrees, e.g.
/// -home-user-myapp and
/// -home-user-myapp--claude-worktrees-feature-x), and
/// returning only the first hid whichever came later in readdir order.
///
/// An empty result means the filter matched nothing in THIS root, which is
/// normal when a project lives only under another account. Callers skip the
/// root rather than falling back to searching all of it, since that fallback
/// silently widens a narrow query into a whole-account scan.
fn resolve_search_paths(base: &Path, project_filter: Option<&str>) -> Vec<PathBuf> {
    let Some(filter) = project_filter else {
        return vec![base.to_path_buf()];
    };

    let filter_lower = filter.to_lowercase();
    let mut paths: Vec<PathBuf> = match fs::read_dir(base) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.to_lowercase().contains(&filter_lower))
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    // readdir order is filesystem-dependent; sort for reproducible results.
    paths.sort();
    paths
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

/// Read a session's user-assigned title from its transcript. Claude Code
/// appends a `custom-title` record on each rename, so the last one wins.
fn read_custom_title(path: &Path) -> Option<String> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    let mut title = None;
    let mut line = Vec::new();
    loop {
        line.clear();
        // Stop at the first read error: a persistent one would repeat forever
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // Cheap pre-check so ordinary message lines are never parsed
        if !line.windows(12).any(|w| w == b"custom-title") {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<serde_json::Value>(&line) else {
            continue;
        };
        if record.get("type").and_then(|t| t.as_str()) == Some("custom-title")
            && let Some(t) = record.get("customTitle").and_then(|t| t.as_str())
        {
            let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
            if !t.is_empty() {
                title = Some(t);
            }
        }
    }
    title
}

/// Transcript path for an index entry. Session IDs come from the index file,
/// so only accept a bare file name that cannot point outside the project dir.
fn index_session_file(index_path: &Path, session_id: &str) -> Option<PathBuf> {
    if Path::new(session_id).file_name() != Some(session_id.as_ref()) {
        return None;
    }
    Some(index_path.with_file_name(format!("{session_id}.jsonl")))
}

/// Read each transcript's title at most once per search
fn cached_custom_title(
    cache: &mut HashMap<PathBuf, Option<String>>,
    path: &Path,
) -> Option<String> {
    cache
        .entry(path.to_path_buf())
        .or_insert_with(|| read_custom_title(path))
        .clone()
}

/// Parse a single ripgrep output line: /path/to/file.jsonl:LINE_NUM:json_content
fn parse_rg_line(line: &str) -> Option<(PathBuf, usize, serde_json::Value)> {
    // Split on first two colons
    let first_colon = line.find(':')?;
    let path = PathBuf::from(&line[..first_colon]);
    let rest = &line[first_colon + 1..];
    let second_colon = rest.find(':')?;
    let line_number = rest[..second_colon].parse().ok()?;
    let json_str = &rest[second_colon + 1..];
    let value = serde_json::from_str(json_str).ok()?;
    Some((path, line_number, value))
}

fn config_root_for_projects(base: &Path) -> String {
    base.parent().unwrap_or(base).to_string_lossy().into_owned()
}

fn record_uuid(record: &serde_json::Value) -> Option<String> {
    record
        .get("uuid")
        .and_then(|value| value.as_str())
        .map(String::from)
}

/// A path as a JSON string, lossily like the other path fields, so one
/// non-UTF-8 name can't fail the whole row
fn lossy_path<S: serde::Serializer>(path: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
    path.as_deref().map(Path::to_string_lossy).serialize(s)
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
        if path.extension().is_none_or(|e| e != "jsonl") {
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
        if let Ok(content) = fs::read_to_string(&path)
            && let Some(first_line) = content.lines().next()
            && let Ok(record) = serde_json::from_str::<serde_json::Value>(first_line)
            && record.get("type").and_then(|t| t.as_str()) == Some("session")
        {
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

    metadata
}

/// Check if all query terms appear in the lowercased text
fn matches_all_terms(text_lower: &str, query_terms_lower: &[String]) -> bool {
    query_terms_lower
        .iter()
        .all(|term| text_lower.contains(term))
}

// ─── Ripgrep Detection & Fallback ───────────────────────────────────

/// Cache for ripgrep availability check
static RIPGREP_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Check if ripgrep (rg) is available in PATH
fn is_ripgrep_available() -> bool {
    *RIPGREP_AVAILABLE.get_or_init(|| {
        Command::new("rg")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Print a one-time warning about ripgrep not being available
static RIPGREP_WARNING_SHOWN: OnceLock<()> = OnceLock::new();

fn warn_ripgrep_not_available() {
    RIPGREP_WARNING_SHOWN.get_or_init(|| {
        eprintln!("WARNING: ripgrep (rg) not found. Using slower Rust fallback.");
        eprintln!("         Install ripgrep for 3-5x faster deep search: brew install ripgrep");
        eprintln!();
    });
}

/// Find all JSONL files in a directory tree
fn find_jsonl_files(base: &Path, exclude_subagents: bool, exclude_deleted: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();

    fn walk_dir(
        dir: &Path,
        files: &mut Vec<PathBuf>,
        exclude_subagents: bool,
        exclude_deleted: bool,
    ) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            // Use file_type() to avoid following symlinks (matches ripgrep behavior)
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            // Skip symlinks entirely to avoid loops
            if file_type.is_symlink() {
                continue;
            }

            if file_type.is_dir() {
                // Skip subagents directory if requested
                if exclude_subagents && path.file_name().is_some_and(|n| n == "subagents") {
                    continue;
                }
                walk_dir(&path, files, exclude_subagents, exclude_deleted);
            } else if file_type.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
                // Skip deleted files if requested
                if exclude_deleted && path.to_string_lossy().contains(".deleted.") {
                    continue;
                }
                // Skip sessions-index.json (though it shouldn't have .jsonl extension)
                if path.file_name().is_some_and(|n| n == "sessions-index.json") {
                    continue;
                }
                files.push(path);
            }
        }
    }

    walk_dir(base, &mut files, exclude_subagents, exclude_deleted);
    files
}

/// Mirror Claude Code's project-folder encoding: every non-alphanumeric char → '-'.
/// e.g. `/Users/arjun/Documents/sandbox/hypeo` → `-Users-arjun-Documents-sandbox-hypeo`.
fn encode_project_folder(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Resolve a session's *canonical* project directory — the cwd Claude Code was launched
/// in, which the session file is filed under — rather than a transient cwd recorded after
/// the shell `cd`'d into a subdirectory. The resume command must `cd` into this directory
/// for the session to belong to the right project (Claude Code's interactive picker is
/// scoped per project folder).
///
/// The folder name under `~/.claude/projects/` is the launch cwd with every non-alphanumeric
/// char replaced by '-', which is lossy (both '/' and '-' map to '-'), so it can't be reversed
/// directly. Instead we find the recorded `cwd` whose encoding equals the folder name. The
/// matched record's own cwd is checked first (no I/O); otherwise we scan the session file.
/// Successful resolutions are memoized per project folder. Falls back to `fallback_cwd` when
/// nothing matches (e.g. remote `ssh-<uuid>` folders, or a project that was moved on disk).
fn resolve_project_dir(jsonl_path: &Path, fallback_cwd: &str) -> String {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();

    let Some(folder_dir) = jsonl_path.parent() else {
        return fallback_cwd.to_string();
    };
    let Some(folder_name) = folder_dir.file_name().and_then(|n| n.to_str()) else {
        return fallback_cwd.to_string();
    };

    // Fast path: the matched record's own cwd is already the project root.
    if encode_project_folder(fallback_cwd) == folder_name {
        return fallback_cwd.to_string();
    }

    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().unwrap().get(folder_dir).cloned() {
        return hit;
    }

    // Scan the session file for the cwd whose encoding matches the folder name.
    let mut resolved: Option<String> = None;
    if let Ok(file) = File::open(jsonl_path) {
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if let Some(cwd) = record.get("cwd").and_then(|c| c.as_str())
                && !cwd.is_empty()
                && encode_project_folder(cwd) == folder_name
            {
                resolved = Some(cwd.to_string());
                break;
            }
        }
    }

    match resolved {
        Some(dir) => {
            cache
                .lock()
                .unwrap()
                .insert(folder_dir.to_path_buf(), dir.clone());
            dir
        }
        None => fallback_cwd.to_string(),
    }
}

/// Pure Rust deep search for Claude Code sessions (fallback when ripgrep unavailable)
fn search_deep_claude_rust(
    query: &str,
    limit: usize,
    project_filter: Option<&str>,
    base: &Path,
    date_range: &DateRange,
    owned: &dyn Fn(&str) -> bool,
    owns_any: bool,
) -> Vec<DeepMatch> {
    warn_ripgrep_not_available();

    let search_paths = resolve_search_paths(base, project_filter);
    if search_paths.is_empty() {
        return Vec::new();
    }
    let query_terms_lower: Vec<String> =
        query.split_whitespace().map(|s| s.to_lowercase()).collect();
    let index_lookup = build_index_lookup(base);

    let jsonl_files: Vec<PathBuf> = search_paths
        .iter()
        .flat_map(|path| find_jsonl_files(path, true, false))
        .collect();

    let mut matches = Vec::new();
    let mut owned_hits = 0;
    let mut unowned_hits = 0;
    let mut seen_sessions: HashMap<String, usize> = HashMap::new();
    let mut titles: HashMap<PathBuf, Option<String>> = HashMap::new();

    'outer: for file_path in jsonl_files {
        let Ok(file) = File::open(&file_path) else {
            continue;
        };
        let reader = BufReader::new(file);

        for (line_index, line) in reader.lines().enumerate() {
            if owned_hits >= limit || (!owns_any && unowned_hits >= unowned_cap(limit)) {
                break 'outer;
            }

            let Ok(line) = line else {
                continue;
            };

            let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };

            let record_type = record.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if record_type != "user" && record_type != "assistant" {
                continue;
            }

            let session_id = record
                .get("sessionId")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();

            let is_owned = owned(&session_id);

            if !is_owned && unowned_hits >= unowned_cap(limit) {
                continue;
            }

            let count = seen_sessions.entry(session_id.clone()).or_insert(0);
            if *count >= MAX_MATCHES_PER_SESSION {
                continue;
            }

            let text = extract_text_claude(&record);
            if text.is_empty() {
                continue;
            }

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
                .map(|cwd| resolve_project_dir(&file_path, cwd))
                .or_else(|| index_entry.map(|e| e.project_path.clone()))
                .unwrap_or_else(|| "unknown".to_string());

            let timestamp = record
                .get("timestamp")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();

            if !date_range.contains(&timestamp) {
                continue;
            }

            matches.push(DeepMatch {
                session_id: session_id.clone(),
                project_path,
                message_type: record_type.to_string(),
                snippet,
                timestamp,
                summary: index_entry.map(|e| e.summary.clone()),
                first_prompt: index_entry.map(|e| e.first_prompt.clone()),
                custom_title: cached_custom_title(&mut titles, &file_path),
                config_dir: None,
                transcript_path: file_path.to_string_lossy().into_owned(),
                config_root: Some(config_root_for_projects(base)),
                sessions_root: None,
                record_line: line_index + 1,
                record_uuid: record_uuid(&record),
            });

            *count += 1;
            if is_owned {
                owned_hits += 1;
            } else {
                unowned_hits += 1;
            }
        }
    }

    matches
}

/// Pure Rust deep search for OpenClaw sessions (fallback when ripgrep unavailable)
fn search_deep_openclaw_rust(
    query: &str,
    limit: usize,
    base: &Path,
    date_range: &DateRange,
) -> Vec<DeepMatch> {
    warn_ripgrep_not_available();

    let query_terms_lower: Vec<String> =
        query.split_whitespace().map(|s| s.to_lowercase()).collect();
    let session_metadata = load_openclaw_session_metadata(base);

    let jsonl_files = find_jsonl_files(base, false, true);

    let mut matches = Vec::new();
    let mut seen_sessions: HashMap<String, usize> = HashMap::new();

    'outer: for file_path in jsonl_files {
        let Ok(file) = File::open(&file_path) else {
            continue;
        };
        let reader = BufReader::new(file);
        let session_id = session_id_from_path(&file_path);

        for (line_index, line) in reader.lines().enumerate() {
            if matches.len() >= limit {
                break 'outer;
            }

            let Ok(line) = line else {
                continue;
            };

            let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };

            let record_type = record.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if record_type != "message" {
                continue;
            }

            let count = seen_sessions.entry(session_id.clone()).or_insert(0);
            if *count >= MAX_MATCHES_PER_SESSION {
                continue;
            }

            let (role, text) = extract_text_openclaw(&record);
            if text.is_empty() || (role != "user" && role != "assistant") {
                continue;
            }

            let text_lower = text.to_lowercase();
            if !matches_all_terms(&text_lower, &query_terms_lower) {
                continue;
            }

            let snippet = get_snippet(&text, query, 80);

            let timestamp = record
                .get("timestamp")
                .and_then(|t| t.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| {
                    session_metadata
                        .get(&session_id)
                        .map(|m| m.timestamp.clone())
                })
                .unwrap_or_default();

            let project_path = session_metadata
                .get(&session_id)
                .map(|m| m.cwd.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".to_string());

            if !date_range.contains(&timestamp) {
                continue;
            }

            matches.push(DeepMatch {
                session_id: session_id.clone(),
                project_path,
                message_type: role,
                snippet,
                timestamp,
                summary: None,
                first_prompt: None,
                custom_title: None,
                config_dir: None,
                transcript_path: file_path.to_string_lossy().into_owned(),
                config_root: None,
                sessions_root: Some(base.to_string_lossy().into_owned()),
                record_line: line_index + 1,
                record_uuid: record_uuid(&record),
            });

            *count += 1;
        }
    }

    matches
}

/// Most hits for sessions another root owns that one root's deep search keeps
fn unowned_cap(limit: usize) -> usize {
    limit.saturating_mul(MAX_MATCHES_PER_SESSION)
}

/// `owned` says whether this root owns a session ID. Hits for sessions owned
/// by another config root are still returned, but don't count toward
/// `limit`, so a backup's copies can't crowd out its own sessions. At most
/// `unowned_cap(limit)` of them are kept; once that is reached a root that
/// owns no sessions (`owns_any` false) stops early instead of being read end
/// to end.
fn search_deep_claude(
    query: &str,
    limit: usize,
    project_filter: Option<&str>,
    base: &Path,
    date_range: &DateRange,
    owned: &dyn Fn(&str) -> bool,
    owns_any: bool,
) -> Vec<DeepMatch> {
    // Check if ripgrep is available, fall back to pure Rust if not
    if !is_ripgrep_available() {
        return search_deep_claude_rust(
            query,
            limit,
            project_filter,
            base,
            date_range,
            owned,
            owns_any,
        );
    }

    let search_paths = resolve_search_paths(base, project_filter);
    if search_paths.is_empty() {
        return Vec::new();
    }
    // Pre-lowercase query terms to avoid repeated allocations
    let query_terms_lower: Vec<String> =
        query.split_whitespace().map(|s| s.to_lowercase()).collect();
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
        .args(&search_paths)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            // Fallback to Rust if ripgrep fails unexpectedly
            eprintln!("WARNING: Failed to run ripgrep: {e}. Using Rust fallback.");
            return search_deep_claude_rust(
                query,
                limit,
                project_filter,
                base,
                date_range,
                owned,
                owns_any,
            );
        }
    };

    // rg returns exit code 1 for no matches, which is fine
    if !output.status.success() && output.status.code() != Some(1) {
        eprintln!(
            "WARNING: ripgrep returned unexpected exit code: {:?}",
            output.status.code()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut matches = Vec::new();
    let mut owned_hits = 0;
    let mut unowned_hits = 0;
    let mut seen_sessions: HashMap<String, usize> = HashMap::new();
    let mut titles: HashMap<PathBuf, Option<String>> = HashMap::new();

    for line in stdout.lines() {
        if owned_hits >= limit || (!owns_any && unowned_hits >= unowned_cap(limit)) {
            break;
        }

        let (path, record_line, record) = match parse_rg_line(line) {
            Some(r) => r,
            None => continue,
        };

        let record_type = record.get("type").and_then(|t| t.as_str()).unwrap_or("");

        if record_type != "user" && record_type != "assistant" {
            continue;
        }

        let session_id = record
            .get("sessionId")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();

        let is_owned = owned(&session_id);

        if !is_owned && unowned_hits >= unowned_cap(limit) {
            continue;
        }

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
            .map(|cwd| resolve_project_dir(&path, cwd))
            .or_else(|| index_entry.map(|e| e.project_path.clone()))
            .unwrap_or_else(|| "unknown".to_string());

        let timestamp = record
            .get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        if !date_range.contains(&timestamp) {
            continue;
        }

        matches.push(DeepMatch {
            session_id: session_id.clone(),
            project_path,
            message_type: record_type.to_string(),
            snippet,
            timestamp,
            summary: index_entry.map(|e| e.summary.clone()),
            first_prompt: index_entry.map(|e| e.first_prompt.clone()),
            custom_title: cached_custom_title(&mut titles, &path),
            config_dir: None,
            transcript_path: path.to_string_lossy().into_owned(),
            config_root: Some(config_root_for_projects(base)),
            sessions_root: None,
            record_line,
            record_uuid: record_uuid(&record),
        });

        *count += 1;
        if is_owned {
            owned_hits += 1;
        } else {
            unowned_hits += 1;
        }
    }

    matches
}

fn search_deep_openclaw(
    query: &str,
    limit: usize,
    base: &Path,
    date_range: &DateRange,
) -> Vec<DeepMatch> {
    // Check if ripgrep is available, fall back to pure Rust if not
    if !is_ripgrep_available() {
        return search_deep_openclaw_rust(query, limit, base, date_range);
    }

    // Pre-lowercase query terms to avoid repeated allocations
    let query_terms_lower: Vec<String> =
        query.split_whitespace().map(|s| s.to_lowercase()).collect();

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
            // Fallback to Rust if ripgrep fails unexpectedly
            eprintln!("WARNING: Failed to run ripgrep: {e}. Using Rust fallback.");
            return search_deep_openclaw_rust(query, limit, base, date_range);
        }
    };

    // rg returns exit code 1 for no matches, which is fine
    if !output.status.success() && output.status.code() != Some(1) {
        eprintln!(
            "WARNING: ripgrep returned unexpected exit code: {:?}",
            output.status.code()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut matches = Vec::new();
    let mut seen_sessions: HashMap<String, usize> = HashMap::new();

    for line in stdout.lines() {
        if matches.len() >= limit {
            break;
        }

        let (path, record_line, record) = match parse_rg_line(line) {
            Some(r) => r,
            None => continue,
        };

        let record_type = record.get("type").and_then(|t| t.as_str()).unwrap_or("");

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
            .or_else(|| {
                session_metadata
                    .get(&session_id)
                    .map(|m| m.timestamp.clone())
            })
            .unwrap_or_default();

        if !date_range.contains(&timestamp) {
            continue;
        }

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
            custom_title: None,
            config_dir: None,
            transcript_path: path.to_string_lossy().into_owned(),
            config_root: None,
            sessions_root: Some(base.to_string_lossy().into_owned()),
            record_line,
            record_uuid: record_uuid(&record),
        });

        *count += 1;
    }

    matches
}

// ─── Obsidian Search ────────────────────────────────────────────────

/// Find all markdown files in an Obsidian vault
fn find_markdown_files(base: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    fn walk_dir(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            // Skip symlinks
            if file_type.is_symlink() {
                continue;
            }

            if file_type.is_dir() {
                // Skip hidden directories (like .obsidian, .git, .trash)
                if path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                {
                    continue;
                }
                walk_dir(&path, files);
            } else if file_type.is_file() && path.extension().is_some_and(|e| e == "md") {
                files.push(path);
            }
        }
    }

    walk_dir(base, &mut files);
    files
}

/// Search Obsidian vault using ripgrep
fn search_obsidian(query: &str, limit: usize, base: &Path) -> Vec<ObsidianMatch> {
    if !is_ripgrep_available() {
        return search_obsidian_rust(query, limit, base);
    }

    let query_terms_lower: Vec<String> =
        query.split_whitespace().map(|s| s.to_lowercase()).collect();

    let output = Command::new("rg")
        .args([
            "--no-heading",
            "--line-number",
            "--ignore-case",
            "--glob",
            "*.md",
            "--glob",
            "!.obsidian/**",
            "--glob",
            "!.git/**",
            "--glob",
            "!.trash/**",
            query,
        ])
        .arg(base)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            eprintln!("WARNING: Failed to run ripgrep: {e}. Using Rust fallback.");
            return search_obsidian_rust(query, limit, base);
        }
    };

    if !output.status.success() && output.status.code() != Some(1) {
        eprintln!(
            "WARNING: ripgrep returned unexpected exit code: {:?}",
            output.status.code()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut matches = Vec::new();
    let mut seen_files: HashMap<PathBuf, usize> = HashMap::new();

    for line in stdout.lines() {
        if matches.len() >= limit {
            break;
        }

        // Parse rg output: /path/to/file.md:LINE_NUM:content
        let Some(first_colon) = line.find(':') else {
            continue;
        };
        let path = PathBuf::from(&line[..first_colon]);
        let rest = &line[first_colon + 1..];

        let Some(second_colon) = rest.find(':') else {
            continue;
        };
        let line_num: usize = rest[..second_colon].parse().unwrap_or(0);
        let content = &rest[second_colon + 1..];

        // Limit matches per file
        let count = seen_files.entry(path.clone()).or_insert(0);
        if *count >= MAX_MATCHES_PER_SESSION {
            continue;
        }

        // Verify all terms match (rg matches any)
        let content_lower = content.to_lowercase();
        if !matches_all_terms(&content_lower, &query_terms_lower) {
            continue;
        }

        let file_name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let snippet = get_snippet(content, query, 80);

        matches.push(ObsidianMatch {
            file_path: path,
            file_name,
            snippet,
            line_number: line_num,
        });

        *count += 1;
    }

    matches
}

/// Pure Rust fallback for Obsidian search
fn search_obsidian_rust(query: &str, limit: usize, base: &Path) -> Vec<ObsidianMatch> {
    warn_ripgrep_not_available();

    let query_terms_lower: Vec<String> =
        query.split_whitespace().map(|s| s.to_lowercase()).collect();

    let md_files = find_markdown_files(base);

    let mut matches = Vec::new();
    let mut seen_files: HashMap<PathBuf, usize> = HashMap::new();

    'outer: for file_path in md_files {
        let Ok(file) = File::open(&file_path) else {
            continue;
        };
        let reader = BufReader::new(file);

        for (line_idx, line_result) in reader.lines().enumerate() {
            if matches.len() >= limit {
                break 'outer;
            }

            let Ok(line) = line_result else {
                continue;
            };

            let count = seen_files.entry(file_path.clone()).or_insert(0);
            if *count >= MAX_MATCHES_PER_SESSION {
                break; // Move to next file
            }

            let line_lower = line.to_lowercase();
            if !matches_all_terms(&line_lower, &query_terms_lower) {
                continue;
            }

            let file_name = file_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let snippet = get_snippet(&line, query, 80);

            matches.push(ObsidianMatch {
                file_path: file_path.clone(),
                file_name,
                snippet,
                line_number: line_idx + 1,
            });

            *count += 1;
        }
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
        if let Some(ref title) = m.custom_title {
            println!("      Label:    {title}");
        }
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
        // Print copy-pasteable resume command
        if !m.session_id.is_empty() {
            println!(
                "      Resume:   {}",
                resume_command(&project_short, &m.session_id, m.config_dir.as_deref())
            );
        }
        println!();
    }

    println!("{sep}");
    println!("  Tip: Use --deep to search inside message content.");
    println!("{sep}\n");
}

/// One `--format jsonl` row: a result's fields plus `kind`, the mode that
/// produced it, which tells a consumer which fields to expect
#[derive(Serialize)]
struct JsonlRow<'a, T> {
    kind: &'static str,
    #[serde(flatten)]
    result: &'a T,
}

fn print_jsonl<T: Serialize>(kind: &'static str, matches: &[T], limit: usize) {
    let mut out = std::io::stdout().lock();
    for result in matches.iter().take(limit) {
        let line = match serde_json::to_string(&JsonlRow { kind, result }) {
            Ok(line) => line,
            Err(error) => {
                eprintln!("ERROR: failed to serialize search result: {error}");
                std::process::exit(1);
            }
        };
        if let Err(error) = writeln!(out, "{line}").and_then(|()| out.flush()) {
            // The reader closed the pipe (`| head -1`): it has what it wanted
            if error.kind() == std::io::ErrorKind::BrokenPipe {
                std::process::exit(0);
            }
            eprintln!("ERROR: failed to write search result: {error}");
            std::process::exit(1);
        }
    }
}

fn print_deep_results(matches: &[DeepMatch], query: &str, limit: usize, is_openclaw: bool) {
    let total = matches.len();
    let displayed = &matches[..total.min(limit)];

    let sep = "=".repeat(60);
    let source = if is_openclaw {
        "OPENCLAW"
    } else {
        "CLAUDE CODE"
    };
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
            .map(String::from)
            .or_else(|| {
                m.first_prompt
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|p| truncate(p, 120))
            })
            .unwrap_or_else(|| "(no summary)".to_string());

        println!("  [{}] [{}] {}", i + 1, role, label);
        if let Some(ref title) = m.custom_title {
            println!("      Label:    {title}");
        }
        println!("      Project:  {project_short}");
        println!("      Date:     {ts}");
        let clean_snippet: String = m.snippet.split_whitespace().collect::<Vec<_>>().join(" ");
        println!("      Snippet:  {clean_snippet}");
        println!("      Session:  {}", m.session_id);
        // Print copy-pasteable resume command (Claude Code only, not OpenClaw)
        if !is_openclaw && m.project_path != "unknown" && !m.session_id.is_empty() {
            println!(
                "      Resume:   {}",
                resume_command(&project_short, &m.session_id, m.config_dir.as_deref())
            );
        }
        println!();
    }

    println!("{sep}\n");
}

fn print_obsidian_results(matches: &[ObsidianMatch], query: &str, limit: usize, vault_path: &Path) {
    let total = matches.len();
    let displayed = &matches[..total.min(limit)];

    let sep = "=".repeat(60);
    let vault_name = vault_path.file_name().unwrap_or_default().to_string_lossy();
    println!("\n{sep}");
    println!("  OBSIDIAN SEARCH ({}): \"{}\"", vault_name, query);
    if total > limit {
        println!("  {total} matches found (showing top {limit})");
    } else {
        println!("  {total} matches found");
    }
    println!("{sep}\n");

    if displayed.is_empty() {
        println!("  No matches found in vault.\n");
        return;
    }

    for (i, m) in displayed.iter().enumerate() {
        let path_short = format_project_path(&m.file_path.to_string_lossy());
        let clean_snippet: String = m.snippet.split_whitespace().collect::<Vec<_>>().join(" ");

        println!("  [{}] {}", i + 1, m.file_name);
        println!("      File:     {path_short}");
        println!("      Line:     {}", m.line_number);
        println!("      Snippet:  {clean_snippet}");
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

    // Build date range from CLI args
    let date_range = match build_date_range(
        cli.since.as_deref(),
        cli.until.as_deref(),
        cli.date.as_deref(),
    ) {
        Ok(dr) => dr,
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    };

    if let Some(ref vault_path) = cli.obsidian {
        // Obsidian mode
        if !vault_path.exists() {
            eprintln!(
                "ERROR: Obsidian vault directory not found: {}",
                vault_path.display()
            );
            std::process::exit(1);
        }
        if !vault_path.is_dir() {
            eprintln!(
                "ERROR: Obsidian vault path is not a directory: {}",
                vault_path.display()
            );
            std::process::exit(1);
        }

        let matches = search_obsidian(&query, cli.limit, vault_path);
        if matches!(cli.format, OutputFormat::Jsonl) {
            print_jsonl("obsidian", &matches, cli.limit);
        } else {
            print_obsidian_results(&matches, &query, cli.limit, vault_path);
        }
    } else if cli.openclaw {
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

        let matches = search_deep_openclaw(&query, cli.limit, &base, &date_range);
        if matches!(cli.format, OutputFormat::Jsonl) {
            print_jsonl("openclaw", &matches, cli.limit);
        } else {
            print_deep_results(&matches, &query, cli.limit, true);
        }
    } else {
        // Claude Code mode. Transcripts are split across config roots
        // (~/.claude, ~/.claude-work, ...), so every root is searched and the
        // results merged; searching only one silently hides whole accounts.
        let bases = claude_projects_dirs();
        if bases.is_empty() {
            let pattern = dirs::home_dir()
                .map(|h| h.join(".claude*").join("projects").display().to_string())
                .unwrap_or_else(|| "~/.claude*/projects (home directory unknown)".to_string());
            eprintln!(
                "ERROR: no Claude projects directory found (looked in $CLAUDE_CONFIG_DIR/projects and {pattern})"
            );
            std::process::exit(1);
        }

        let project_filter = cli.project.as_deref();
        let resume_dirs: Vec<Option<PathBuf>> =
            bases.iter().map(|base| resume_config_dir(base)).collect();

        if cli.deep {
            let owners = session_owners(&bases, project_filter);
            if let Some(filter) = project_filter
                && bases
                    .iter()
                    .all(|base| resolve_search_paths(base, Some(filter)).is_empty())
            {
                eprintln!(
                    "NOTE: --project {filter:?} matched no project directory in any config root."
                );
            }
            let found: Vec<(usize, DeepMatch)> = bases
                .iter()
                .enumerate()
                .flat_map(|(root, base)| {
                    let owned = |id: &str| owners.get(id).is_none_or(|&owner| owner == root);
                    let owns_any = owners.is_empty() || owners.values().any(|&o| o == root);
                    search_deep_claude(
                        &query,
                        cli.limit,
                        project_filter,
                        base,
                        &date_range,
                        &owned,
                        owns_any,
                    )
                    .into_iter()
                    .map(move |hit| (root, hit))
                })
                .collect();
            // Pick each session's hits before interleaving, so copies shown
            // from another root don't take a root's turns in the round-robin.
            // A root's own sessions go ahead of copies it shows for another
            // root's sessions (the owner may simply have hit its --limit).
            let mut own: Vec<Vec<DeepMatch>> = bases.iter().map(|_| Vec::new()).collect();
            let mut copies: Vec<Vec<DeepMatch>> = bases.iter().map(|_| Vec::new()).collect();
            for (root, hit) in pick_session_hits(
                found,
                |m| &m.session_id,
                &owners,
                |m, _, owner| {
                    m.config_dir.clone_from(&resume_dirs[owner]);
                    m.config_root = Some(config_root_for_projects(&bases[owner]));
                },
            ) {
                match owners.get(&hit.session_id) {
                    Some(&owner) if owner != root => copies[root].push(hit),
                    _ => own[root].push(hit),
                }
            }
            let mut per_root: Vec<std::vec::IntoIter<DeepMatch>> = own
                .into_iter()
                .zip(copies)
                .map(|(mut own, copies)| {
                    own.extend(copies);
                    own.into_iter()
                })
                .collect();
            // Deep matches carry no comparable score (ripgrep returns file
            // order), so a plain concatenation lets the first root consume the
            // whole limit and hide every other account. Round-robin instead, so
            // a low --limit shows a spread across accounts rather than only the
            // alphabetically-first one.
            let mut matches = Vec::new();
            loop {
                let mut progressed = false;
                for hits in per_root.iter_mut() {
                    if let Some(hit) = hits.next() {
                        matches.push(hit);
                        progressed = true;
                    }
                }
                if !progressed {
                    break;
                }
            }
            if matches!(cli.format, OutputFormat::Jsonl) {
                print_jsonl("deep", &matches, cli.limit);
            } else {
                print_deep_results(&matches, &query, cli.limit, false);
            }
        } else {
            let mut merged = Vec::new();
            for (root, base) in bases.iter().enumerate() {
                merged.extend(
                    search_index(&query, project_filter, base, &date_range)
                        .into_iter()
                        .map(|m| (root, m)),
                );
            }
            // Each root sorted its own results; re-sort so the merged list is
            // ranked globally rather than grouped by which root it came from.
            merged.sort_by(|(_, a), (_, b)| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.modified.cmp(&a.modified))
            });
            let owners = index_owners(&bases, &merged);
            let mut matches: Vec<IndexMatch> = pick_session_hits(
                merged,
                |m| &m.session_id,
                &owners,
                |m, root, owner| {
                    m.config_dir.clone_from(&resume_dirs[owner]);
                    m.config_root = config_root_for_projects(&bases[owner]);
                    // Read the custom title from the owner's transcript
                    if owner != root
                        && let Some(file) = &m.session_file
                        && let Ok(rel) = file.strip_prefix(&bases[root])
                    {
                        m.session_file = Some(bases[owner].join(rel));
                    }
                },
            )
            .into_iter()
            .map(|(_, m)| m)
            .collect();
            // Titles live in the transcripts, so only read them for displayed results
            for m in matches.iter_mut().take(cli.limit) {
                // An index can list a session whose transcript is gone
                m.session_file = m.session_file.take().filter(|path| path.is_file());
                if let Some(path) = &m.session_file {
                    m.custom_title = read_custom_title(path);
                }
            }
            if matches!(cli.format, OutputFormat::Jsonl) {
                print_jsonl("index", &matches, cli.limit);
            } else {
                print_index_results(&matches, &query, cli.limit);
            }
        }
    }
}
