use crate::analysis::*;
use crate::trace::TraceMetadata;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Summary,
    ScrollFrames,
    CpuProfile,
    LayoutDirty,
    Compare,
}

impl Tab {
    pub fn all(has_compare: bool) -> Vec<Tab> {
        let mut tabs = vec![
            Tab::Summary,
            Tab::ScrollFrames,
            Tab::CpuProfile,
            Tab::LayoutDirty,
        ];
        if has_compare {
            tabs.push(Tab::Compare);
        }
        tabs
    }

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Summary => "Summary",
            Tab::ScrollFrames => "Scroll Frames",
            Tab::CpuProfile => "CPU Profile",
            Tab::LayoutDirty => "Layout Dirty",
            Tab::Compare => "Compare",
        }
    }
}

pub struct App {
    pub tab: Tab,
    pub tabs: Vec<Tab>,
    pub throttle_factor: f64,      // 1.0 = no throttle, Nx = divide times by N
    pub throttle_factor_saved: f64, // saved value for toggle
    pub scroll_offset: usize,
    pub should_quit: bool,
    pub summary: SummaryResult,
    pub scroll_frames: ScrollFrameResult,
    pub cpu_profile: CpuProfileResult,
    pub layout_dirty: LayoutDirtyResult,
    pub style_recalc: StyleRecalcResult,
    pub forced_reflows: ForcedReflowResult,
    pub compare: Option<CompareResult>,
    pub trace_name_a: String,
    pub trace_name_b: Option<String>,
    pub metadata: Option<TraceMetadata>,
    pub status_message: Option<String>,
}

impl App {
    pub fn new(
        summary: SummaryResult,
        scroll_frames: ScrollFrameResult,
        cpu_profile: CpuProfileResult,
        layout_dirty: LayoutDirtyResult,
        style_recalc: StyleRecalcResult,
        forced_reflows: ForcedReflowResult,
        compare: Option<CompareResult>,
        trace_name_a: String,
        trace_name_b: Option<String>,
        metadata: Option<TraceMetadata>,
    ) -> Self {
        let tabs = Tab::all(compare.is_some());
        App {
            tab: Tab::Summary,
            tabs,
            throttle_factor: 1.0,
            throttle_factor_saved: 20.0,
            scroll_offset: 0,
            should_quit: false,
            summary,
            scroll_frames,
            cpu_profile,
            layout_dirty,
            style_recalc,
            forced_reflows,
            compare,
            trace_name_a,
            trace_name_b,
            metadata,
            status_message: None,
        }
    }

    pub fn next_tab(&mut self) {
        let idx = self.tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
        self.tab = self.tabs[(idx + 1) % self.tabs.len()];
        self.scroll_offset = 0;
    }

    pub fn prev_tab(&mut self) {
        let idx = self.tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
        self.tab = if idx == 0 {
            self.tabs[self.tabs.len() - 1]
        } else {
            self.tabs[idx - 1]
        };
        self.scroll_offset = 0;
    }

    pub fn toggle_throttle(&mut self) {
        if self.throttle_factor > 1.0 {
            self.throttle_factor_saved = self.throttle_factor;
            self.throttle_factor = 1.0;
        } else {
            self.throttle_factor = self.throttle_factor_saved;
        }
    }

    pub fn is_throttled(&self) -> bool {
        self.throttle_factor > 1.0
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn set_message(&mut self, msg: String) {
        self.status_message = Some(msg);
    }

    pub fn row_count(&self) -> usize {
        match self.tab {
            Tab::Summary => self.summary.event_stats.len() + self.summary.long_tasks_top.len() + 3,
            Tab::ScrollFrames => self.scroll_frames.tasks.len() + 2,
            Tab::CpuProfile => self.cpu_profile.functions.len(),
            Tab::LayoutDirty => self.layout_dirty.entries.len(),
            Tab::Compare => self.compare.as_ref().map(|c| c.rows.len()).unwrap_or(0),
        }
    }
}
