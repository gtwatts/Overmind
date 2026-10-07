//! Width-aware composition of a one-line status bar from prioritized segments.
//!
//! Each segment offers variants from richest to most compact. Fitting runs in two passes:
//!
//! 1. Shrink: while the line is too wide, the lowest-priority segment that still has a more
//!    compact variant steps down. Once every segment is at its most compact form, the
//!    lowest-priority segment is dropped. The highest-priority segment is never dropped
//!    (ratatui clips it when the terminal is narrower than its compact form).
//! 2. Grow: spare columns go back to segments in priority order, so the most important signal
//!    regains detail first.

use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub(crate) struct Segment {
    pub(crate) priority: u8,
    pub(crate) align: Align,
    variants: Vec<Vec<Span<'static>>>,
}

impl Segment {
    /// `variants` must be ordered richest first; empty variants are ignored.
    pub(crate) fn new(priority: u8, align: Align, variants: Vec<Vec<Span<'static>>>) -> Self {
        let variants = variants.into_iter().filter(|v| !v.is_empty()).collect();
        Self {
            priority,
            align,
            variants,
        }
    }
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(Span::width).sum()
}

const GROUP_GAP: usize = 2;

struct Layout<'a> {
    segments: &'a [Segment],
    choice: Vec<Option<usize>>,
    lead: usize,
    separator: usize,
}

impl Layout<'_> {
    fn width(&self) -> usize {
        let mut left = (0usize, 0usize);
        let mut right = (0usize, 0usize);
        for (segment, choice) in self.segments.iter().zip(&self.choice) {
            let Some(index) = choice else { continue };
            let width = spans_width(&segment.variants[*index]);
            let group = match segment.align {
                Align::Left => &mut left,
                Align::Right => &mut right,
            };
            group.0 += width;
            group.1 += 1;
        }
        let group_width =
            |(width, count): (usize, usize)| width + self.separator * count.saturating_sub(1);
        let gap = if left.1 > 0 && right.1 > 0 {
            GROUP_GAP
        } else {
            0
        };
        self.lead + group_width(left) + group_width(right) + gap
    }

    fn visible(&self) -> usize {
        self.choice.iter().filter(|choice| choice.is_some()).count()
    }
}

/// Fit `segments` into `width` columns. Returns `None` when nothing is visible.
pub(crate) fn fit_line(
    segments: &[Segment],
    width: u16,
    lead: &str,
    separator: &str,
    separator_style: Style,
) -> Option<Line<'static>> {
    let width = usize::from(width);
    let mut layout = Layout {
        segments,
        choice: segments
            .iter()
            .map(|segment| (!segment.variants.is_empty()).then_some(0))
            .collect(),
        lead: Span::raw(lead).width(),
        separator: Span::raw(separator).width(),
    };
    if layout.visible() == 0 || width == 0 {
        return None;
    }
    let mut ascending: Vec<usize> = (0..segments.len()).collect();
    ascending.sort_by_key(|&index| segments[index].priority);
    while layout.width() > width {
        let shrinkable = ascending.iter().copied().find(|&index| {
            layout.choice[index].is_some_and(|choice| choice + 1 < segments[index].variants.len())
        });
        if let Some(index) = shrinkable {
            layout.choice[index] = layout.choice[index].map(|choice| choice + 1);
            continue;
        }
        if layout.visible() <= 1 {
            break;
        }
        if let Some(index) = ascending
            .iter()
            .copied()
            .find(|&index| layout.choice[index].is_some())
        {
            layout.choice[index] = None;
        }
    }
    for &index in ascending.iter().rev() {
        while let Some(choice) = layout.choice[index].filter(|choice| *choice > 0) {
            layout.choice[index] = Some(choice - 1);
            if layout.width() > width {
                layout.choice[index] = Some(choice);
                break;
            }
        }
    }

    let used = layout.width();
    let mut left: Vec<Span<'static>> = Vec::new();
    let mut right: Vec<Span<'static>> = Vec::new();
    for (segment, choice) in segments.iter().zip(&layout.choice) {
        let Some(index) = choice else { continue };
        let group = match segment.align {
            Align::Left => &mut left,
            Align::Right => &mut right,
        };
        if !group.is_empty() {
            group.push(Span::styled(separator.to_string(), separator_style));
        }
        group.extend(segment.variants[*index].iter().cloned());
    }
    let mut spans = Vec::with_capacity(left.len() + right.len() + 2);
    if !lead.is_empty() {
        spans.push(Span::raw(lead.to_string()));
    }
    let has_left = !left.is_empty();
    spans.extend(left);
    if !right.is_empty() {
        let pad = width.saturating_sub(used) + if has_left { GROUP_GAP } else { 0 };
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        spans.extend(right);
    }
    Some(Line::from(spans))
}
