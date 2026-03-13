# chperf

Chrome DevTools Performance trace analyzer with TUI. Parses `{ "traceEvents": [...] }` JSON and surfaces actionable performance insights.

Chrome DevTools trace JSON files are massive and impractical to read by hand. chperf structures and summarizes the trace, then exports it as Markdown. Feed the exported result directly to an LLM like Claude Code for instant bottleneck identification and improvement suggestions.

## Install

```sh
cargo install --path .
```

Requires Rust 2024 edition (1.85+).

## Usage

```sh
# Single trace analysis (TUI)
chperf trace.json

# Compare two traces
chperf before.json --compare after.json

# Export as Markdown (to stdout)
chperf trace.json --export

# Export to file
chperf trace.json --export=report.md

# Compare + export
chperf before.json --compare after.json --export

# Compare + summary only (PR-friendly)
chperf before.json --compare after.json --export --summary

# Manual CPU throttle override (auto-detected from trace metadata when available)
chperf trace.json --throttle 6
```

## Features

### Tabs

| Tab | What it shows |
|-----|---------------|
| **Summary** | Trace metadata, main thread busy %, long tasks (>50ms) with histogram, event breakdown table, forced reflow detection, style recalc element counts |
| **Scroll Frames** | Scroll tasks (RunTask containing ULT>50ms or FunctionCall>50ms), avg/P50/P90/P99 duration, bottleneck analysis, per-task breakdown bars (JS/Style/Layout/Paint/Composite/HitTest/Other) |
| **CPU Profile** | Top functions by self-time from ProfileChunk events, source classification (App/Runtime/Native), stacked distribution bar |
| **Layout Dirty** | Layout events with dirty/total object counts, avg dirty ratio |
| **Compare** | Side-by-side scroll breakdown bars, key findings (auto-detected regressions/improvements), quick stats diff, style element count comparison, event average diff, CPU profile diff by percentage-point impact |

### Auto-detected Trace Metadata

Extracts from Chrome trace JSON:

- **Page URL** from `TracingStartedInBrowser` event
- **CPU Throttle** from `metadata.cpuThrottling` (e.g. 20x)
- **Record Time** from `metadata.startTime`
- **DPR** from `metadata.hostDPR`
- **Network Throttle** from `metadata.networkThrottling` (when set)

### Compare Findings

Automatically detects and flags:

- Long task count changes
- Scroll frame duration / bottleneck shifts
- JS, Style (ULT), Layout, Paint, HitTest, Composite time regressions/improvements
- Layout dirty object changes
- Style recalc element count changes (avg/max)

### Markdown Export

Exports structured Markdown (~7KB for a compare report) suitable for feeding to AI for further analysis. Includes all analysis sections, trace metadata, and throttle context.

### Summary Export (`--summary`)

PR-friendly comparison summary for use with `--compare --export --summary`. Outputs a concise, sectioned Markdown report:

| Section | Content |
|---------|---------|
| **Overall** | P50-based verdict (Improved / Regressed / No significant change) |
| **Scroll Performance** | P50, P90, avg duration + per-category breakdown (JS/Style/Layout/Paint/HitTest/Composite) |
| **Root Cause** | Style element counts (avg/max), layout dirty objects, IntersectionObserver |
| **Regressions** | Scroll-related categories that regressed >5% |
| **Notes** | Long Tasks, Main Thread Busy (marked as including non-scroll tasks), GC (MajorGC/MinorGC) |

```sh
# Output summary to stdout
chperf before.json --compare after.json --export --summary

# Save to file
chperf before.json --compare after.json --export=summary.md --summary
```

## TUI Keybindings

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Next / previous tab |
| `1`-`5` | Jump to tab directly |
| `j` / `k` / `Up` / `Down` | Scroll |
| `Ctrl+d` / `Ctrl+u` | Page down / up |
| `g` / `G` | Top / bottom |
| `t` | Toggle CPU throttle display (trace time vs real time) |
| `e` | Export current analysis to `chperf-export-<name>.md` |
| `q` / `Esc` / `Ctrl+c` | Quit |

## Analyzed Events

`RunTask`, `UpdateLayoutTree`, `Layout`, `Paint`, `FunctionCall`, `FireAnimationFrame`, `Layerize`, `Commit`, `HitTest`, `IntersectionObserverController::computeIntersections`, `MajorGC`, `MinorGC`, `EvaluateScript`
