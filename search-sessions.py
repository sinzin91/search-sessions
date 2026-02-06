#!/usr/bin/env python3
"""Search across all Claude Code session history.

Usage:
    search-sessions.py <query> [--deep] [--limit N] [--project FILTER]

Modes:
    Index search (default): Searches session metadata (instant)
    Deep search (--deep):   Searches full JSONL message content via ripgrep
"""

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Optional


# ─── Constants ───────────────────────────────────────────────────────

CLAUDE_DIR = Path.home() / ".claude"
PROJECTS_DIR = CLAUDE_DIR / "projects"
MAX_SNIPPET_LEN = 200
DEFAULT_LIMIT = 20


# ─── Data Classes ────────────────────────────────────────────────────


@dataclass
class IndexMatch:
    """A match found in session index metadata."""

    session_id: str
    project_path: str
    first_prompt: str
    summary: str
    git_branch: str
    created: str
    modified: str
    message_count: int
    matched_field: str
    score: float = 0.0


@dataclass
class DeepMatch:
    """A match found in JSONL message content."""

    session_id: str
    project_path: str
    jsonl_path: str
    line_number: int
    message_type: str
    snippet: str
    timestamp: str
    summary: Optional[str] = None
    first_prompt: Optional[str] = None


# ─── Argument Parsing ────────────────────────────────────────────────


def parse_args(raw_args: list[str]) -> argparse.Namespace:
    """Parse command line arguments.

    Handles the quirk that skill files may pass all args as a single string.
    """
    parser = argparse.ArgumentParser(
        description="Search Claude Code session history"
    )
    parser.add_argument(
        "query",
        nargs="+",
        help="Search query (words are ANDed together)",
    )
    parser.add_argument(
        "--deep",
        action="store_true",
        help="Search full message content (slower)",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=DEFAULT_LIMIT,
        help=f"Maximum results to show (default: {DEFAULT_LIMIT})",
    )
    parser.add_argument(
        "--project",
        type=str,
        default=None,
        help="Filter to sessions from projects matching this substring",
    )

    args = parser.parse_args(raw_args)
    args.query = " ".join(args.query)
    return args


# ─── Index Search ────────────────────────────────────────────────────


def find_all_index_files() -> list[Path]:
    """Discover all sessions-index.json files under ~/.claude/projects/."""
    if not PROJECTS_DIR.exists():
        return []
    return sorted(PROJECTS_DIR.glob("*/sessions-index.json"))


def load_index(index_path: Path) -> tuple[str, list[dict]]:
    """Load a sessions-index.json file, return (originalPath, entries)."""
    try:
        with open(index_path) as f:
            data = json.load(f)
        original_path = data.get("originalPath", str(index_path.parent.name))
        entries = data.get("entries", [])
        return original_path, entries
    except (json.JSONDecodeError, OSError):
        return str(index_path.parent.name), []


def score_index_entry(entry: dict, query_terms: list[str]) -> tuple[float, str]:
    """Score an index entry against query terms. Returns (score, matched_field).

    All query terms must appear in at least one searchable field (AND logic).
    Fields weighted: summary 3x, firstPrompt 2x, branch/path 1x.
    """
    searchable_fields = {
        "summary": (entry.get("summary", ""), 3.0),
        "firstPrompt": (entry.get("firstPrompt", ""), 2.0),
        "gitBranch": (entry.get("gitBranch", ""), 1.0),
        "projectPath": (entry.get("projectPath", ""), 1.0),
    }

    total_score = 0.0
    best_field = ""
    best_field_score = 0.0

    for term in query_terms:
        term_lower = term.lower()
        term_found = False

        for field_name, (field_value, weight) in searchable_fields.items():
            if term_lower in field_value.lower():
                term_found = True
                total_score += weight
                if weight > best_field_score:
                    best_field_score = weight
                    best_field = field_name

        if not term_found:
            return 0.0, ""

    return total_score, best_field


def search_index(
    query: str, project_filter: Optional[str] = None
) -> list[IndexMatch]:
    """Search all session index files for matching metadata."""
    query_terms = query.split()
    matches: list[IndexMatch] = []

    for index_path in find_all_index_files():
        original_path, entries = load_index(index_path)

        if project_filter and project_filter.lower() not in original_path.lower():
            continue

        for entry in entries:
            score, matched_field = score_index_entry(entry, query_terms)
            if score > 0:
                matches.append(
                    IndexMatch(
                        session_id=entry.get("sessionId", "unknown"),
                        project_path=entry.get("projectPath", original_path),
                        first_prompt=entry.get("firstPrompt", "")[
                            :MAX_SNIPPET_LEN
                        ],
                        summary=entry.get("summary", ""),
                        git_branch=entry.get("gitBranch", ""),
                        created=entry.get("created", ""),
                        modified=entry.get("modified", ""),
                        message_count=entry.get("messageCount", 0),
                        matched_field=matched_field,
                        score=score,
                    )
                )

    matches.sort(key=lambda m: (m.score, m.modified), reverse=True)
    return matches


# ─── Deep Search (via ripgrep) ───────────────────────────────────────


def build_rg_command(
    query: str, project_filter: Optional[str] = None
) -> list[str]:
    """Build a ripgrep command to search JSONL files."""
    search_path = str(PROJECTS_DIR)
    if project_filter:
        for d in PROJECTS_DIR.iterdir():
            if d.is_dir() and project_filter.lower() in d.name.lower():
                search_path = str(d)
                break

    return [
        "rg",
        "--no-heading",
        "--line-number",
        "--ignore-case",
        "--max-count",
        "5",
        "--glob",
        "*.jsonl",
        "--glob",
        "!**/subagents/**",
        "--glob",
        "!**/sessions-index.json",
        query,
        search_path,
    ]


def parse_rg_output(line: str) -> Optional[dict]:
    """Parse a single ripgrep output line.

    Format: /path/to/file.jsonl:LINE_NUM:json_content
    """
    # Split on first two colons to get path:linenum:content
    parts = line.split(":", 2)
    if len(parts) < 3:
        return None

    file_path = parts[0]
    try:
        line_number = int(parts[1])
    except ValueError:
        return None
    json_str = parts[2]

    try:
        record = json.loads(json_str)
    except json.JSONDecodeError:
        return None

    return {
        "file_path": file_path,
        "line_number": line_number,
        "record": record,
    }


def extract_text_from_message(record: dict) -> str:
    """Extract readable text from a JSONL message record.

    Handles:
    - User messages: record["message"]["content"] is a string
    - Assistant messages: record["message"]["content"] is a list of objects
    """
    msg = record.get("message", {})
    content = msg.get("content", "")

    if isinstance(content, str):
        return content

    if isinstance(content, list):
        texts = []
        for item in content:
            if isinstance(item, dict):
                if item.get("type") == "text":
                    texts.append(item.get("text", ""))
                elif item.get("type") == "tool_result":
                    texts.append(str(item.get("content", "")))
        return " ".join(texts)

    return str(content)


def get_snippet(text: str, query: str, context_chars: int = 80) -> str:
    """Extract a snippet around the first occurrence of query in text."""
    text_lower = text.lower()
    query_lower = query.lower()

    idx = text_lower.find(query_lower)
    if idx == -1:
        for term in query.split():
            idx = text_lower.find(term.lower())
            if idx != -1:
                break

    if idx == -1:
        return text[:MAX_SNIPPET_LEN]

    start = max(0, idx - context_chars)
    end = min(len(text), idx + len(query) + context_chars)

    snippet = text[start:end]
    if start > 0:
        snippet = "..." + snippet
    if end < len(text):
        snippet = snippet + "..."

    return snippet


def build_index_lookup() -> dict[str, dict]:
    """Build a lookup from sessionId -> index entry for cross-referencing."""
    lookup: dict[str, dict] = {}
    for index_path in find_all_index_files():
        original_path, entries = load_index(index_path)
        for entry in entries:
            sid = entry.get("sessionId", "")
            if sid:
                entry["_originalPath"] = original_path
                lookup[sid] = entry
    return lookup


def search_deep(
    query: str,
    limit: int = DEFAULT_LIMIT,
    project_filter: Optional[str] = None,
) -> list[DeepMatch]:
    """Search JSONL message content using ripgrep."""
    cmd = build_rg_command(query, project_filter)

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=60,
        )
    except FileNotFoundError:
        print(
            "ERROR: ripgrep (rg) not found. Install it: brew install ripgrep",
            file=sys.stderr,
        )
        sys.exit(1)
    except subprocess.TimeoutExpired:
        print(
            "WARNING: Search timed out after 60s. Try a more specific query.",
            file=sys.stderr,
        )
        return []

    if result.returncode not in (0, 1):  # 1 = no matches (ok)
        print(
            f"WARNING: ripgrep returned code {result.returncode}",
            file=sys.stderr,
        )
        if result.stderr:
            print(result.stderr[:500], file=sys.stderr)

    index_lookup = build_index_lookup()

    matches: list[DeepMatch] = []
    seen_sessions: dict[str, int] = {}

    for line in result.stdout.splitlines():
        if len(matches) >= limit:
            break

        parsed = parse_rg_output(line)
        if parsed is None:
            continue

        record = parsed["record"]
        record_type = record.get("type", "")

        if record_type not in ("user", "assistant"):
            continue

        session_id = record.get("sessionId", "")

        # Limit matches per session to avoid one session dominating
        session_count = seen_sessions.get(session_id, 0)
        if session_count >= 2:
            continue
        seen_sessions[session_id] = session_count + 1

        text = extract_text_from_message(record)
        if not text:
            continue

        # Verify the query actually appears in extracted text
        if not any(t.lower() in text.lower() for t in query.split()):
            continue

        snippet = get_snippet(text, query)

        index_entry = index_lookup.get(session_id, {})
        project_path = (
            record.get("cwd", "")
            or index_entry.get("projectPath", "")
            or index_entry.get("_originalPath", "unknown")
        )

        matches.append(
            DeepMatch(
                session_id=session_id,
                project_path=project_path,
                jsonl_path=parsed["file_path"],
                line_number=parsed["line_number"],
                message_type=record_type,
                snippet=snippet,
                timestamp=record.get("timestamp", ""),
                summary=index_entry.get("summary"),
                first_prompt=index_entry.get("firstPrompt", "")[:120],
            )
        )

    return matches


# ─── Output Formatting ───────────────────────────────────────────────


def format_date(iso_str: str) -> str:
    """Format ISO date string to readable format."""
    if not iso_str:
        return "unknown"
    try:
        dt = datetime.fromisoformat(iso_str.replace("Z", "+00:00"))
        return dt.strftime("%Y-%m-%d %H:%M")
    except (ValueError, TypeError):
        return iso_str[:16] if len(iso_str) >= 16 else iso_str


def format_project_path(path: str) -> str:
    """Shorten project path for display."""
    home = str(Path.home())
    if path.startswith(home):
        return "~" + path[len(home):]
    return path


def print_index_results(
    matches: list[IndexMatch], query: str, limit: int
) -> None:
    """Print index search results."""
    displayed = matches[:limit]

    print(f"\n{'=' * 60}")
    print(f'  INDEX SEARCH: "{query}"')
    total = len(matches)
    suffix = f" (showing top {limit})" if total > limit else ""
    print(f"  {total} matches found{suffix}")
    print(f"{'=' * 60}\n")

    if not displayed:
        print("  No matches found in session metadata.")
        print("  Tip: Try --deep to search full message content.\n")
        return

    for i, m in enumerate(displayed, 1):
        project_short = format_project_path(m.project_path)
        created = format_date(m.created)

        print(f"  [{i}] {m.summary or '(no summary)'}")
        print(f"      Project:  {project_short}")
        if m.git_branch:
            print(f"      Branch:   {m.git_branch}")
        print(f"      Date:     {created}")
        print(f"      Messages: {m.message_count}")
        print(f"      Matched:  {m.matched_field}")
        if m.first_prompt and m.matched_field != "firstPrompt":
            prompt_preview = m.first_prompt[:100]
            if len(m.first_prompt) > 100:
                prompt_preview += "..."
            print(f"      Prompt:   {prompt_preview}")
        print(f"      Session:  {m.session_id}")
        print()

    print(f"{'=' * 60}")
    print("  Tip: Use --deep to search inside message content.")
    print(f"{'=' * 60}\n")


def print_deep_results(
    matches: list[DeepMatch], query: str, limit: int
) -> None:
    """Print deep search results."""
    displayed = matches[:limit]

    print(f"\n{'=' * 60}")
    print(f'  DEEP SEARCH: "{query}"')
    total = len(matches)
    suffix = f" (showing top {limit})" if total > limit else ""
    print(f"  {total} matches found{suffix}")
    print(f"{'=' * 60}\n")

    if not displayed:
        print("  No matches found in session message content.")
        print("  Tip: Try without --deep to search metadata only.\n")
        return

    for i, m in enumerate(displayed, 1):
        project_short = format_project_path(m.project_path)
        ts = format_date(m.timestamp)
        role = "USER" if m.message_type == "user" else "ASST"

        label = m.summary or m.first_prompt or "(no summary)"
        print(f"  [{i}] [{role}] {label}")
        print(f"      Project:  {project_short}")
        print(f"      Date:     {ts}")
        # Clean up snippet for display (collapse whitespace, strip newlines)
        clean_snippet = " ".join(m.snippet.split())
        print(f"      Snippet:  {clean_snippet}")
        print(f"      Session:  {m.session_id}")
        print()

    print(f"{'=' * 60}\n")


# ─── Main ────────────────────────────────────────────────────────────


def main() -> None:
    args = parse_args(sys.argv[1:])

    if not PROJECTS_DIR.exists():
        print(
            f"ERROR: Claude projects directory not found: {PROJECTS_DIR}",
            file=sys.stderr,
        )
        sys.exit(1)

    if args.deep:
        matches = search_deep(args.query, args.limit, args.project)
        print_deep_results(matches, args.query, args.limit)
    else:
        matches = search_index(args.query, args.project)
        print_index_results(matches, args.query, args.limit)


if __name__ == "__main__":
    main()
