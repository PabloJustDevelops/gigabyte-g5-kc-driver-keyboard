// Width-driven layout decisions.
//
// The panel used to lay out as if it had a fixed width: a 220 px sidebar and
// a content column capped at 940 px, both pinned. On a monitor where the window
// was opened larger that left a large empty gutter to the right of the content
// and a shorter gutter below it, which is what made the panel read as flat and
// unfinished rather than as a dense tool.
//
// There is no CSS here, so the breakpoints are decided once per frame from the
// window size and passed down to whatever needs them. `container_query` exists
// in this version of gpui, but its child is laid out after the container's own
// size is fixed, so it can switch a layout but never grow one — and the thing
// that has to give here is the content column's own width.

use gpui_kit::{Pixels, Window, px};

/// Which of the three layouts the panel is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Breakpoint {
    /// Narrow windows: the sidebar is icons only and every view is one column.
    Compact,
    /// The default: full sidebar, one column.
    Medium,
    /// Wide windows: full sidebar, two lanes per view, content capped so it
    /// does not stretch to the far corners of a 4K panel.
    Wide,
}

impl Breakpoint {
    /// Below this the 220 px sidebar costs more than it gives, so it collapses
    /// to its icons. Measured against the window, not the content: the content
    /// is what adapts.
    pub const COMPACT_BELOW: f32 = 800.;
    /// Two lanes need roughly 600 px each to stay readable, which is what this
    /// buys once the sidebar has taken its share.
    pub const WIDE_ABOVE: f32 = 1180.;

    /// Classify a window width in logical pixels.
    pub fn of(window_width: Pixels) -> Self {
        let w: f32 = window_width.into();
        if w < Self::COMPACT_BELOW {
            Breakpoint::Compact
        } else if w < Self::WIDE_ABOVE {
            Breakpoint::Medium
        } else {
            Breakpoint::Wide
        }
    }

    /// The breakpoint for the window currently being drawn into.
    pub fn for_window(window: &Window) -> Self {
        Self::of(window.viewport_size().width)
    }

    /// Whether the sidebar rows carry their label and sub-label, as opposed to
    /// just the icon.
    pub fn sidebar_labels(self) -> bool {
        self != Breakpoint::Compact
    }

    pub fn sidebar_w(self) -> Pixels {
        if self.sidebar_labels() {
            px(220.)
        } else {
            px(64.)
        }
    }

    /// How many lanes a view is arranged in.
    pub fn lanes(self) -> usize {
        match self {
            Breakpoint::Wide => 2,
            _ => 1,
        }
    }

    /// The cap on the content column. Only the wide layout has one: below it
    /// the column is exactly the pane, and capping it there is what produced
    /// the dead gutter.
    pub fn content_max_w(self) -> Option<Pixels> {
        match self {
            Breakpoint::Wide => Some(px(1340.)),
            _ => None,
        }
    }

    /// Padding around the content column.
    pub fn gutter(self) -> Pixels {
        match self {
            Breakpoint::Compact => px(16.),
            Breakpoint::Medium => px(20.),
            Breakpoint::Wide => px(28.),
        }
    }

    /// The narrowest a gauge may be before the grid has to wrap. Fan duty and
    /// temperature readouts sit side by side with their sub-labels, and below
    /// this the sub-label is what gets pushed off.
    pub fn gauge_min_w(self) -> Pixels {
        match self {
            Breakpoint::Compact => px(112.),
            _ => px(148.),
        }
    }

    /// Gap between lanes and between cards.
    pub fn gap(self) -> Pixels {
        match self {
            Breakpoint::Compact => px(12.),
            _ => px(16.),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_windows_collapse_the_sidebar_and_the_lanes() {
        // The width the panel is expected to be usable at: half a large screen.
        assert_eq!(Breakpoint::of(px(700.)), Breakpoint::Compact);
        assert!(!Breakpoint::of(px(700.)).sidebar_labels());
        assert_eq!(Breakpoint::of(px(700.)).lanes(), 1);
    }

    #[test]
    fn the_shipped_size_is_the_default_layout() {
        assert_eq!(Breakpoint::of(px(1040.)), Breakpoint::Medium);
        assert!(Breakpoint::of(px(1040.)).sidebar_labels());
    }

    #[test]
    fn wide_windows_get_two_lanes_and_a_cap() {
        assert_eq!(Breakpoint::of(px(1880.)), Breakpoint::Wide);
        assert_eq!(Breakpoint::of(px(1880.)).lanes(), 2);
        // A 4K monitor would otherwise stretch the column past the eye's
        // comfortable line length.
        assert!(Breakpoint::of(px(3840.)).content_max_w().is_some());
        assert!(Breakpoint::of(px(1040.)).content_max_w().is_none());
    }

    #[test]
    fn the_breakpoints_do_not_leave_a_dead_band() {
        // Every width must classify; a gap between the thresholds would fall
        // through both comparisons and silently take the wide branch.
        for w in [320., 700., 799., 800., 1179., 1180., 1880., 3840.] {
            let bp = Breakpoint::of(px(w));
            assert!(matches!(
                bp,
                Breakpoint::Compact | Breakpoint::Medium | Breakpoint::Wide
            ));
        }
    }
}
