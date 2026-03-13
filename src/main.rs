mod analysis;
mod app;
mod export;
mod trace;
mod ui;

use std::io;
use std::path::Path;

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;

#[derive(Parser)]
#[command(name = "chperf", about = "Chrome DevTools Trace JSON analyzer (TUI)")]
struct Cli {
    /// Path to trace JSON file
    trace: String,

    /// Optional second trace file for comparison
    #[arg(short, long)]
    compare: Option<String>,

    /// Export analysis as Markdown (to stdout or file)
    /// Use --export to print to stdout, --export=FILE to write to file
    #[arg(short, long, num_args = 0..=1, default_missing_value = "-")]
    export: Option<String>,

    /// CPU throttle factor (e.g. --throttle 20 divides all times by 20)
    #[arg(short, long)]
    throttle: Option<f64>,

    /// Export only the comparison summary table (use with --export --compare)
    #[arg(short, long)]
    summary: bool,
}

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Parse trace A
    eprintln!("Loading {}...", cli.trace);
    let trace_a = trace::parse_trace(Path::new(&cli.trace))?;
    let main_tid_a = trace::detect_main_thread(&trace_a.trace_events);
    eprintln!(
        "  {} events, main thread tid={}",
        trace_a.trace_events.len(),
        main_tid_a
    );

    // Analyze A
    let summary_a = analysis::analyze_summary(&trace_a.trace_events, main_tid_a);
    let scroll_frames = analysis::analyze_scroll_frames(&trace_a.trace_events, main_tid_a);
    let cpu_profile = analysis::analyze_cpu_profile(&trace_a.trace_events);
    let layout_dirty = analysis::analyze_layout_dirty(&trace_a.trace_events, main_tid_a);
    let style_recalc = analysis::analyze_style_recalc(&trace_a.trace_events, main_tid_a);
    let forced_reflows = analysis::analyze_forced_reflows(&trace_a.trace_events, main_tid_a);

    // Optional: parse and compare trace B
    let (compare_result, trace_name_b) = if let Some(ref compare_path) = cli.compare {
        eprintln!("Loading {}...", compare_path);
        let trace_b = trace::parse_trace(Path::new(compare_path))?;
        let main_tid_b = trace::detect_main_thread(&trace_b.trace_events);
        eprintln!(
            "  {} events, main thread tid={}",
            trace_b.trace_events.len(),
            main_tid_b
        );

        let summary_b = analysis::analyze_summary(&trace_b.trace_events, main_tid_b);
        let scroll_frames_b =
            analysis::analyze_scroll_frames(&trace_b.trace_events, main_tid_b);
        let cpu_profile_b = analysis::analyze_cpu_profile(&trace_b.trace_events);
        let layout_dirty_b =
            analysis::analyze_layout_dirty(&trace_b.trace_events, main_tid_b);
        let style_recalc_b =
            analysis::analyze_style_recalc(&trace_b.trace_events, main_tid_b);
        let cmp = analysis::analyze_compare(
            &summary_a,
            &summary_b,
            &scroll_frames,
            &scroll_frames_b,
            &cpu_profile,
            &cpu_profile_b,
            &layout_dirty,
            &layout_dirty_b,
            &style_recalc,
            &style_recalc_b,
        );
        (Some(cmp), Some(file_stem(compare_path)))
    } else {
        (None, None)
    };

    let mut app = app::App::new(
        summary_a,
        scroll_frames,
        cpu_profile,
        layout_dirty,
        style_recalc,
        forced_reflows,
        compare_result,
        file_stem(&cli.trace),
        trace_name_b,
        trace_a.metadata.clone(),
    );

    // Apply throttle: CLI flag takes priority, otherwise auto-detect from trace metadata
    let throttle = cli.throttle.unwrap_or_else(|| {
        trace_a
            .metadata
            .as_ref()
            .and_then(|m| m.cpu_throttling)
            .unwrap_or(1.0)
    });
    if throttle > 1.0 {
        app.throttle_factor = throttle;
        app.throttle_factor_saved = throttle;
        eprintln!("  CPU throttle: {:.0}x ({})",
            throttle,
            if cli.throttle.is_some() { "from --throttle" } else { "auto-detected from trace" }
        );
    }

    // Export mode: skip TUI, output Markdown
    if let Some(ref export_target) = cli.export {
        let md = if cli.summary {
            export::export_summary_only(&app)
        } else {
            export::export_markdown(&app)
        };
        if export_target == "-" {
            print!("{}", md);
        } else {
            std::fs::write(export_target, &md)?;
            eprintln!("Exported to {}", export_target);
        }
        return Ok(());
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Event loop
    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        if let Event::Key(key) = event::read()? {
            // Dismiss status message on any keypress
            if app.status_message.is_some() {
                app.status_message = None;
                continue;
            }

            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    app.should_quit = true;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.should_quit = true;
                }
                KeyCode::Char('e') => {
                    // Export to file from TUI
                    let filename = format!("chperf-export-{}.md", app.trace_name_a);
                    let md = export::export_markdown(&app);
                    if let Err(e) = std::fs::write(&filename, &md) {
                        app.set_message(format!("Export failed: {}", e));
                    } else {
                        app.set_message(format!("Exported to {}", filename));
                    }
                }
                KeyCode::Tab => app.next_tab(),
                KeyCode::BackTab => app.prev_tab(),
                KeyCode::Char('t') => app.toggle_throttle(),
                KeyCode::Char('j') | KeyCode::Down => app.scroll_down(1),
                KeyCode::Char('k') | KeyCode::Up => app.scroll_up(1),
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.scroll_down(20)
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.scroll_up(20)
                }
                KeyCode::Char('g') => app.scroll_offset = 0,
                KeyCode::Char('G') => {
                    let max = app.row_count().saturating_sub(1);
                    app.scroll_offset = max;
                }
                KeyCode::Char('1') => {
                    if !app.tabs.is_empty() {
                        app.tab = app.tabs[0];
                        app.scroll_offset = 0;
                    }
                }
                KeyCode::Char('2') => {
                    if app.tabs.len() > 1 {
                        app.tab = app.tabs[1];
                        app.scroll_offset = 0;
                    }
                }
                KeyCode::Char('3') => {
                    if app.tabs.len() > 2 {
                        app.tab = app.tabs[2];
                        app.scroll_offset = 0;
                    }
                }
                KeyCode::Char('4') => {
                    if app.tabs.len() > 3 {
                        app.tab = app.tabs[3];
                        app.scroll_offset = 0;
                    }
                }
                KeyCode::Char('5') => {
                    if app.tabs.len() > 4 {
                        app.tab = app.tabs[4];
                        app.scroll_offset = 0;
                    }
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
