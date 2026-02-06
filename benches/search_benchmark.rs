use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::fs;
use std::path::PathBuf;
use std::collections::HashMap;

// Re-implement core functions for benchmarking
// (In a real project, these would be exposed from the main library)

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionIndex {
    #[serde(default)]
    original_path: String,
    #[serde(default)]
    entries: Vec<SessionIndexEntry>,
}

#[derive(serde::Deserialize, Clone)]
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

fn load_index(path: &std::path::Path) -> (String, Vec<SessionIndexEntry>) {
    let data = match fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => return (String::new(), vec![]),
    };
    let index: SessionIndex = match serde_json::from_str(&data) {
        Ok(i) => i,
        Err(_) => return (String::new(), vec![]),
    };
    (index.original_path, index.entries)
}

fn score_index_entry(entry: &SessionIndexEntry, query_terms: &[&str]) -> f64 {
    let fields: &[(&str, f64)] = &[
        (&entry.summary, 3.0),
        (&entry.first_prompt, 2.0),
        (&entry.git_branch, 1.0),
        (&entry.project_path, 1.0),
    ];

    let mut total_score = 0.0;

    for term in query_terms {
        let term_lower = term.to_lowercase();
        let mut term_found = false;

        for &(field_value, weight) in fields {
            if field_value.to_lowercase().contains(&term_lower) {
                term_found = true;
                total_score += weight;
            }
        }

        if !term_found {
            return 0.0;
        }
    }

    total_score
}

fn extract_content_array(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut texts = Vec::new();
            for item in arr {
                if let Some(t) = item.get("type").and_then(|t| t.as_str()) {
                    if t == "text" {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            texts.push(text.to_string());
                        }
                    }
                }
            }
            texts.join(" ")
        }
        _ => content.to_string(),
    }
}

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

fn matches_all_terms(text_lower: &str, query_terms_lower: &[String]) -> bool {
    query_terms_lower.iter().all(|term| text_lower.contains(term))
}

// Benchmarks

fn bench_index_loading(c: &mut Criterion) {
    let index_path = fixtures_dir().join("sessions-index.json");
    
    c.bench_function("load_index", |b| {
        b.iter(|| {
            load_index(black_box(&index_path))
        })
    });
}

fn bench_index_scoring(c: &mut Criterion) {
    let index_path = fixtures_dir().join("sessions-index.json");
    let (_, entries) = load_index(&index_path);
    
    let queries = vec![
        vec!["kubernetes"],
        vec!["docker", "compose"],
        vec!["rbac", "kubernetes", "pods"],
    ];
    
    let mut group = c.benchmark_group("index_scoring");
    for query in queries {
        group.bench_with_input(
            BenchmarkId::from_parameter(query.join("_")),
            &query,
            |b, q| {
                b.iter(|| {
                    for entry in &entries {
                        score_index_entry(black_box(entry), black_box(q));
                    }
                })
            },
        );
    }
    group.finish();
}

fn bench_jsonl_parsing(c: &mut Criterion) {
    let claude_path = fixtures_dir().join("claude-session.jsonl");
    let openclaw_path = fixtures_dir().join("openclaw-session.jsonl");
    
    let claude_content = fs::read_to_string(&claude_path).unwrap();
    let openclaw_content = fs::read_to_string(&openclaw_path).unwrap();
    
    let mut group = c.benchmark_group("jsonl_parsing");
    
    group.bench_function("claude_session", |b| {
        b.iter(|| {
            for line in claude_content.lines() {
                let _: serde_json::Value = serde_json::from_str(black_box(line)).unwrap();
            }
        })
    });
    
    group.bench_function("openclaw_session", |b| {
        b.iter(|| {
            for line in openclaw_content.lines() {
                let _: serde_json::Value = serde_json::from_str(black_box(line)).unwrap();
            }
        })
    });
    
    group.finish();
}

fn bench_text_extraction(c: &mut Criterion) {
    let openclaw_path = fixtures_dir().join("openclaw-session.jsonl");
    let content = fs::read_to_string(&openclaw_path).unwrap();
    
    let messages: Vec<serde_json::Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|v: &serde_json::Value| v.get("type").and_then(|t| t.as_str()) == Some("message"))
        .collect();
    
    c.bench_function("extract_text_openclaw", |b| {
        b.iter(|| {
            for msg in &messages {
                extract_text_openclaw(black_box(msg));
            }
        })
    });
}

fn bench_term_matching(c: &mut Criterion) {
    let texts = vec![
        "How do I configure the security audit schedule?",
        "You can configure the security audit schedule using a cron job.",
        "The security audit checks credential file permissions and exposed secrets.",
    ];
    
    let queries = vec![
        vec!["security".to_string()],
        vec!["security".to_string(), "audit".to_string()],
        vec!["security".to_string(), "audit".to_string(), "cron".to_string()],
    ];
    
    let mut group = c.benchmark_group("term_matching");
    
    for query in &queries {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_terms", query.len())),
            query,
            |b, q| {
                b.iter(|| {
                    for text in &texts {
                        let text_lower = text.to_lowercase();
                        matches_all_terms(black_box(&text_lower), black_box(q));
                    }
                })
            },
        );
    }
    
    group.finish();
}

fn bench_metadata_preload(c: &mut Criterion) {
    let openclaw_path = fixtures_dir().join("openclaw-session.jsonl");
    
    c.bench_function("preload_session_metadata", |b| {
        b.iter(|| {
            let content = fs::read_to_string(black_box(&openclaw_path)).unwrap();
            if let Some(first_line) = content.lines().next() {
                if let Ok(record) = serde_json::from_str::<serde_json::Value>(first_line) {
                    if record.get("type").and_then(|t| t.as_str()) == Some("session") {
                        let _cwd = record.get("cwd").and_then(|c| c.as_str()).unwrap_or("");
                        let _ts = record.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
                    }
                }
            }
        })
    });
}

criterion_group!(
    benches,
    bench_index_loading,
    bench_index_scoring,
    bench_jsonl_parsing,
    bench_text_extraction,
    bench_term_matching,
    bench_metadata_preload,
);

criterion_main!(benches);
