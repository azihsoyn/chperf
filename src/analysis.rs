use crate::trace::TraceEvent;
use std::collections::HashMap;

// ── Summary ──

#[derive(Clone)]
pub struct EventTypeStat {
    pub name: String,
    pub total_time_us: f64,
    pub count: usize,
    pub avg_time_us: f64,
    pub pct_of_trace: f64, // percentage of total trace time
}

#[derive(Clone)]
pub struct SummaryResult {
    pub long_task_count: usize,
    pub long_tasks_top: Vec<f64>,
    pub total_trace_duration_us: f64,
    pub main_thread_busy_us: f64, // total RunTask time on main thread
    pub event_stats: Vec<EventTypeStat>,
}

const TARGET_EVENTS: &[&str] = &[
    "RunTask",
    "UpdateLayoutTree",
    "Layout",
    "Paint",
    "FunctionCall",
    "FireAnimationFrame",
    "Layerize",
    "Commit",
    "HitTest",
    "IntersectionObserverController::computeIntersections",
    "MajorGC",
    "MinorGC",
    "EvaluateScript",
];

pub fn analyze_summary(events: &[TraceEvent], main_tid: u64) -> SummaryResult {
    let main_events: Vec<&TraceEvent> = events
        .iter()
        .filter(|e| e.tid == main_tid && e.ph == "X")
        .collect();

    // Long tasks
    let mut long_task_durs: Vec<f64> = main_events
        .iter()
        .filter(|e| e.name == "RunTask")
        .filter_map(|e| e.dur)
        .filter(|&d| d > 50_000.0)
        .collect();
    long_task_durs.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let long_task_count = long_task_durs.len();
    let long_tasks_top: Vec<f64> = long_task_durs.into_iter().take(10).collect();

    // Trace duration
    let min_ts = events.iter().map(|e| e.ts).fold(f64::INFINITY, f64::min);
    let max_ts = events.iter().fold(0.0f64, |acc, e| {
        let end = e.ts + e.dur.unwrap_or(0.0);
        acc.max(end)
    });
    let total_trace_duration_us = max_ts - min_ts;

    // Main thread busy time
    let main_thread_busy_us: f64 = main_events
        .iter()
        .filter(|e| e.name == "RunTask")
        .filter_map(|e| e.dur)
        .sum();

    // Event stats
    let mut stats_map: HashMap<&str, (f64, usize)> = HashMap::new();
    for e in &main_events {
        if TARGET_EVENTS.contains(&e.name.as_str()) {
            if let Some(dur) = e.dur {
                let entry = stats_map
                    .entry(TARGET_EVENTS.iter().find(|&&n| n == e.name).unwrap())
                    .or_default();
                entry.0 += dur;
                entry.1 += 1;
            }
        }
    }

    let mut event_stats: Vec<EventTypeStat> = stats_map
        .into_iter()
        .map(|(name, (total, count))| EventTypeStat {
            name: name.to_string(),
            total_time_us: total,
            count,
            avg_time_us: if count > 0 {
                total / count as f64
            } else {
                0.0
            },
            pct_of_trace: if total_trace_duration_us > 0.0 {
                total / total_trace_duration_us * 100.0
            } else {
                0.0
            },
        })
        .collect();
    event_stats.sort_by(|a, b| b.total_time_us.partial_cmp(&a.total_time_us).unwrap());

    SummaryResult {
        long_task_count,
        long_tasks_top,
        total_trace_duration_us,
        main_thread_busy_us,
        event_stats,
    }
}

// ── Scroll Frame Analysis ──

#[derive(Clone)]
pub struct FrameTask {
    #[allow(dead_code)]
    pub ts: f64,
    pub dur_us: f64,
    pub js_us: f64,
    pub ult_us: f64,
    pub paint_us: f64,
    pub composite_us: f64,
    pub hit_test_us: f64,
    pub layout_us: f64,
}

impl FrameTask {
    /// Returns the name of the dominant cost category
    pub fn bottleneck(&self) -> &'static str {
        let costs = [
            (self.js_us, "JS"),
            (self.ult_us, "Style"),
            (self.layout_us, "Layout"),
            (self.paint_us, "Paint"),
            (self.composite_us, "Composite"),
            (self.hit_test_us, "HitTest"),
        ];
        costs
            .iter()
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .map(|(_, name)| *name)
            .unwrap_or("?")
    }

    /// Returns breakdown as (label, value, color_index) sorted by value desc
    #[allow(dead_code)]
    pub fn breakdown(&self) -> Vec<(&'static str, f64)> {
        let mut v = vec![
            ("JS", self.js_us),
            ("Style", self.ult_us),
            ("Layout", self.layout_us),
            ("Paint", self.paint_us),
            ("Comp", self.composite_us),
            ("Hit", self.hit_test_us),
        ];
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        v
    }
}

#[derive(Clone)]
pub struct ScrollFramePercentiles {
    pub p50_us: f64,
    pub p90_us: f64,
    pub p99_us: f64,
}

#[derive(Clone)]
pub struct ScrollFrameResult {
    pub tasks: Vec<FrameTask>,
    pub avg: FrameTask,
    pub percentiles: ScrollFramePercentiles,
}

pub fn analyze_scroll_frames(events: &[TraceEvent], main_tid: u64) -> ScrollFrameResult {
    let main_x: Vec<&TraceEvent> = events
        .iter()
        .filter(|e| e.tid == main_tid && e.ph == "X")
        .collect();

    let run_tasks: Vec<&&TraceEvent> = main_x
        .iter()
        .filter(|e| e.name == "RunTask" && e.dur.is_some())
        .collect();

    let mut tasks = Vec::new();

    for rt in &run_tasks {
        let rt_ts = rt.ts;
        let rt_dur = rt.dur.unwrap();
        let rt_end = rt_ts + rt_dur;

        let children: Vec<&&TraceEvent> = main_x
            .iter()
            .filter(|e| {
                e.ts >= rt_ts && (e.ts + e.dur.unwrap_or(0.0)) <= rt_end && e.name != "RunTask"
            })
            .collect();

        let has_heavy_ult = children
            .iter()
            .any(|e| e.name == "UpdateLayoutTree" && e.dur.unwrap_or(0.0) > 50_000.0);
        let has_heavy_fc = children
            .iter()
            .any(|e| e.name == "FunctionCall" && e.dur.unwrap_or(0.0) > 50_000.0);

        if !has_heavy_ult && !has_heavy_fc {
            continue;
        }

        let mut ft = FrameTask {
            ts: rt_ts,
            dur_us: rt_dur,
            js_us: 0.0,
            ult_us: 0.0,
            paint_us: 0.0,
            composite_us: 0.0,
            hit_test_us: 0.0,
            layout_us: 0.0,
        };

        for c in &children {
            let d = c.dur.unwrap_or(0.0);
            match c.name.as_str() {
                "FunctionCall" => ft.js_us += d,
                "UpdateLayoutTree" => ft.ult_us += d,
                "Paint" => ft.paint_us += d,
                "Layerize" | "Commit" => ft.composite_us += d,
                "HitTest" => ft.hit_test_us += d,
                "Layout" => ft.layout_us += d,
                _ => {}
            }
        }
        tasks.push(ft);
    }

    let n = tasks.len().max(1) as f64;
    let avg = FrameTask {
        ts: 0.0,
        dur_us: tasks.iter().map(|t| t.dur_us).sum::<f64>() / n,
        js_us: tasks.iter().map(|t| t.js_us).sum::<f64>() / n,
        ult_us: tasks.iter().map(|t| t.ult_us).sum::<f64>() / n,
        paint_us: tasks.iter().map(|t| t.paint_us).sum::<f64>() / n,
        composite_us: tasks.iter().map(|t| t.composite_us).sum::<f64>() / n,
        hit_test_us: tasks.iter().map(|t| t.hit_test_us).sum::<f64>() / n,
        layout_us: tasks.iter().map(|t| t.layout_us).sum::<f64>() / n,
    };

    // Percentiles (sorted ascending by duration)
    let percentiles = if tasks.is_empty() {
        ScrollFramePercentiles {
            p50_us: 0.0,
            p90_us: 0.0,
            p99_us: 0.0,
        }
    } else {
        let mut durs: Vec<f64> = tasks.iter().map(|t| t.dur_us).collect();
        durs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let percentile = |p: f64| -> f64 {
            let idx = ((p / 100.0) * (durs.len() as f64 - 1.0)).round() as usize;
            durs[idx.min(durs.len() - 1)]
        };
        ScrollFramePercentiles {
            p50_us: percentile(50.0),
            p90_us: percentile(90.0),
            p99_us: percentile(99.0),
        }
    };

    ScrollFrameResult { tasks, avg, percentiles }
}

// ── CPU Profile ──

#[derive(Clone)]
pub struct FunctionTime {
    pub function_name: String,
    pub url: String,
    pub self_time_us: f64,
    pub source_type: SourceType,
}

#[derive(Clone, PartialEq, Eq)]
pub enum SourceType {
    Runtime,  // framework/library
    AppCode,  // user application code
    Native,   // browser internals / no URL
}

impl SourceType {
    pub fn label(&self) -> &'static str {
        match self {
            SourceType::Runtime => "runtime",
            SourceType::AppCode => "app",
            SourceType::Native => "native",
        }
    }
}

fn classify_url(url: &str) -> SourceType {
    if url.is_empty() {
        return SourceType::Native;
    }
    let lower = url.to_lowercase();
    if lower.contains("node_modules")
        || lower.contains("svelte")
        || lower.contains("react")
        || lower.contains("vue")
        || lower.contains("angular")
        || lower.contains("polyfill")
        || lower.contains("vendor")
        || lower.contains("chunk-")
        || lower.contains(".min.")
    {
        SourceType::Runtime
    } else {
        SourceType::AppCode
    }
}

#[derive(Clone)]
pub struct CpuProfileResult {
    pub functions: Vec<FunctionTime>,
    pub total_sample_time_us: f64,
    pub app_time_us: f64,
    pub runtime_time_us: f64,
    pub native_time_us: f64,
}

pub fn analyze_cpu_profile(events: &[TraceEvent]) -> CpuProfileResult {
    let mut node_map: HashMap<u64, (String, String)> = HashMap::new();
    let mut self_times: HashMap<u64, f64> = HashMap::new();

    for e in events {
        if e.name != "ProfileChunk" {
            continue;
        }
        let args = match &e.args {
            Some(a) => a,
            None => continue,
        };
        let data = match args.get("data") {
            Some(d) => d,
            None => continue,
        };
        let cpu_profile = match data.get("cpuProfile") {
            Some(cp) => cp,
            None => continue,
        };

        if let Some(nodes) = cpu_profile.get("nodes").and_then(|n| n.as_array()) {
            for node in nodes {
                let id = node.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                let call_frame = node.get("callFrame");
                let func_name = call_frame
                    .and_then(|cf| cf.get("functionName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("(anonymous)")
                    .to_string();
                let url = call_frame
                    .and_then(|cf| cf.get("url"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                node_map.entry(id).or_insert((func_name, url));
            }
        }

        let samples = cpu_profile.get("samples").and_then(|s| s.as_array());
        let time_deltas = data.get("timeDeltas").and_then(|t| t.as_array());

        if let (Some(samples), Some(deltas)) = (samples, time_deltas) {
            for (sample, delta) in samples.iter().zip(deltas.iter()) {
                let node_id = sample.as_u64().unwrap_or(0);
                let dt = delta.as_f64().unwrap_or(0.0);
                *self_times.entry(node_id).or_default() += dt;
            }
        }
    }

    let mut functions: Vec<FunctionTime> = self_times
        .into_iter()
        .map(|(id, time)| {
            let (name, url) = node_map.get(&id).cloned().unwrap_or_default();
            let source_type = classify_url(&url);
            FunctionTime {
                function_name: name,
                url,
                self_time_us: time,
                source_type,
            }
        })
        .filter(|f| f.self_time_us > 0.0)
        .collect();

    functions.sort_by(|a, b| b.self_time_us.partial_cmp(&a.self_time_us).unwrap());

    let total_sample_time_us: f64 = functions.iter().map(|f| f.self_time_us).sum();
    let app_time_us: f64 = functions
        .iter()
        .filter(|f| f.source_type == SourceType::AppCode)
        .map(|f| f.self_time_us)
        .sum();
    let runtime_time_us: f64 = functions
        .iter()
        .filter(|f| f.source_type == SourceType::Runtime)
        .map(|f| f.self_time_us)
        .sum();
    let native_time_us: f64 = functions
        .iter()
        .filter(|f| f.source_type == SourceType::Native)
        .map(|f| f.self_time_us)
        .sum();

    CpuProfileResult {
        functions,
        total_sample_time_us,
        app_time_us,
        runtime_time_us,
        native_time_us,
    }
}

// ── Style Recalc (UpdateLayoutTree) ──

#[derive(Clone)]
pub struct StyleRecalcEntry {
    pub dur_us: f64,
    pub element_count: u32,
}

#[derive(Clone)]
pub struct StyleRecalcResult {
    pub entries: Vec<StyleRecalcEntry>,
    pub avg_elements: f64,
    pub max_elements: u32,
    pub total_count: usize,
}

pub fn analyze_style_recalc(events: &[TraceEvent], main_tid: u64) -> StyleRecalcResult {
    let mut entries = Vec::new();

    for e in events {
        if e.tid != main_tid || e.name != "UpdateLayoutTree" || e.ph != "X" {
            continue;
        }
        let element_count = e
            .args
            .as_ref()
            .and_then(|a| a.get("elementCount"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        if element_count > 0 {
            entries.push(StyleRecalcEntry {
                dur_us: e.dur.unwrap_or(0.0),
                element_count,
            });
        }
    }

    entries.sort_by(|a, b| b.element_count.cmp(&a.element_count));

    let n = entries.len().max(1) as f64;
    let avg_elements = entries.iter().map(|e| e.element_count as f64).sum::<f64>() / n;
    let max_elements = entries.iter().map(|e| e.element_count).max().unwrap_or(0);
    let total_count = entries.len();

    StyleRecalcResult {
        entries,
        avg_elements,
        max_elements,
        total_count,
    }
}

// ── Layout Dirty Objects ──

#[derive(Clone)]
pub struct LayoutDirtyEntry {
    #[allow(dead_code)]
    pub ts: f64,
    pub dur_us: f64,
    pub dirty_count: u32,
    pub total_count: u32,
}

#[derive(Clone)]
pub struct LayoutDirtyResult {
    pub entries: Vec<LayoutDirtyEntry>,
    pub avg_dirty: f64,
    pub max_dirty: u32,
    pub avg_ratio: f64,
}

pub fn analyze_layout_dirty(events: &[TraceEvent], main_tid: u64) -> LayoutDirtyResult {
    let mut entries = Vec::new();

    for e in events {
        if e.tid != main_tid || e.name != "Layout" || e.ph != "X" {
            continue;
        }
        let args = match &e.args {
            Some(a) => a,
            None => continue,
        };
        let begin_data = match args.get("beginData") {
            Some(bd) => bd,
            None => continue,
        };

        let dirty = begin_data
            .get("dirtyObjects")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let total = begin_data
            .get("totalObjects")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        entries.push(LayoutDirtyEntry {
            ts: e.ts,
            dur_us: e.dur.unwrap_or(0.0),
            dirty_count: dirty,
            total_count: total,
        });
    }

    entries.sort_by(|a, b| b.dirty_count.cmp(&a.dirty_count));

    let n = entries.len().max(1) as f64;
    let avg_dirty = entries.iter().map(|e| e.dirty_count as f64).sum::<f64>() / n;
    let max_dirty = entries.iter().map(|e| e.dirty_count).max().unwrap_or(0);
    let avg_ratio = entries
        .iter()
        .filter(|e| e.total_count > 0)
        .map(|e| e.dirty_count as f64 / e.total_count as f64 * 100.0)
        .sum::<f64>()
        / n;

    LayoutDirtyResult {
        entries,
        avg_dirty,
        max_dirty,
        avg_ratio,
    }
}

// ── Forced Reflow (Layout Thrashing) Detection ──

#[derive(Clone)]
pub struct ForcedReflowEntry {
    pub task_dur_us: f64,
    pub reflow_count: usize, // number of JS→Layout/ULT alternations
    pub layout_time_us: f64,
}

#[derive(Clone)]
pub struct ForcedReflowResult {
    pub entries: Vec<ForcedReflowEntry>,
    pub total_reflows: usize,
    pub total_layout_time_us: f64,
}

/// Detect forced reflow: RunTask containing alternating FunctionCall→(Layout|UpdateLayoutTree)→FunctionCall pattern
pub fn analyze_forced_reflows(events: &[TraceEvent], main_tid: u64) -> ForcedReflowResult {
    let main_x: Vec<&TraceEvent> = events
        .iter()
        .filter(|e| e.tid == main_tid && e.ph == "X")
        .collect();

    let run_tasks: Vec<&&TraceEvent> = main_x
        .iter()
        .filter(|e| e.name == "RunTask" && e.dur.is_some())
        .collect();

    let mut entries = Vec::new();

    for rt in &run_tasks {
        let rt_ts = rt.ts;
        let rt_end = rt_ts + rt.dur.unwrap();

        // Get direct children sorted by timestamp
        let mut children: Vec<&&TraceEvent> = main_x
            .iter()
            .filter(|e| {
                e.ts >= rt_ts
                    && (e.ts + e.dur.unwrap_or(0.0)) <= rt_end
                    && e.name != "RunTask"
                    && (e.name == "FunctionCall"
                        || e.name == "Layout"
                        || e.name == "UpdateLayoutTree")
            })
            .collect();
        children.sort_by(|a, b| a.ts.partial_cmp(&b.ts).unwrap());

        // Look for JS→Layout/ULT alternation pattern
        let mut reflow_count = 0usize;
        let mut layout_time = 0.0f64;
        let mut last_was_js = false;

        for c in &children {
            match c.name.as_str() {
                "FunctionCall" => {
                    last_was_js = true;
                }
                "Layout" | "UpdateLayoutTree" => {
                    if last_was_js {
                        reflow_count += 1;
                        layout_time += c.dur.unwrap_or(0.0);
                    }
                    last_was_js = false;
                }
                _ => {}
            }
        }

        if reflow_count >= 2 {
            entries.push(ForcedReflowEntry {
                task_dur_us: rt.dur.unwrap(),
                reflow_count,
                layout_time_us: layout_time,
            });
        }
    }

    entries.sort_by(|a, b| b.reflow_count.cmp(&a.reflow_count));

    let total_reflows: usize = entries.iter().map(|e| e.reflow_count).sum();
    let total_layout_time_us: f64 = entries.iter().map(|e| e.layout_time_us).sum();

    ForcedReflowResult {
        entries,
        total_reflows,
        total_layout_time_us,
    }
}

// ── Compare ──

#[derive(Clone)]
#[allow(dead_code)]
pub struct CompareRow {
    pub event_name: String,
    pub count_a: usize,
    pub count_b: usize,
    pub avg_a_us: f64,
    pub avg_b_us: f64,
    pub diff_pct: f64,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct CpuFunctionDiff {
    pub function_name: String,
    pub url: String,
    pub source_type: SourceType,
    pub time_a_us: f64,
    pub time_b_us: f64,
    pub pct_a: f64,
    pub pct_b: f64,
    pub diff_pct: f64,
}

#[derive(Clone)]
pub struct Finding {
    pub severity: FindingSeverity,
    pub category: String,
    pub message: String,
    pub detail: String,
}

#[derive(Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FindingSeverity {
    Improved,
    Regressed,
    Neutral,
}

#[derive(Clone)]
pub struct CompareResult {
    pub rows: Vec<CompareRow>,
    pub scroll_avg_a: Option<FrameTask>,
    pub scroll_avg_b: Option<FrameTask>,
    pub scroll_count_a: usize,
    pub scroll_count_b: usize,
    pub summary_a: SummaryResult,
    pub summary_b: SummaryResult,
    pub cpu_diff: Vec<CpuFunctionDiff>,
    pub layout_a: LayoutDirtyResult,
    pub layout_b: LayoutDirtyResult,
    pub style_recalc_a: StyleRecalcResult,
    pub style_recalc_b: StyleRecalcResult,
    pub findings: Vec<Finding>,
}

fn pct_diff(a: f64, b: f64) -> f64 {
    if a > 0.0 {
        (b - a) / a * 100.0
    } else {
        0.0
    }
}

pub fn analyze_compare(
    summary_a: &SummaryResult,
    summary_b: &SummaryResult,
    scroll_a: &ScrollFrameResult,
    scroll_b: &ScrollFrameResult,
    cpu_a: &CpuProfileResult,
    cpu_b: &CpuProfileResult,
    layout_a: &LayoutDirtyResult,
    layout_b: &LayoutDirtyResult,
    style_recalc_a: &StyleRecalcResult,
    style_recalc_b: &StyleRecalcResult,
) -> CompareResult {
    // ── Event rows ──
    let map_a: HashMap<&str, &EventTypeStat> = summary_a
        .event_stats
        .iter()
        .map(|s| (s.name.as_str(), s))
        .collect();
    let map_b: HashMap<&str, &EventTypeStat> = summary_b
        .event_stats
        .iter()
        .map(|s| (s.name.as_str(), s))
        .collect();

    let mut all_names: Vec<&str> = map_a.keys().chain(map_b.keys()).copied().collect();
    all_names.sort();
    all_names.dedup();

    let mut rows: Vec<CompareRow> = all_names
        .into_iter()
        .map(|name| {
            let a = map_a.get(name);
            let b = map_b.get(name);
            let avg_a = a.map(|s| s.avg_time_us).unwrap_or(0.0);
            let avg_b = b.map(|s| s.avg_time_us).unwrap_or(0.0);
            CompareRow {
                event_name: name.to_string(),
                count_a: a.map(|s| s.count).unwrap_or(0),
                count_b: b.map(|s| s.count).unwrap_or(0),
                avg_a_us: avg_a,
                avg_b_us: avg_b,
                diff_pct: pct_diff(avg_a, avg_b),
            }
        })
        .collect();

    rows.sort_by(|a, b| b.diff_pct.abs().partial_cmp(&a.diff_pct.abs()).unwrap());

    // ── Scroll frame ──
    let scroll_avg_a = if scroll_a.tasks.is_empty() {
        None
    } else {
        Some(scroll_a.avg.clone())
    };
    let scroll_avg_b = if scroll_b.tasks.is_empty() {
        None
    } else {
        Some(scroll_b.avg.clone())
    };

    // ── CPU profile diff ──
    let total_a = cpu_a.total_sample_time_us;
    let total_b = cpu_b.total_sample_time_us;

    let mut func_map_a: HashMap<(&str, &str), &FunctionTime> = HashMap::new();
    for f in &cpu_a.functions {
        func_map_a
            .entry((&f.function_name, &f.url))
            .or_insert(f);
    }
    let mut func_map_b: HashMap<(&str, &str), &FunctionTime> = HashMap::new();
    for f in &cpu_b.functions {
        func_map_b
            .entry((&f.function_name, &f.url))
            .or_insert(f);
    }

    let mut all_funcs: Vec<(&str, &str)> = func_map_a
        .keys()
        .chain(func_map_b.keys())
        .copied()
        .collect();
    all_funcs.sort();
    all_funcs.dedup();

    let mut cpu_diff: Vec<CpuFunctionDiff> = all_funcs
        .into_iter()
        .map(|(name, url)| {
            let fa = func_map_a.get(&(name, url));
            let fb = func_map_b.get(&(name, url));
            let time_a = fa.map(|f| f.self_time_us).unwrap_or(0.0);
            let time_b = fb.map(|f| f.self_time_us).unwrap_or(0.0);
            let pct_a = if total_a > 0.0 { time_a / total_a * 100.0 } else { 0.0 };
            let pct_b = if total_b > 0.0 { time_b / total_b * 100.0 } else { 0.0 };
            let source = fa
                .map(|f| f.source_type.clone())
                .or_else(|| fb.map(|f| f.source_type.clone()))
                .unwrap_or(SourceType::Native);
            CpuFunctionDiff {
                function_name: name.to_string(),
                url: url.to_string(),
                source_type: source,
                time_a_us: time_a,
                time_b_us: time_b,
                pct_a,
                pct_b,
                diff_pct: pct_diff(time_a, time_b),
            }
        })
        .filter(|d| d.time_a_us > 0.0 || d.time_b_us > 0.0)
        .collect();

    // Sort by absolute diff in percentage points (most impactful first)
    cpu_diff.sort_by(|a, b| {
        (b.pct_b - b.pct_a)
            .abs()
            .partial_cmp(&(a.pct_b - a.pct_a).abs())
            .unwrap()
    });
    cpu_diff.truncate(30); // top 30

    // ── Findings ──
    let mut findings = Vec::new();

    // Long task comparison
    let lt_diff = pct_diff(
        summary_a.long_task_count as f64,
        summary_b.long_task_count as f64,
    );
    if lt_diff.abs() > 10.0 {
        findings.push(Finding {
            severity: if lt_diff < 0.0 {
                FindingSeverity::Improved
            } else {
                FindingSeverity::Regressed
            },
            category: "Long Tasks".to_string(),
            message: format!(
                "{} -> {} ({:+.0}%)",
                summary_a.long_task_count, summary_b.long_task_count, lt_diff
            ),
            detail: if lt_diff < 0.0 {
                "Fewer long tasks blocking the main thread".to_string()
            } else {
                "More long tasks blocking the main thread".to_string()
            },
        });
    }

    // Scroll frame duration
    if let (Some(sa), Some(sb)) = (&scroll_avg_a, &scroll_avg_b) {
        let dur_diff = pct_diff(sa.dur_us, sb.dur_us);
        if dur_diff.abs() > 5.0 {
            findings.push(Finding {
                severity: if dur_diff < 0.0 {
                    FindingSeverity::Improved
                } else {
                    FindingSeverity::Regressed
                },
                category: "Scroll Duration".to_string(),
                message: format!("{:+.1}% avg scroll task time", dur_diff),
                detail: format!(
                    "Bottleneck: {} -> {}",
                    sa.bottleneck(),
                    sb.bottleneck()
                ),
            });
        }

        // JS time
        let js_diff = pct_diff(sa.js_us, sb.js_us);
        if js_diff.abs() > 10.0 {
            findings.push(Finding {
                severity: if js_diff < 0.0 {
                    FindingSeverity::Improved
                } else {
                    FindingSeverity::Regressed
                },
                category: "JS in Scroll".to_string(),
                message: format!("{:+.1}% JS execution time", js_diff),
                detail: String::new(),
            });
        }

        // Style/ULT time
        let ult_diff = pct_diff(sa.ult_us, sb.ult_us);
        if ult_diff.abs() > 10.0 {
            findings.push(Finding {
                severity: if ult_diff < 0.0 {
                    FindingSeverity::Improved
                } else {
                    FindingSeverity::Regressed
                },
                category: "Style Recalc".to_string(),
                message: format!("{:+.1}% UpdateLayoutTree time", ult_diff),
                detail: String::new(),
            });
        }

        // Layout time
        let layout_diff = pct_diff(sa.layout_us, sb.layout_us);
        if layout_diff.abs() > 10.0 {
            findings.push(Finding {
                severity: if layout_diff < 0.0 {
                    FindingSeverity::Improved
                } else {
                    FindingSeverity::Regressed
                },
                category: "Layout".to_string(),
                message: format!("{:+.1}% layout time", layout_diff),
                detail: String::new(),
            });
        }

        // Paint time
        let paint_diff = pct_diff(sa.paint_us, sb.paint_us);
        if paint_diff.abs() > 10.0 {
            findings.push(Finding {
                severity: if paint_diff < 0.0 {
                    FindingSeverity::Improved
                } else {
                    FindingSeverity::Regressed
                },
                category: "Paint".to_string(),
                message: format!("{:+.1}% paint time", paint_diff),
                detail: String::new(),
            });
        }

        // HitTest time
        let hit_diff = pct_diff(sa.hit_test_us, sb.hit_test_us);
        if hit_diff.abs() > 10.0 {
            findings.push(Finding {
                severity: if hit_diff < 0.0 {
                    FindingSeverity::Improved
                } else {
                    FindingSeverity::Regressed
                },
                category: "HitTest".to_string(),
                message: format!("{:+.1}% hit test time", hit_diff),
                detail: String::new(),
            });
        }

        // Composite time
        let comp_diff = pct_diff(sa.composite_us, sb.composite_us);
        if comp_diff.abs() > 10.0 {
            findings.push(Finding {
                severity: if comp_diff < 0.0 {
                    FindingSeverity::Improved
                } else {
                    FindingSeverity::Regressed
                },
                category: "Composite".to_string(),
                message: format!("{:+.1}% composite time", comp_diff),
                detail: String::new(),
            });
        }
    }

    // Main thread busy
    let busy_diff = pct_diff(summary_a.main_thread_busy_us, summary_b.main_thread_busy_us);
    if busy_diff.abs() > 10.0 {
        findings.push(Finding {
            severity: if busy_diff < 0.0 {
                FindingSeverity::Improved
            } else {
                FindingSeverity::Regressed
            },
            category: "Main Thread".to_string(),
            message: format!("{:+.1}% total busy time", busy_diff),
            detail: String::new(),
        });
    }

    // Layout dirty
    let dirty_diff = pct_diff(layout_a.avg_dirty as f64, layout_b.avg_dirty as f64);
    if dirty_diff.abs() > 15.0 {
        findings.push(Finding {
            severity: if dirty_diff < 0.0 {
                FindingSeverity::Improved
            } else {
                FindingSeverity::Regressed
            },
            category: "Layout Dirty".to_string(),
            message: format!(
                "Avg dirty {:.0} -> {:.0} ({:+.1}%)",
                layout_a.avg_dirty, layout_b.avg_dirty, dirty_diff
            ),
            detail: String::new(),
        });
    }

    // Style recalc elements
    if style_recalc_a.total_count > 0 && style_recalc_b.total_count > 0 {
        let elem_diff = pct_diff(style_recalc_a.avg_elements, style_recalc_b.avg_elements);
        if elem_diff.abs() > 10.0 {
            findings.push(Finding {
                severity: if elem_diff < 0.0 {
                    FindingSeverity::Improved
                } else {
                    FindingSeverity::Regressed
                },
                category: "Style Elements".to_string(),
                message: format!(
                    "Avg {:.0} -> {:.0} ({:+.1}%)",
                    style_recalc_a.avg_elements, style_recalc_b.avg_elements, elem_diff
                ),
                detail: format!(
                    "Max {} -> {}",
                    style_recalc_a.max_elements, style_recalc_b.max_elements
                ),
            });
        }
    }

    // Sort findings: regressions first, then improvements
    findings.sort_by(|a, b| {
        let ord_a = match a.severity {
            FindingSeverity::Regressed => 0,
            FindingSeverity::Improved => 1,
            FindingSeverity::Neutral => 2,
        };
        let ord_b = match b.severity {
            FindingSeverity::Regressed => 0,
            FindingSeverity::Improved => 1,
            FindingSeverity::Neutral => 2,
        };
        ord_a.cmp(&ord_b)
    });

    CompareResult {
        rows,
        scroll_avg_a,
        scroll_avg_b,
        scroll_count_a: scroll_a.tasks.len(),
        scroll_count_b: scroll_b.tasks.len(),
        summary_a: summary_a.clone(),
        summary_b: summary_b.clone(),
        cpu_diff,
        layout_a: layout_a.clone(),
        layout_b: layout_b.clone(),
        style_recalc_a: style_recalc_a.clone(),
        style_recalc_b: style_recalc_b.clone(),
        findings,
    }
}
