use crate::analysis::*;
use crate::app::App;

fn fmt_us(us: f64) -> String {
    if us.abs() < 0.01 {
        "0".to_string()
    } else if us.abs() >= 1_000_000.0 {
        format!("{:.2}s", us / 1_000_000.0)
    } else if us.abs() >= 1_000.0 {
        format!("{:.2}ms", us / 1_000.0)
    } else {
        format!("{:.0}us", us)
    }
}

fn pct_diff(a: f64, b: f64) -> String {
    if a > 0.0 {
        let d = (b - a) / a * 100.0;
        if d > 0.0 {
            format!("+{:.1}%", d)
        } else {
            format!("{:.1}%", d)
        }
    } else {
        "-".to_string()
    }
}

pub fn export_markdown(app: &App) -> String {
    let mut out = String::new();

    out.push_str(&format!("# Chrome Trace Analysis: {}\n\n", app.trace_name_a));
    // Metadata block
    if let Some(ref meta) = app.metadata {
        out.push_str("**Trace Info**\n\n");
        if let Some(ref url) = meta.page_url {
            out.push_str(&format!("- **URL**: {}\n", url));
        }
        if let Some(ref start) = meta.start_time {
            out.push_str(&format!("- **Recorded**: {}\n", start));
        }
        if let Some(cpu) = meta.cpu_throttling {
            if cpu > 1.0 {
                out.push_str(&format!("- **CPU Throttle**: {:.0}x (divide times by {:.0} for real-world)\n", cpu, cpu));
            }
        }
        if let Some(ref net) = meta.network_throttling {
            if !net.is_empty() && net != "No throttling" {
                out.push_str(&format!("- **Network**: {}\n", net));
            }
        }
        if let Some(dpr) = meta.host_dpr {
            out.push_str(&format!("- **DPR**: {}\n", dpr));
        }
        out.push('\n');
    } else if app.throttle_factor > 1.0 {
        out.push_str(&format!(
            "> **Note**: CPU throttle {:.0}x applied. Divide times by {:.0} for real-world estimates.\n\n",
            app.throttle_factor, app.throttle_factor
        ));
    }

    // ── Overview ──
    export_overview(&mut out, &app.summary, &app.trace_name_a);

    // ── Scroll Frame Analysis ──
    export_scroll_frames(&mut out, &app.scroll_frames);

    // ── CPU Profile ──
    export_cpu_profile(&mut out, &app.cpu_profile);

    // ── Layout Dirty ──
    export_layout_dirty(&mut out, &app.layout_dirty);

    // ── Style Recalc ──
    export_style_recalc(&mut out, &app.style_recalc);

    // ── Forced Reflow ──
    export_forced_reflows(&mut out, &app.forced_reflows);

    // ── Event Breakdown ──
    export_event_breakdown(&mut out, &app.summary);

    // ── Compare ──
    if let Some(ref cmp) = app.compare {
        let name_b = app.trace_name_b.as_deref().unwrap_or("B");
        export_compare(&mut out, cmp, &app.trace_name_a, name_b);
    }

    out
}

fn export_overview(out: &mut String, summary: &SummaryResult, name: &str) {
    let busy_pct = if summary.total_trace_duration_us > 0.0 {
        summary.main_thread_busy_us / summary.total_trace_duration_us * 100.0
    } else {
        0.0
    };

    out.push_str("## Overview\n\n");
    out.push_str(&format!("- **Trace**: {}\n", name));
    out.push_str(&format!(
        "- **Duration**: {}\n",
        fmt_us(summary.total_trace_duration_us)
    ));
    out.push_str(&format!(
        "- **Main Thread Busy**: {:.1}% ({})\n",
        busy_pct,
        fmt_us(summary.main_thread_busy_us)
    ));
    out.push_str(&format!(
        "- **Long Tasks (>50ms)**: {}\n",
        summary.long_task_count
    ));

    if !summary.long_tasks_top.is_empty() {
        out.push_str("- **Top Long Tasks**: ");
        let items: Vec<String> = summary
            .long_tasks_top
            .iter()
            .take(5)
            .map(|d| fmt_us(*d))
            .collect();
        out.push_str(&items.join(", "));
        out.push('\n');
    }
    out.push('\n');
}

fn export_scroll_frames(out: &mut String, sf: &ScrollFrameResult) {
    out.push_str("## Scroll Frame Analysis\n\n");

    if sf.tasks.is_empty() {
        out.push_str("No scroll tasks detected (RunTask containing ULT>50ms or FunctionCall>50ms).\n\n");
        return;
    }

    let avg = &sf.avg;
    let pct = &sf.percentiles;
    out.push_str(&format!("- **Scroll Tasks**: {}\n", sf.tasks.len()));
    out.push_str(&format!("- **Avg Duration**: {}\n", fmt_us(avg.dur_us)));
    out.push_str(&format!("- **P50**: {} / **P90**: {} / **P99**: {}\n", fmt_us(pct.p50_us), fmt_us(pct.p90_us), fmt_us(pct.p99_us)));
    out.push_str(&format!("- **Bottleneck**: {}\n", avg.bottleneck()));
    out.push('\n');

    out.push_str("### Average Breakdown (per task)\n\n");
    out.push_str("| Category | Time | % of Task |\n");
    out.push_str("|----------|------|-----------|\n");

    let categories = [
        ("JS (FunctionCall)", avg.js_us),
        ("Style (UpdateLayoutTree)", avg.ult_us),
        ("Layout", avg.layout_us),
        ("Paint", avg.paint_us),
        ("Composite (Layerize+Commit)", avg.composite_us),
        ("HitTest", avg.hit_test_us),
    ];

    for (name, us) in &categories {
        let pct = if avg.dur_us > 0.0 {
            us / avg.dur_us * 100.0
        } else {
            0.0
        };
        out.push_str(&format!("| {} | {} | {:.1}% |\n", name, fmt_us(*us), pct));
    }
    out.push('\n');

    // Top 10 worst tasks
    out.push_str("### Worst Scroll Tasks\n\n");
    out.push_str("| # | Duration | Bottleneck | JS | Style | Layout | Paint |\n");
    out.push_str("|---|----------|------------|-------|-------|--------|-------|\n");
    for (i, task) in sf.tasks.iter().take(10).enumerate() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            i + 1,
            fmt_us(task.dur_us),
            task.bottleneck(),
            fmt_us(task.js_us),
            fmt_us(task.ult_us),
            fmt_us(task.layout_us),
            fmt_us(task.paint_us),
        ));
    }
    out.push('\n');
}

fn export_cpu_profile(out: &mut String, cp: &CpuProfileResult) {
    out.push_str("## CPU Profile (Top Functions by Self-Time)\n\n");

    if cp.functions.is_empty() {
        out.push_str("No CPU profile data found (ProfileChunk events).\n\n");
        return;
    }

    let total = cp.total_sample_time_us;

    let app_pct = if total > 0.0 {
        cp.app_time_us / total * 100.0
    } else {
        0.0
    };
    let rt_pct = if total > 0.0 {
        cp.runtime_time_us / total * 100.0
    } else {
        0.0
    };
    let native_pct = if total > 0.0 {
        cp.native_time_us / total * 100.0
    } else {
        0.0
    };

    out.push_str(&format!("- **Total Sample Time**: {}\n", fmt_us(total)));
    out.push_str(&format!(
        "- **App Code**: {} ({:.1}%)\n",
        fmt_us(cp.app_time_us),
        app_pct
    ));
    out.push_str(&format!(
        "- **Runtime/Library**: {} ({:.1}%)\n",
        fmt_us(cp.runtime_time_us),
        rt_pct
    ));
    out.push_str(&format!(
        "- **Native/Browser**: {} ({:.1}%)\n\n",
        fmt_us(cp.native_time_us),
        native_pct
    ));

    out.push_str("| # | Source | Function | Self Time | % | Cumulative % | File |\n");
    out.push_str("|---|--------|----------|-----------|---|-------------|------|\n");

    let mut cum_pct = 0.0;
    for (i, f) in cp.functions.iter().take(30).enumerate() {
        let pct = if total > 0.0 {
            f.self_time_us / total * 100.0
        } else {
            0.0
        };
        cum_pct += pct;

        let name = if f.function_name.is_empty() {
            "(anonymous)"
        } else {
            &f.function_name
        };

        // Shorten URL to last path segment
        let short_url = if let Some(idx) = f.url.rfind('/') {
            &f.url[idx + 1..]
        } else {
            &f.url
        };

        out.push_str(&format!(
            "| {} | {} | {} | {} | {:.1}% | {:.1}% | {} |\n",
            i + 1,
            f.source_type.label(),
            name,
            fmt_us(f.self_time_us),
            pct,
            cum_pct,
            short_url,
        ));
    }
    out.push('\n');
}

fn export_layout_dirty(out: &mut String, ld: &LayoutDirtyResult) {
    out.push_str("## Layout Dirty Objects\n\n");

    if ld.entries.is_empty() {
        out.push_str("No Layout events with dirty object data found.\n\n");
        return;
    }

    out.push_str(&format!("- **Layout Events**: {}\n", ld.entries.len()));
    out.push_str(&format!("- **Avg Dirty Objects**: {:.0}\n", ld.avg_dirty));
    out.push_str(&format!("- **Max Dirty Objects**: {}\n", ld.max_dirty));
    out.push_str(&format!("- **Avg Dirty Ratio**: {:.1}%\n\n", ld.avg_ratio));

    out.push_str("### Top 10 Layout Events (by dirty count)\n\n");
    out.push_str("| # | Duration | Dirty | Total | Ratio |\n");
    out.push_str("|---|----------|-------|-------|-------|\n");
    for (i, e) in ld.entries.iter().take(10).enumerate() {
        let ratio = if e.total_count > 0 {
            e.dirty_count as f64 / e.total_count as f64 * 100.0
        } else {
            0.0
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {:.1}% |\n",
            i + 1,
            fmt_us(e.dur_us),
            e.dirty_count,
            e.total_count,
            ratio,
        ));
    }
    out.push('\n');
}

fn export_style_recalc(out: &mut String, sr: &StyleRecalcResult) {
    if sr.entries.is_empty() {
        return;
    }
    out.push_str("## Style Recalc (UpdateLayoutTree)\n\n");
    out.push_str(&format!("- **Events**: {}\n", sr.total_count));
    out.push_str(&format!("- **Avg Elements**: {:.0}\n", sr.avg_elements));
    out.push_str(&format!("- **Max Elements**: {}\n\n", sr.max_elements));

    out.push_str("| # | Duration | Elements |\n");
    out.push_str("|---|----------|----------|\n");
    for (i, e) in sr.entries.iter().take(10).enumerate() {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            i + 1,
            fmt_us(e.dur_us),
            e.element_count,
        ));
    }
    out.push('\n');
}

fn export_forced_reflows(out: &mut String, fr: &ForcedReflowResult) {
    if fr.entries.is_empty() {
        return;
    }
    out.push_str("## Forced Reflow (Layout Thrashing)\n\n");
    out.push_str(&format!("- **Tasks with thrashing**: {}\n", fr.entries.len()));
    out.push_str(&format!("- **Total reflows**: {}\n", fr.total_reflows));
    out.push_str(&format!("- **Total layout time**: {}\n\n", fmt_us(fr.total_layout_time_us)));

    out.push_str("| # | Task Duration | Reflows | Layout Time |\n");
    out.push_str("|---|---------------|---------|-------------|\n");
    for (i, e) in fr.entries.iter().take(10).enumerate() {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            i + 1,
            fmt_us(e.task_dur_us),
            e.reflow_count,
            fmt_us(e.layout_time_us),
        ));
    }
    out.push('\n');
}

fn export_event_breakdown(out: &mut String, summary: &SummaryResult) {
    out.push_str("## Event Breakdown\n\n");
    out.push_str("| Event | Total | Count | Avg | % of Trace |\n");
    out.push_str("|-------|-------|-------|-----|------------|\n");

    for stat in &summary.event_stats {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {:.1}% |\n",
            stat.name,
            fmt_us(stat.total_time_us),
            stat.count,
            fmt_us(stat.avg_time_us),
            stat.pct_of_trace,
        ));
    }
    out.push('\n');
}

fn export_compare(
    out: &mut String,
    cmp: &CompareResult,
    name_a: &str,
    name_b: &str,
) {
    out.push_str(&format!(
        "## Comparison: {} (A) vs {} (B)\n\n",
        name_a, name_b
    ));

    // Key Findings
    if !cmp.findings.is_empty() {
        out.push_str("### Key Findings\n\n");
        for f in &cmp.findings {
            let icon = match f.severity {
                FindingSeverity::Improved => "+",
                FindingSeverity::Regressed => "!",
                FindingSeverity::Neutral => "=",
            };
            let label = match f.severity {
                FindingSeverity::Improved => "IMPROVED",
                FindingSeverity::Regressed => "REGRESSED",
                FindingSeverity::Neutral => "NEUTRAL",
            };
            if f.detail.is_empty() {
                out.push_str(&format!("- **[{}]** {} {}: {}\n", icon, label, f.category, f.message));
            } else {
                out.push_str(&format!(
                    "- **[{}]** {} {}: {} ({})\n",
                    icon, label, f.category, f.message, f.detail
                ));
            }
        }
        out.push('\n');
    }

    // Summary stats comparison
    out.push_str("### Quick Stats\n\n");
    out.push_str("| Metric | A | B | Change |\n");
    out.push_str("|--------|---|---|--------|\n");

    let sa = &cmp.summary_a;
    let sb = &cmp.summary_b;

    out.push_str(&format!(
        "| Long Tasks | {} | {} | {} |\n",
        sa.long_task_count,
        sb.long_task_count,
        pct_diff(sa.long_task_count as f64, sb.long_task_count as f64),
    ));
    out.push_str(&format!(
        "| Worst Task | {} | {} | {} |\n",
        fmt_us(sa.long_tasks_top.first().copied().unwrap_or(0.0)),
        fmt_us(sb.long_tasks_top.first().copied().unwrap_or(0.0)),
        pct_diff(
            sa.long_tasks_top.first().copied().unwrap_or(0.0),
            sb.long_tasks_top.first().copied().unwrap_or(0.0)
        ),
    ));
    out.push_str(&format!(
        "| Main Thread Busy | {} | {} | {} |\n",
        fmt_us(sa.main_thread_busy_us),
        fmt_us(sb.main_thread_busy_us),
        pct_diff(sa.main_thread_busy_us, sb.main_thread_busy_us),
    ));
    out.push_str(&format!(
        "| Layout Dirty (avg) | {:.0} | {:.0} | {} |\n",
        cmp.layout_a.avg_dirty,
        cmp.layout_b.avg_dirty,
        pct_diff(cmp.layout_a.avg_dirty as f64, cmp.layout_b.avg_dirty as f64),
    ));
    if cmp.style_recalc_a.total_count > 0 || cmp.style_recalc_b.total_count > 0 {
        out.push_str(&format!(
            "| Style Elements (avg) | {:.0} | {:.0} | {} |\n",
            cmp.style_recalc_a.avg_elements,
            cmp.style_recalc_b.avg_elements,
            pct_diff(cmp.style_recalc_a.avg_elements, cmp.style_recalc_b.avg_elements),
        ));
        out.push_str(&format!(
            "| Style Elements (max) | {} | {} | {} |\n",
            cmp.style_recalc_a.max_elements,
            cmp.style_recalc_b.max_elements,
            pct_diff(cmp.style_recalc_a.max_elements as f64, cmp.style_recalc_b.max_elements as f64),
        ));
    }
    out.push('\n');

    // Scroll frame comparison
    if let (Some(avg_a), Some(avg_b)) = (&cmp.scroll_avg_a, &cmp.scroll_avg_b) {
        out.push_str("### Scroll Frame Comparison (per task avg)\n\n");
        out.push_str("| Category | A | B | Change |\n");
        out.push_str("|----------|---|---|--------|\n");

        let cats = [
            ("Duration", avg_a.dur_us, avg_b.dur_us),
            ("JS", avg_a.js_us, avg_b.js_us),
            ("Style (ULT)", avg_a.ult_us, avg_b.ult_us),
            ("Layout", avg_a.layout_us, avg_b.layout_us),
            ("Paint", avg_a.paint_us, avg_b.paint_us),
            ("Composite", avg_a.composite_us, avg_b.composite_us),
            ("HitTest", avg_a.hit_test_us, avg_b.hit_test_us),
        ];
        for (name, a, b) in &cats {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                name,
                fmt_us(*a),
                fmt_us(*b),
                pct_diff(*a, *b),
            ));
        }
        out.push_str(&format!(
            "\n- **A Bottleneck**: {} ({} tasks)\n",
            avg_a.bottleneck(),
            cmp.scroll_count_a
        ));
        out.push_str(&format!(
            "- **B Bottleneck**: {} ({} tasks)\n\n",
            avg_b.bottleneck(),
            cmp.scroll_count_b
        ));
    }

    // Event average comparison
    out.push_str("### Per-Event Average Comparison\n\n");
    out.push_str("| Event | Avg (A) | Avg (B) | Change |\n");
    out.push_str("|-------|---------|---------|--------|\n");
    for r in &cmp.rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            r.event_name,
            fmt_us(r.avg_a_us),
            fmt_us(r.avg_b_us),
            pct_diff(r.avg_a_us, r.avg_b_us),
        ));
    }
    out.push('\n');

    // CPU profile diff
    if !cmp.cpu_diff.is_empty() {
        out.push_str("### CPU Profile Diff (Top Functions by Impact)\n\n");
        out.push_str("| Source | Function | A % | B % | Change (pp) |\n");
        out.push_str("|--------|----------|-----|-----|-------------|\n");
        for d in cmp.cpu_diff.iter().take(20) {
            let name = if d.function_name.is_empty() {
                "(anonymous)"
            } else {
                &d.function_name
            };
            let pp = d.pct_b - d.pct_a;
            let pp_str = if pp > 0.0 {
                format!("+{:.1}pp", pp)
            } else {
                format!("{:.1}pp", pp)
            };
            out.push_str(&format!(
                "| {} | {} | {:.1}% | {:.1}% | {} |\n",
                d.source_type.label(),
                name,
                d.pct_a,
                d.pct_b,
                pp_str,
            ));
        }
        out.push('\n');
    }
}
