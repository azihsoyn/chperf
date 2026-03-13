use crate::analysis::SourceType;
use crate::app::{App, Tab};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
};

// ── Constants ──

const FRAME_BUDGET_US: f64 = 16_667.0; // 60fps = ~16.67ms

// Category colors (consistent across all tabs)
const COLOR_JS: Color = Color::Cyan;
const COLOR_STYLE: Color = Color::Yellow;
const COLOR_LAYOUT: Color = Color::Magenta;
const COLOR_PAINT: Color = Color::Green;
const COLOR_COMPOSITE: Color = Color::Blue;
const COLOR_HITTEST: Color = Color::Red;

// ── Formatting helpers ──

fn fmt_time(us: f64, throttle: bool) -> String {
    let v = if throttle { us / 20.0 } else { us };
    if v.abs() < 0.01 {
        "-".to_string()
    } else if v.abs() >= 1_000_000.0 {
        format!("{:.2}s", v / 1_000_000.0)
    } else if v.abs() >= 1_000.0 {
        format!("{:.2}ms", v / 1_000.0)
    } else {
        format!("{:.0}us", v)
    }
}

/// Text gauge: [████████░░░░] 65.2%
fn text_gauge(ratio: f64, width: usize) -> Vec<Span<'static>> {
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    let fill_color = if ratio > 0.8 {
        Color::Red
    } else if ratio > 0.5 {
        Color::Yellow
    } else {
        Color::Green
    };

    vec![
        Span::styled("[", Style::default().fg(Color::DarkGray)),
        Span::styled("█".repeat(filled), Style::default().fg(fill_color)),
        Span::styled("░".repeat(empty), Style::default().fg(Color::DarkGray)),
        Span::styled("]", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" {:.1}%", ratio * 100.0),
            Style::default().fg(fill_color).add_modifier(Modifier::BOLD),
        ),
    ]
}

/// Horizontal bar with color
fn bar(value: f64, max_value: f64, max_width: usize) -> String {
    if max_value <= 0.0 || value <= 0.0 {
        return String::new();
    }
    let ratio = (value / max_value).min(1.0);
    let filled = (ratio * max_width as f64).round() as usize;
    let filled = filled.min(max_width);
    "█".repeat(filled)
}

/// Colored stacked bar showing proportions
fn stacked_bar_spans(parts: &[(&str, f64, Color)], total: f64, width: usize) -> Vec<Span<'static>> {
    if total <= 0.0 {
        return vec![];
    }
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (i, (_, value, color)) in parts.iter().enumerate() {
        let ratio = (*value / total).min(1.0);
        let chars = if i == parts.len() - 1 && *value > 0.0 {
            // Last item gets remaining space to avoid rounding gaps
            width.saturating_sub(used)
        } else {
            (ratio * width as f64).round() as usize
        };
        let chars = chars.min(width.saturating_sub(used));
        if chars > 0 {
            spans.push(Span::styled(
                "█".repeat(chars),
                Style::default().fg(*color),
            ));
            used += chars;
        }
    }
    // Fill remainder
    if used < width {
        spans.push(Span::styled(
            "░".repeat(width - used),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans
}

/// Colored mini bar for table rows
fn colored_mini_bar(parts: &[(f64, Color)], total: f64, width: usize) -> Vec<Span<'static>> {
    if total <= 0.0 {
        return vec![Span::styled(
            "░".repeat(width),
            Style::default().fg(Color::DarkGray),
        )];
    }
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (value, color) in parts {
        let ratio = (*value / total).min(1.0);
        let chars = (ratio * width as f64).round() as usize;
        let chars = chars.min(width.saturating_sub(used));
        if chars > 0 {
            spans.push(Span::styled(
                "▓".repeat(chars),
                Style::default().fg(*color),
            ));
            used += chars;
        }
    }
    if used < width {
        spans.push(Span::styled(
            "░".repeat(width - used),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans
}

/// Severity label for a duration
fn severity_label(us: f64) -> (&'static str, Color) {
    if us > 200_000.0 {
        ("CRITICAL", Color::Red)
    } else if us > 100_000.0 {
        ("SLOW", Color::LightRed)
    } else if us > 50_000.0 {
        ("WARN", Color::Yellow)
    } else {
        ("OK", Color::Green)
    }
}

/// How many frame budgets this duration takes
fn frame_budget_ratio(us: f64, throttle: bool) -> f64 {
    let v = if throttle { us / 20.0 } else { us };
    v / FRAME_BUDGET_US
}

/// Alternating row background
fn row_bg(idx: usize) -> Style {
    if idx % 2 == 0 {
        Style::default()
    } else {
        Style::default().bg(Color::Rgb(30, 30, 40))
    }
}

// ── Main draw ──

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(frame.area());

    draw_tabs(frame, app, chunks[0]);
    draw_content(frame, app, chunks[1]);
    draw_status(frame, app, chunks[2]);
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = app
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if *t == app.tab {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(format!(" {} {} ", i + 1, t.label()), style))
        })
        .collect();

    let idx = app.tabs.iter().position(|t| *t == app.tab).unwrap_or(0);

    let mut title_spans = vec![Span::styled(
        " chperf ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    if app.throttle_20x {
        title_spans.push(Span::styled(
            " 20x THROTTLE ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Line::from(title_spans)),
        )
        .select(idx)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(" | ", Style::default().fg(Color::DarkGray)));

    frame.render_widget(tabs, area);
}

fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    match app.tab {
        Tab::Summary => draw_summary(frame, app, area),
        Tab::ScrollFrames => draw_scroll_frames(frame, app, area),
        Tab::CpuProfile => draw_cpu_profile(frame, app, area),
        Tab::LayoutDirty => draw_layout_dirty(frame, app, area),
        Tab::Compare => draw_compare(frame, app, area),
    }
}

// ── Summary Tab ──

fn draw_summary(frame: &mut Frame, app: &App, area: Rect) {
    let has_diagnostics = app.forced_reflows.total_reflows > 0 || app.style_recalc.total_count > 0;
    let diag_height = if has_diagnostics { 5 } else { 0 };
    let chunks = Layout::vertical([
        Constraint::Length(10),          // overview panel
        Constraint::Length(diag_height), // diagnostics
        Constraint::Min(0),             // event stats table
    ])
    .split(area);

    draw_summary_overview(frame, app, chunks[0]);
    if has_diagnostics {
        draw_summary_diagnostics(frame, app, chunks[1]);
    }
    draw_summary_table(frame, app, chunks[if has_diagnostics { 2 } else { 1 }]);
}

fn draw_summary_overview(frame: &mut Frame, app: &App, area: Rect) {
    // Split into left (metrics) and right (long task histogram)
    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    draw_summary_metrics(frame, app, cols[0]);
    draw_long_task_histogram(frame, app, cols[1]);
}

fn draw_summary_metrics(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let s = &app.summary;

    let busy_pct = if s.total_trace_duration_us > 0.0 {
        s.main_thread_busy_us / s.total_trace_duration_us * 100.0
    } else {
        0.0
    };

    let (lt_verdict, lt_color) = if s.long_task_count > 20 {
        ("!! Heavily blocked", Color::Red)
    } else if s.long_task_count > 5 {
        ("! Several detected", Color::Yellow)
    } else if s.long_task_count > 0 {
        ("Minor occurrences", Color::Green)
    } else {
        ("None detected", Color::Green)
    };

    let worst_dur = s.long_tasks_top.first().copied().unwrap_or(0.0);
    let (worst_label, worst_color) = severity_label(worst_dur);

    let gauge_spans = text_gauge(busy_pct / 100.0, 20);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Duration: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                fmt_time(s.total_trace_duration_us, t),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(
            [vec![Span::styled(
                "  Main Thread: ",
                Style::default().fg(Color::DarkGray),
            )]]
            .into_iter()
            .chain(std::iter::once(gauge_spans))
            .flatten()
            .collect::<Vec<_>>(),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Long Tasks:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", s.long_task_count),
                Style::default()
                    .fg(lt_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", lt_verdict),
                Style::default().fg(lt_color),
            ),
        ]),
    ];

    if worst_dur > 0.0 {
        lines.push(Line::from(vec![
            Span::styled("  Worst:       ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                fmt_time(worst_dur, t),
                Style::default()
                    .fg(worst_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" [{}]", worst_label),
                Style::default()
                    .fg(worst_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " ({:.0}x frame budget)",
                    frame_budget_ratio(worst_dur, t)
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " {} ",
                app.trace_name_a
            ))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(panel, area);
}

fn draw_long_task_histogram(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let s = &app.summary;

    if s.long_tasks_top.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  No Long Tasks (>50ms) detected",
                Style::default().fg(Color::Green),
            )]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Long Tasks ")
                .title_style(Style::default().fg(Color::Green)),
        );
        frame.render_widget(msg, area);
        return;
    }

    let max_dur = s.long_tasks_top.first().copied().unwrap_or(1.0);
    let inner_width = area.width.saturating_sub(14) as usize; // space for label + border

    let lines: Vec<Line> = s
        .long_tasks_top
        .iter()
        .enumerate()
        .map(|(i, dur)| {
            let (_, color) = severity_label(*dur);
            let bar_len =
                ((*dur / max_dur) * inner_width as f64).round() as usize;
            let bar_len = bar_len.min(inner_width).max(1);
            Line::from(vec![
                Span::styled(
                    format!(" #{:<2} ", i + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:>8} ", fmt_time(*dur, t)),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled("█".repeat(bar_len), Style::default().fg(color)),
            ])
        })
        .collect();

    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Top Long Tasks ")
            .title_style(Style::default().fg(Color::Red)),
    );
    frame.render_widget(panel, area);
}

fn draw_summary_diagnostics(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: Forced Reflow
    let fr = &app.forced_reflows;
    let fr_lines = if fr.entries.is_empty() {
        vec![Line::from(vec![Span::styled(
            "  No layout thrashing detected",
            Style::default().fg(Color::Green),
        )])]
    } else {
        vec![
            Line::from(vec![
                Span::styled("  Tasks: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", fr.entries.len()),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Reflows: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", fr.total_reflows),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Layout time: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    fmt_time(fr.total_layout_time_us, t),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]),
        ]
    };
    let fr_panel = Paragraph::new(fr_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Forced Reflow (Layout Thrashing) ")
            .title_style(Style::default().fg(if fr.entries.is_empty() { Color::Green } else { Color::Red })),
    );
    frame.render_widget(fr_panel, cols[0]);

    // Right: Style Recalc
    let sr = &app.style_recalc;
    let sr_lines = if sr.entries.is_empty() {
        vec![Line::from(vec![Span::styled(
            "  No element count data",
            Style::default().fg(Color::DarkGray),
        )])]
    } else {
        vec![
            Line::from(vec![
                Span::styled("  Events: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", sr.total_count),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Avg elements: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.0}", sr.avg_elements),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Max: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", sr.max_elements),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]),
        ]
    };
    let sr_panel = Paragraph::new(sr_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Style Recalc (elementCount) ")
            .title_style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(sr_panel, cols[1]);
}

fn draw_summary_table(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let s = &app.summary;

    let max_total = s
        .event_stats
        .first()
        .map(|e| e.total_time_us)
        .unwrap_or(1.0);
    let bar_width = 25;

    let rows: Vec<Row> = s
        .event_stats
        .iter()
        .enumerate()
        .map(|(i, stat)| {
            let importance = match stat.name.as_str() {
                "RunTask" | "UpdateLayoutTree" | "Layout" => "***",
                "Paint" | "FunctionCall" | "FireAnimationFrame" => "** ",
                _ => "*  ",
            };
            let name_color = match stat.name.as_str() {
                "RunTask" | "UpdateLayoutTree" | "Layout" => Color::Red,
                "Paint" | "FunctionCall" | "FireAnimationFrame" => Color::Yellow,
                _ => Color::White,
            };

            let bar_str = bar(stat.total_time_us, max_total, bar_width);
            let bar_color = if stat.pct_of_trace > 10.0 {
                Color::Red
            } else if stat.pct_of_trace > 3.0 {
                Color::Yellow
            } else {
                Color::Green
            };

            let avg_label = severity_label(stat.avg_time_us);

            Row::new(vec![
                Cell::from(importance).style(Style::default().fg(Color::DarkGray)),
                Cell::from(stat.name.clone()).style(Style::default().fg(name_color)),
                Cell::from(fmt_time(stat.total_time_us, t)),
                Cell::from(format!("{}", stat.count))
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(fmt_time(stat.avg_time_us, t))
                    .style(Style::default().fg(avg_label.1)),
                Cell::from(format!("{:.1}%", stat.pct_of_trace)),
                Cell::from(bar_str).style(Style::default().fg(bar_color)),
            ])
            .style(row_bg(i))
        })
        .collect();

    let visible = visible_rows(&rows, app.scroll_offset, area.height as usize);

    let table = Table::new(
        visible,
        [
            Constraint::Length(3),
            Constraint::Percentage(22),
            Constraint::Percentage(11),
            Constraint::Percentage(8),
            Constraint::Percentage(11),
            Constraint::Percentage(7),
            Constraint::Min(25),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Event Breakdown ")
            .title_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    )
    .header(
        Row::new(vec!["", "Event", "Total", "Count", "Avg", "%", "Distribution"])
            .style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Cyan),
            )
            .bottom_margin(1),
    );

    frame.render_widget(table, area);
}

// ── Scroll Frames Tab ──

fn draw_scroll_frames(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(12), // avg breakdown panel
        Constraint::Min(0),    // task list
    ])
    .split(area);

    draw_scroll_avg(frame, app, chunks[0]);
    draw_scroll_tasks(frame, app, chunks[1]);
}

fn draw_scroll_avg(frame: &mut Frame, app: &App, area: Rect) {
    // Split into left (metrics) and right (bar + legend)
    let cols = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_scroll_metrics(frame, app, cols[0]);
    draw_scroll_breakdown_chart(frame, app, cols[1]);
}

fn draw_scroll_metrics(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let sf = &app.scroll_frames;
    let avg = &sf.avg;

    let bottleneck = avg.bottleneck();
    let (sev_label, sev_color) = severity_label(avg.dur_us);
    let frames_ratio = frame_budget_ratio(avg.dur_us, t);

    let budget_gauge = text_gauge((frames_ratio / 5.0).min(1.0), 15); // 5x budget = full gauge

    let pct = &sf.percentiles;

    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("  {} scroll tasks", sf.tasks.len()),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("[{}]", sev_label),
                Style::default()
                    .fg(sev_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Avg Duration:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                fmt_time(avg.dur_us, t),
                Style::default()
                    .fg(sev_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  P50: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                fmt_time(pct.p50_us, t),
                Style::default().fg(Color::White),
            ),
            Span::styled("  P90: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                fmt_time(pct.p90_us, t),
                Style::default().fg(severity_label(pct.p90_us).1),
            ),
            Span::styled("  P99: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                fmt_time(pct.p99_us, t),
                Style::default().fg(severity_label(pct.p99_us).1),
            ),
        ]),
        Line::from(
            [vec![Span::styled(
                "  Frame Budget:  ",
                Style::default().fg(Color::DarkGray),
            )]]
            .into_iter()
            .chain(std::iter::once(budget_gauge))
            .flatten()
            .chain(std::iter::once(Span::styled(
                format!(" ({:.1}x)", frames_ratio),
                Style::default().fg(Color::DarkGray),
            )))
            .collect::<Vec<_>>(),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Bottleneck:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} ", bottleneck),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                match bottleneck {
                    "JS" => "-> Optimize JS / reduce work",
                    "Style" => "-> Reduce CSS complexity / selectors",
                    "Layout" => "-> Reduce layout thrashing",
                    "Paint" => "-> Reduce paint area / use layers",
                    "Composite" => "-> Reduce layer count",
                    "HitTest" => "-> Simplify DOM structure",
                    _ => "",
                },
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];

    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Scroll Performance ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(panel, area);
}

fn draw_scroll_breakdown_chart(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let avg = &app.scroll_frames.avg;
    let total = avg.dur_us;

    let categories: Vec<(&str, f64, Color)> = vec![
        ("JS", avg.js_us, COLOR_JS),
        ("Style", avg.ult_us, COLOR_STYLE),
        ("Layout", avg.layout_us, COLOR_LAYOUT),
        ("Paint", avg.paint_us, COLOR_PAINT),
        ("Composite", avg.composite_us, COLOR_COMPOSITE),
        ("HitTest", avg.hit_test_us, COLOR_HITTEST),
    ];

    let bar_width = area.width.saturating_sub(4) as usize;
    let bar_spans = stacked_bar_spans(&categories, total, bar_width);

    let mut lines = vec![
        Line::from(vec![Span::styled(
            "  Average Task Breakdown:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(
            std::iter::once(Span::raw("  "))
                .chain(bar_spans.into_iter())
                .collect::<Vec<_>>(),
        ),
        Line::from(""),
    ];

    // Legend with values - two rows
    let mut legend_row1 = vec![Span::raw("  ")];
    let mut legend_row2 = vec![Span::raw("  ")];

    for (i, (label, value, color)) in categories.iter().enumerate() {
        let pct = if total > 0.0 {
            value / total * 100.0
        } else {
            0.0
        };
        if *value > 0.0 {
            let entry = format!(" {} {} {:.0}% ", "█", label, pct);
            if i < 3 {
                legend_row1.push(Span::styled(entry, Style::default().fg(*color)));
            } else {
                legend_row2.push(Span::styled(entry, Style::default().fg(*color)));
            }
        }
    }

    lines.push(Line::from(legend_row1));
    lines.push(Line::from(legend_row2));

    // Per-category time values
    let mut detail_line = vec![Span::raw("  ")];
    for (label, value, color) in &categories {
        if *value > 0.0 {
            detail_line.push(Span::styled(
                format!("{}={} ", label, fmt_time(*value, t)),
                Style::default().fg(*color),
            ));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(detail_line));

    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Time Breakdown ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(panel, area);
}

fn draw_scroll_tasks(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let sf = &app.scroll_frames;

    let bar_width = 20;

    let rows: Vec<Row> = sf
        .tasks
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let (sev, sev_color) = severity_label(task.dur_us);
            let bottleneck = task.bottleneck();
            let frames = frame_budget_ratio(task.dur_us, t);

            // Colored mini bar
            let parts = [
                (task.js_us, COLOR_JS),
                (task.ult_us, COLOR_STYLE),
                (task.layout_us, COLOR_LAYOUT),
                (task.paint_us, COLOR_PAINT),
                (task.composite_us, COLOR_COMPOSITE),
                (task.hit_test_us, COLOR_HITTEST),
            ];
            let bar_spans = colored_mini_bar(&parts, task.dur_us, bar_width);

            // Build the breakdown cell with colored spans
            let breakdown_line = Line::from(bar_spans);

            Row::new(vec![
                Cell::from(format!("#{}", i + 1))
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(Span::styled(
                    format!(" {} ", sev),
                    Style::default()
                        .fg(if sev == "OK" {
                            Color::Black
                        } else {
                            Color::White
                        })
                        .bg(sev_color)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(fmt_time(task.dur_us, t))
                    .style(Style::default().fg(sev_color).add_modifier(Modifier::BOLD)),
                Cell::from(format!("{:.1}f", frames))
                    .style(Style::default().fg(if frames > 3.0 {
                        Color::Red
                    } else if frames > 1.0 {
                        Color::Yellow
                    } else {
                        Color::Green
                    })),
                Cell::from(bottleneck.to_string())
                    .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Cell::from(fmt_time(task.js_us, t)).style(Style::default().fg(COLOR_JS)),
                Cell::from(fmt_time(task.ult_us, t))
                    .style(Style::default().fg(COLOR_STYLE)),
                Cell::from(fmt_time(task.layout_us, t))
                    .style(Style::default().fg(COLOR_LAYOUT)),
                Cell::from(breakdown_line),
            ])
            .style(row_bg(i))
        })
        .collect();

    let visible = visible_rows(&rows, app.scroll_offset, area.height as usize);

    let table = Table::new(
        visible,
        [
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Percentage(9),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Percentage(9),
            Constraint::Percentage(9),
            Constraint::Percentage(9),
            Constraint::Min(20),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Individual Scroll Tasks ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    )
    .header(
        Row::new(vec![
            "#",
            "Status",
            "Duration",
            "Frames",
            "Bottleneck",
            "JS",
            "Style",
            "Layout",
            "Breakdown",
        ])
        .style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )
        .bottom_margin(1),
    );

    frame.render_widget(table, area);
}

// ── CPU Profile Tab ──

fn draw_cpu_profile(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(8),
        Constraint::Min(0),
    ])
    .split(area);

    draw_cpu_overview(frame, app, chunks[0]);
    draw_cpu_table(frame, app, chunks[1]);
}

fn draw_cpu_overview(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let cp = &app.cpu_profile;
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

    let bar_width = area.width.saturating_sub(6) as usize;
    let bar_parts = vec![
        ("App", cp.app_time_us, Color::Cyan),
        ("Runtime", cp.runtime_time_us, Color::Yellow),
        ("Native", cp.native_time_us, Color::DarkGray),
    ];
    let bar_spans = stacked_bar_spans(&bar_parts, total, bar_width);

    let lines = vec![
        Line::from(vec![
            Span::styled("  Sample Time: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                fmt_time(total, t),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled(
                format!("{} functions", cp.functions.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("█ App: {} ({:.1}%)", fmt_time(cp.app_time_us, t), app_pct),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("  "),
            Span::styled(
                format!(
                    "█ Runtime: {} ({:.1}%)",
                    fmt_time(cp.runtime_time_us, t),
                    rt_pct
                ),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("  "),
            Span::styled(
                format!(
                    "█ Native: {} ({:.1}%)",
                    fmt_time(cp.native_time_us, t),
                    native_pct
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(
            std::iter::once(Span::raw("  "))
                .chain(bar_spans.into_iter())
                .collect::<Vec<_>>(),
        ),
    ];

    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" CPU Time Distribution ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(panel, area);
}

fn draw_cpu_table(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let cp = &app.cpu_profile;

    let total_time = cp.total_sample_time_us;
    let max_time = cp
        .functions
        .first()
        .map(|f| f.self_time_us)
        .unwrap_or(1.0);
    let bar_width = 18;

    let mut cumulative_pct = 0.0;

    let rows: Vec<Row> = cp
        .functions
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let pct = if total_time > 0.0 {
                f.self_time_us / total_time * 100.0
            } else {
                0.0
            };
            cumulative_pct += pct;

            let name = if f.function_name.is_empty() {
                "(anonymous)"
            } else {
                &f.function_name
            };

            let short_url = shorten_url(&f.url);

            let source_color = match f.source_type {
                SourceType::AppCode => Color::Cyan,
                SourceType::Runtime => Color::Yellow,
                SourceType::Native => Color::DarkGray,
            };

            let pct_color = if pct > 5.0 {
                Color::Red
            } else if pct > 1.0 {
                Color::Yellow
            } else {
                Color::White
            };

            let bar_str = bar(f.self_time_us, max_time, bar_width);

            Row::new(vec![
                Cell::from(format!("#{}", i + 1))
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(f.source_type.label())
                    .style(Style::default().fg(source_color)),
                Cell::from(name.to_string()),
                Cell::from(fmt_time(f.self_time_us, t)),
                Cell::from(format!("{:.1}%", pct))
                    .style(Style::default().fg(pct_color)),
                Cell::from(format!("{:.1}%", cumulative_pct))
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(bar_str).style(Style::default().fg(source_color)),
                Cell::from(short_url).style(Style::default().fg(Color::DarkGray)),
            ])
            .style(row_bg(i))
        })
        .collect();

    let visible = visible_rows(&rows, app.scroll_offset, area.height as usize);

    let table = Table::new(
        visible,
        [
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Percentage(20),
            Constraint::Percentage(10),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Min(18),
            Constraint::Percentage(20),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Functions by Self-Time ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    )
    .header(
        Row::new(vec![
            "#", "Source", "Function", "Self Time", "%", "Cum%", "", "File",
        ])
        .style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )
        .bottom_margin(1),
    );

    frame.render_widget(table, area);
}

// ── Layout Dirty Tab ──

fn draw_layout_dirty(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(8),
        Constraint::Min(0),
    ])
    .split(area);

    draw_layout_overview(frame, app, chunks[0]);
    draw_layout_table(frame, app, chunks[1]);
}

fn draw_layout_overview(frame: &mut Frame, app: &App, area: Rect) {
    let ld = &app.layout_dirty;

    let cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    // Left: metrics
    let ratio_gauge = text_gauge(ld.avg_ratio / 100.0, 15);

    let (verdict, verdict_color) = if ld.avg_dirty > 500.0 {
        (
            "!! High - large DOM mutations per layout",
            Color::Red,
        )
    } else if ld.avg_dirty > 100.0 {
        (
            "! Moderate - consider batching DOM updates",
            Color::Yellow,
        )
    } else {
        ("Dirty count is reasonable", Color::Green)
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("  Events:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", ld.entries.len()),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Avg dirty:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.0}", ld.avg_dirty),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Max: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", ld.max_dirty),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(
            [vec![Span::styled(
                "  Avg ratio:  ",
                Style::default().fg(Color::DarkGray),
            )]]
            .into_iter()
            .chain(std::iter::once(ratio_gauge))
            .flatten()
            .collect::<Vec<_>>(),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(verdict, Style::default().fg(verdict_color)),
        ]),
    ];

    let left = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Layout Stats ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(left, cols[0]);

    // Right: dirty count distribution (top 5 as horizontal bars)
    let top5: Vec<&crate::analysis::LayoutDirtyEntry> =
        ld.entries.iter().take(5).collect();
    let max_d = ld.max_dirty.max(1) as f64;
    let inner_w = cols[1].width.saturating_sub(16) as usize;

    let dist_lines: Vec<Line> = top5
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let ratio = e.dirty_count as f64 / e.total_count.max(1) as f64 * 100.0;
            let bar_len =
                (e.dirty_count as f64 / max_d * inner_w as f64).round() as usize;
            let bar_len = bar_len.max(1).min(inner_w);
            let color = if ratio > 50.0 {
                Color::Red
            } else if ratio > 20.0 {
                Color::Yellow
            } else {
                Color::Green
            };
            Line::from(vec![
                Span::styled(format!(" #{} ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:>5} ", e.dirty_count),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled("█".repeat(bar_len), Style::default().fg(color)),
                Span::styled(
                    format!(" {:.0}%", ratio),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        })
        .collect();

    let right = Paragraph::new(dist_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Top Dirty Counts ")
            .title_style(Style::default().fg(Color::Red)),
    );
    frame.render_widget(right, cols[1]);
}

fn draw_layout_table(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.throttle_20x;
    let ld = &app.layout_dirty;

    let max_dirty = ld.max_dirty.max(1) as f64;
    let bar_width = 25;

    let rows: Vec<Row> = ld
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let ratio = if e.total_count > 0 {
                e.dirty_count as f64 / e.total_count as f64 * 100.0
            } else {
                0.0
            };

            let ratio_color = if ratio > 50.0 {
                Color::Red
            } else if ratio > 20.0 {
                Color::Yellow
            } else {
                Color::Green
            };

            let bar_str = bar(e.dirty_count as f64, max_dirty, bar_width);

            Row::new(vec![
                Cell::from(format!("#{}", i + 1))
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(fmt_time(e.dur_us, t)),
                Cell::from(format!("{}", e.dirty_count))
                    .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Cell::from(format!("{}", e.total_count))
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(format!("{:.1}%", ratio))
                    .style(Style::default().fg(ratio_color).add_modifier(Modifier::BOLD)),
                Cell::from(bar_str).style(Style::default().fg(ratio_color)),
            ])
            .style(row_bg(i))
        })
        .collect();

    let visible = visible_rows(&rows, app.scroll_offset, area.height as usize);

    let table = Table::new(
        visible,
        [
            Constraint::Length(5),
            Constraint::Percentage(12),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Min(25),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" All Layout Events (by dirty count) ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    )
    .header(
        Row::new(vec!["#", "Duration", "Dirty", "Total", "Ratio", "Distribution"])
            .style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Cyan),
            )
            .bottom_margin(1),
    );

    frame.render_widget(table, area);
}

// ── Compare Tab ──

fn draw_compare(frame: &mut Frame, app: &App, area: Rect) {
    let cmp = match &app.compare {
        Some(c) => c,
        None => {
            let msg = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No comparison file provided. Use: chperf trace.json --compare other.json",
                    Style::default().fg(Color::DarkGray),
                )]),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Compare "),
            );
            frame.render_widget(msg, area);
            return;
        }
    };

    let metrics_height = if cmp.style_recalc_a.total_count > 0 || cmp.style_recalc_b.total_count > 0 {
        10
    } else {
        8
    };
    let chunks = Layout::vertical([
        Constraint::Length(4),             // header + findings
        Constraint::Length(9),             // scroll frame side-by-side bars
        Constraint::Length(metrics_height), // key metrics comparison
        Constraint::Min(0),               // bottom section (event table + cpu diff)
    ])
    .split(area);

    draw_compare_header(frame, app, cmp, chunks[0]);
    draw_compare_scroll_bars(frame, app, cmp, chunks[1]);
    draw_compare_metrics(frame, app, cmp, chunks[2]);
    draw_compare_bottom(frame, app, cmp, chunks[3]);
}

fn draw_compare_header(
    frame: &mut Frame,
    app: &App,
    cmp: &crate::analysis::CompareResult,
    area: Rect,
) {
    let name_a = &app.trace_name_a;
    let name_b = app.trace_name_b.as_deref().unwrap_or("B");

    // Findings summary on one line
    let improved = cmp
        .findings
        .iter()
        .filter(|f| f.severity == crate::analysis::FindingSeverity::Improved)
        .count();
    let regressed = cmp
        .findings
        .iter()
        .filter(|f| f.severity == crate::analysis::FindingSeverity::Regressed)
        .count();

    let lines = vec![
        Line::from(vec![
            Span::styled("  A: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                name_a.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  vs  "),
            Span::styled("B: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                name_b.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("      "),
            Span::styled(
                format!("{} improved", improved),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} regressed", regressed),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Compare ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(panel, area);
}

fn draw_compare_scroll_bars(
    frame: &mut Frame,
    app: &App,
    cmp: &crate::analysis::CompareResult,
    area: Rect,
) {
    let t = app.throttle_20x;
    let cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    let name_a = &app.trace_name_a;
    let name_b = app.trace_name_b.as_deref().unwrap_or("B");

    // Side-by-side stacked bars
    for (idx, (avg_opt, name, col_area, label_color)) in [
        (&cmp.scroll_avg_a, name_a.as_str(), cols[0], Color::Cyan),
        (&cmp.scroll_avg_b, name_b, cols[1], Color::Yellow),
    ]
    .into_iter()
    .enumerate()
    {
        let count = if idx == 0 {
            cmp.scroll_count_a
        } else {
            cmp.scroll_count_b
        };

        if let Some(avg) = avg_opt {
            let bar_w = col_area.width.saturating_sub(4) as usize;
            let categories: Vec<(&str, f64, Color)> = vec![
                ("JS", avg.js_us, COLOR_JS),
                ("Style", avg.ult_us, COLOR_STYLE),
                ("Layout", avg.layout_us, COLOR_LAYOUT),
                ("Paint", avg.paint_us, COLOR_PAINT),
                ("Comp", avg.composite_us, COLOR_COMPOSITE),
                ("Hit", avg.hit_test_us, COLOR_HITTEST),
            ];
            let bar_spans = stacked_bar_spans(&categories, avg.dur_us, bar_w);

            let (sev_label, sev_color) = severity_label(avg.dur_us);

            // Legend
            let legend: Vec<Span> = categories
                .iter()
                .filter(|(_, v, _)| *v > 0.0)
                .map(|(label, v, color)| {
                    let pct = if avg.dur_us > 0.0 {
                        v / avg.dur_us * 100.0
                    } else {
                        0.0
                    };
                    Span::styled(format!("{}:{:.0}% ", label, pct), Style::default().fg(*color))
                })
                .collect();

            let lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!("  {} tasks  ", count),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("Avg: {}", fmt_time(avg.dur_us, t)),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("[{}]", sev_label),
                        Style::default()
                            .fg(sev_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  Bottleneck: {}", avg.bottleneck()),
                        Style::default().fg(Color::Red),
                    ),
                ]),
                Line::from(""),
                Line::from(
                    std::iter::once(Span::raw("  "))
                        .chain(bar_spans.into_iter())
                        .collect::<Vec<_>>(),
                ),
                Line::from(""),
                Line::from(
                    std::iter::once(Span::raw("  "))
                        .chain(legend.into_iter())
                        .collect::<Vec<_>>(),
                ),
            ];

            let panel = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", name))
                    .title_style(
                        Style::default()
                            .fg(label_color)
                            .add_modifier(Modifier::BOLD),
                    ),
            );
            frame.render_widget(panel, col_area);
        } else {
            let panel = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No scroll tasks detected",
                    Style::default().fg(Color::DarkGray),
                )]),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", name))
                    .title_style(
                        Style::default()
                            .fg(label_color)
                            .add_modifier(Modifier::BOLD),
                    ),
            );
            frame.render_widget(panel, col_area);
        }
    }
}

fn draw_compare_metrics(
    frame: &mut Frame,
    app: &App,
    cmp: &crate::analysis::CompareResult,
    area: Rect,
) {
    let t = app.throttle_20x;

    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: Key Findings
    let mut finding_lines: Vec<Line> = Vec::new();
    if cmp.findings.is_empty() {
        finding_lines.push(Line::from(vec![Span::styled(
            "  No significant differences found",
            Style::default().fg(Color::DarkGray),
        )]));
    } else {
        for f in cmp.findings.iter().take(5) {
            let (icon, color) = match f.severity {
                crate::analysis::FindingSeverity::Improved => ("  +", Color::Green),
                crate::analysis::FindingSeverity::Regressed => ("  !", Color::Red),
                crate::analysis::FindingSeverity::Neutral => ("  =", Color::White),
            };
            let mut spans = vec![
                Span::styled(icon, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(" {}: ", f.category),
                    Style::default().fg(Color::White),
                ),
                Span::styled(f.message.clone(), Style::default().fg(color)),
            ];
            if !f.detail.is_empty() {
                spans.push(Span::styled(
                    format!("  ({})", f.detail),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            finding_lines.push(Line::from(spans));
        }
    }

    let findings_panel = Paragraph::new(finding_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Key Findings ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(findings_panel, cols[0]);

    // Right: Quick Stats comparison
    let sa = &cmp.summary_a;
    let sb = &cmp.summary_b;

    let mut metrics = vec![
        (
            "Long Tasks",
            sa.long_task_count as f64,
            sb.long_task_count as f64,
            false,
        ),
        (
            "Worst Task",
            sa.long_tasks_top.first().copied().unwrap_or(0.0),
            sb.long_tasks_top.first().copied().unwrap_or(0.0),
            true,
        ),
        (
            "Main Busy",
            sa.main_thread_busy_us,
            sb.main_thread_busy_us,
            true,
        ),
        (
            "Layout Dirty",
            cmp.layout_a.avg_dirty as f64,
            cmp.layout_b.avg_dirty as f64,
            false,
        ),
    ];
    // Add style elements if either trace has data
    if cmp.style_recalc_a.total_count > 0 || cmp.style_recalc_b.total_count > 0 {
        metrics.push((
            "Style Elem avg",
            cmp.style_recalc_a.avg_elements,
            cmp.style_recalc_b.avg_elements,
            false,
        ));
        metrics.push((
            "Style Elem max",
            cmp.style_recalc_a.max_elements as f64,
            cmp.style_recalc_b.max_elements as f64,
            false,
        ));
    }

    let stat_lines: Vec<Line> = metrics
        .iter()
        .map(|(name, a, b, is_time)| {
            let diff = if *a > 0.0 {
                (b - a) / a * 100.0
            } else {
                0.0
            };
            let (ds, dc) = format_diff(diff);
            let va = if *is_time {
                fmt_time(*a, t)
            } else {
                format!("{:.0}", a)
            };
            let vb = if *is_time {
                fmt_time(*b, t)
            } else {
                format!("{:.0}", b)
            };
            Line::from(vec![
                Span::styled(format!("  {:<13}", name), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:>8}", va), Style::default().fg(Color::Cyan)),
                Span::styled(" -> ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<8}", vb), Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!(" {}", ds),
                    Style::default().fg(dc).add_modifier(Modifier::BOLD),
                ),
            ])
        })
        .collect();

    let stats_panel = Paragraph::new(stat_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Quick Stats ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(stats_panel, cols[1]);
}

fn draw_compare_bottom(
    frame: &mut Frame,
    app: &App,
    cmp: &crate::analysis::CompareResult,
    area: Rect,
) {
    let t = app.throttle_20x;

    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: Event Average Comparison
    let event_rows: Vec<Row> = cmp
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let (diff_str, diff_color) = format_diff(r.diff_pct);
            let bar_str = diff_bar(r.diff_pct, 8);

            Row::new(vec![
                Cell::from(r.event_name.clone()),
                Cell::from(fmt_time(r.avg_a_us, t)).style(Style::default().fg(Color::Cyan)),
                Cell::from(fmt_time(r.avg_b_us, t)).style(Style::default().fg(Color::Yellow)),
                Cell::from(diff_str).style(
                    Style::default()
                        .fg(diff_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(bar_str).style(Style::default().fg(diff_color)),
            ])
            .style(row_bg(i))
        })
        .collect();

    let visible_events = visible_rows(&event_rows, app.scroll_offset, area.height as usize);

    let event_table = Table::new(
        visible_events,
        [
            Constraint::Percentage(28),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(13),
            Constraint::Min(8),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Event Averages ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    )
    .header(
        Row::new(vec!["Event", "A", "B", "Change", ""])
            .style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Cyan),
            )
            .bottom_margin(1),
    );

    frame.render_widget(event_table, cols[0]);

    // Right: CPU Profile Diff (top functions)
    let cpu_rows: Vec<Row> = cmp
        .cpu_diff
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let name = if d.function_name.is_empty() {
                "(anonymous)"
            } else {
                &d.function_name
            };

            let source_color = match d.source_type {
                SourceType::AppCode => Color::Cyan,
                SourceType::Runtime => Color::Yellow,
                SourceType::Native => Color::DarkGray,
            };

            // Show percentage point change (pct_b - pct_a)
            let pp_change = d.pct_b - d.pct_a;
            let (pp_str, pp_color) = if pp_change > 0.5 {
                (format!("+{:.1}pp", pp_change), Color::Red)
            } else if pp_change < -0.5 {
                (format!("{:.1}pp", pp_change), Color::Green)
            } else {
                (format!("{:.1}pp", pp_change), Color::White)
            };

            Row::new(vec![
                Cell::from(d.source_type.label()).style(Style::default().fg(source_color)),
                Cell::from(name.to_string()),
                Cell::from(format!("{:.1}%", d.pct_a)).style(Style::default().fg(Color::Cyan)),
                Cell::from(format!("{:.1}%", d.pct_b)).style(Style::default().fg(Color::Yellow)),
                Cell::from(pp_str).style(Style::default().fg(pp_color).add_modifier(Modifier::BOLD)),
            ])
            .style(row_bg(i))
        })
        .collect();

    let visible_cpu = visible_rows(&cpu_rows, app.scroll_offset, area.height as usize);

    let cpu_table = Table::new(
        visible_cpu,
        [
            Constraint::Length(8),
            Constraint::Percentage(30),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Min(8),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" CPU Profile Diff (by impact) ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
    )
    .header(
        Row::new(vec!["Source", "Function", "A %", "B %", "Change"])
            .style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Cyan),
            )
            .bottom_margin(1),
    );

    frame.render_widget(cpu_table, cols[1]);
}

fn format_diff(pct: f64) -> (String, Color) {
    let color = if pct < -5.0 {
        Color::Green
    } else if pct > 5.0 {
        Color::Red
    } else {
        Color::White
    };
    let s = if pct > 0.0 {
        format!("+{:.1}%", pct)
    } else {
        format!("{:.1}%", pct)
    };
    (s, color)
}

fn diff_bar(pct: f64, half_width: usize) -> String {
    let clamped = pct.max(-100.0).min(100.0);
    let chars = (clamped.abs() / 100.0 * half_width as f64).round() as usize;
    let chars = chars.min(half_width);

    if clamped < 0.0 {
        let padding = half_width - chars;
        format!("{}{}|", " ".repeat(padding), "◄".repeat(chars),)
    } else {
        format!("|{}", "►".repeat(chars),)
    }
}

// ── Status Bar ──

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let row_count = app.row_count();
    let scroll_info = if row_count > 0 {
        format!(
            " {}/{}",
            (app.scroll_offset + 1).min(row_count),
            row_count
        )
    } else {
        String::new()
    };

    // Show status message if present
    if let Some(ref msg) = app.status_message {
        let line = Line::from(vec![
            Span::styled(
                format!(" {} ", msg),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  (press any key to dismiss)",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let mut spans = vec![
        Span::styled(" q", Style::default().fg(Color::Yellow)),
        Span::raw(":Quit "),
        Span::styled("Tab", Style::default().fg(Color::Yellow)),
        Span::raw(":Switch "),
        Span::styled("e", Style::default().fg(Color::Yellow)),
        Span::raw(":Export "),
        Span::styled("t", Style::default().fg(Color::Yellow)),
        Span::raw(":Throttle "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(":Scroll "),
        Span::styled("g/G", Style::default().fg(Color::Yellow)),
        Span::raw(":Top/Bot "),
        Span::styled("1-5", Style::default().fg(Color::Yellow)),
        Span::raw(":Tab"),
    ];

    if app.throttle_20x {
        spans.push(Span::styled(
            " [20x] ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    spans.push(Span::styled(
        scroll_info,
        Style::default().fg(Color::DarkGray),
    ));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ── Helpers ──

fn visible_rows<'a>(rows: &'a [Row<'a>], offset: usize, area_height: usize) -> Vec<Row<'a>> {
    let max_visible = area_height.saturating_sub(4);
    rows.iter().skip(offset).take(max_visible).cloned().collect()
}

fn shorten_url(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    if let Some(idx) = url.rfind('/') {
        url[idx + 1..].to_string()
    } else {
        url.to_string()
    }
}
