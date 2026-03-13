use serde::Deserialize;
use std::io::BufReader;
use std::path::Path;

#[derive(Deserialize)]
pub struct TraceFile {
    #[serde(rename = "traceEvents")]
    pub trace_events: Vec<TraceEvent>,
}

#[derive(Deserialize, Clone)]
pub struct TraceEvent {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub ph: String,
    #[serde(default)]
    pub ts: f64,
    #[serde(default)]
    pub dur: Option<f64>,
    #[serde(default)]
    pub tid: u64,
    #[serde(default)]
    #[allow(dead_code)]
    pub pid: u64,
    #[serde(default)]
    #[allow(dead_code)]
    pub cat: Option<String>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

pub fn parse_trace(path: &Path) -> Result<TraceFile, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let trace: TraceFile = serde_json::from_reader(reader)?;
    Ok(trace)
}

/// Detect main thread: first RunTask with dur > 500ms
pub fn detect_main_thread(events: &[TraceEvent]) -> u64 {
    for e in events {
        if e.name == "RunTask" && e.ph == "X" {
            if let Some(dur) = e.dur {
                if dur > 500_000.0 {
                    return e.tid;
                }
            }
        }
    }
    // Fallback: tid with most RunTask events
    let mut counts: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for e in events {
        if e.name == "RunTask" && e.ph == "X" {
            *counts.entry(e.tid).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(tid, _)| tid)
        .unwrap_or(0)
}
