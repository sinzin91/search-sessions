#!/usr/bin/env python3
"""
Search OpenClaw session history.
Adapted from search-sessions for OpenClaw's JSONL format.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from datetime import datetime
from pathlib import Path

# OpenClaw session storage path
SESSIONS_DIR = Path.home() / ".openclaw" / "agents" / "main" / "sessions"

def get_session_metadata(session_path: Path) -> dict:
    """Extract metadata from first few lines of session JSONL."""
    meta = {
        "id": session_path.stem,
        "path": str(session_path),
        "timestamp": None,
        "first_prompt": None,
        "message_count": 0,
    }
    
    try:
        with open(session_path, 'r') as f:
            for i, line in enumerate(f):
                if i > 100:  # Don't read entire file for metadata
                    break
                try:
                    record = json.loads(line)
                    
                    # Get session start time
                    if record.get("type") == "session":
                        meta["timestamp"] = record.get("timestamp")
                    
                    # Get first user message
                    if record.get("type") == "message":
                        meta["message_count"] += 1
                        msg = record.get("message", {})
                        if msg.get("role") == "user" and not meta["first_prompt"]:
                            content = msg.get("content", [])
                            for item in content:
                                if item.get("type") == "text":
                                    text = item.get("text", "")
                                    # Skip heartbeat messages
                                    if "HEARTBEAT" not in text and "heartbeat" not in text.lower():
                                        meta["first_prompt"] = text[:200]
                                        break
                except json.JSONDecodeError:
                    continue
    except Exception as e:
        pass
    
    return meta


def index_search(query: str, limit: int = 20) -> list:
    """Search session metadata (first prompts, timestamps)."""
    results = []
    query_lower = query.lower()
    query_terms = query_lower.split()
    
    if not SESSIONS_DIR.exists():
        print(f"Sessions directory not found: {SESSIONS_DIR}", file=sys.stderr)
        return []
    
    for session_file in SESSIONS_DIR.glob("*.jsonl"):
        meta = get_session_metadata(session_file)
        
        # Score based on query match
        score = 0
        first_prompt = (meta.get("first_prompt") or "").lower()
        
        for term in query_terms:
            if term in first_prompt:
                score += 2
        
        if score > 0:
            meta["score"] = score
            results.append(meta)
    
    # Sort by score, then recency
    results.sort(key=lambda x: (x["score"], x.get("timestamp") or ""), reverse=True)
    return results[:limit]


def deep_search(query: str, limit: int = 20) -> list:
    """Search full message content using grep."""
    results = []
    
    if not SESSIONS_DIR.exists():
        print(f"Sessions directory not found: {SESSIONS_DIR}", file=sys.stderr)
        return []
    
    try:
        # Use grep to find matches (fallback from ripgrep)
        cmd = [
            "grep", "-i", "-r", "-l", query, str(SESSIONS_DIR)
        ]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        
        matching_files = [f for f in proc.stdout.strip().split("\n") if f]
        
        seen_sessions = set()
        
        for file_path in matching_files:
            if len(results) >= limit:
                break
                
            session_id = Path(file_path).stem
            if session_id in seen_sessions:
                continue
            
            # Search within the file for the actual match
            try:
                with open(file_path, 'r') as f:
                    for line in f:
                        try:
                            record = json.loads(line)
                            if record.get("type") != "message":
                                continue
                            
                            msg = record.get("message", {})
                            role = msg.get("role", "unknown")
                            content = msg.get("content", [])
                            
                            text_content = ""
                            for item in content:
                                if item.get("type") == "text":
                                    text_content += item.get("text", "") + " "
                            
                            # Check if query matches
                            if query.lower() not in text_content.lower():
                                continue
                            
                            # Skip if just a heartbeat
                            if "HEARTBEAT" in text_content:
                                continue
                            
                            # Extract snippet around match
                            query_lower = query.lower()
                            text_lower = text_content.lower()
                            idx = text_lower.find(query_lower)
                            if idx >= 0:
                                start = max(0, idx - 50)
                                end = min(len(text_content), idx + len(query) + 100)
                                snippet = "..." + text_content[start:end].strip() + "..."
                            else:
                                snippet = text_content[:150].strip() + "..."
                            
                            # Get session metadata
                            meta = get_session_metadata(Path(file_path))
                            
                            seen_sessions.add(session_id)
                            results.append({
                                "session_id": session_id,
                                "role": role.upper()[:4],
                                "first_prompt": meta.get("first_prompt", ""),
                                "timestamp": meta.get("timestamp"),
                                "snippet": snippet,
                                "path": file_path,
                            })
                            break  # One match per session is enough
                            
                        except json.JSONDecodeError:
                            continue
            except Exception:
                continue
        
        return results
                
    except FileNotFoundError:
        print("grep not found", file=sys.stderr)
        return []
    
    return results


def format_timestamp(ts: str) -> str:
    """Format ISO timestamp to readable date."""
    if not ts:
        return "Unknown"
    try:
        dt = datetime.fromisoformat(ts.replace("Z", "+00:00"))
        return dt.strftime("%Y-%m-%d %H:%M")
    except:
        return ts[:16] if ts else "Unknown"


def print_index_results(query: str, results: list):
    """Print index search results."""
    print("=" * 60)
    print(f'  INDEX SEARCH: "{query}"')
    print(f"  {len(results)} matches found")
    print("=" * 60)
    print()
    
    for i, r in enumerate(results, 1):
        first_prompt = r.get('first_prompt') or 'No prompt'
        print(f"  [{i}] {first_prompt[:60]}")
        print(f"      Date:     {format_timestamp(r.get('timestamp'))}")
        print(f"      Messages: {r.get('message_count', '?')}")
        print(f"      Session:  {r.get('id')}")
        print()
    
    print("=" * 60)
    print("  Tip: Use --deep to search inside message content.")
    print("=" * 60)


def print_deep_results(query: str, results: list):
    """Print deep search results."""
    print("=" * 60)
    print(f'  DEEP SEARCH: "{query}"')
    print(f"  {len(results)} matches found")
    print("=" * 60)
    print()
    
    for i, r in enumerate(results, 1):
        first_prompt = r.get('first_prompt') or 'Unknown session'
        snippet = r.get('snippet') or ''
        print(f"  [{i}] [{r.get('role', '?')}] {first_prompt[:50]}")
        print(f"      Date:    {format_timestamp(r.get('timestamp'))}")
        print(f"      Snippet: {snippet[:100]}")
        print(f"      Session: {r.get('session_id')}")
        print()
    
    print("=" * 60)


def main():
    parser = argparse.ArgumentParser(description="Search OpenClaw session history")
    parser.add_argument("query", help="Search query")
    parser.add_argument("--deep", action="store_true", help="Search full message content")
    parser.add_argument("--limit", type=int, default=20, help="Max results (default: 20)")
    
    args = parser.parse_args()
    
    if args.deep:
        results = deep_search(args.query, args.limit)
        print_deep_results(args.query, results)
    else:
        results = index_search(args.query, args.limit)
        print_index_results(args.query, results)
    
    if not results:
        if not args.deep:
            print("\n  No results. Try --deep to search message content.\n")
        else:
            print("\n  No results. Try refining your query.\n")


if __name__ == "__main__":
    main()
