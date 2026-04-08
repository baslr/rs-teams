//! LayoutTracker — a custom iced widget that wraps a Column and records
//! the Y-position of each child after layout.
//!
//! This enables sticky-header functionality: after `layout()` runs,
//! `child_y_positions` contains the exact pixel offset of every child
//! row relative to the content top. Combined with the scrollable's
//! `absolute_offset`, we can determine which row is at the viewport top.

use std::cell::RefCell;
use std::rc::Rc;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::event::Event;
use iced::mouse;
use iced::{Element, Length, Rectangle, Size, Vector};

/// Shared storage for child Y positions.
/// Written by the LayoutTracker widget during `layout()`,
/// read by the app during `update()` / `view()`.
pub type ChildPositions = Rc<RefCell<Vec<f32>>>;

/// Create a new shared ChildPositions store.
pub fn child_positions() -> ChildPositions {
    Rc::new(RefCell::new(Vec::new()))
}

/// A wrapper widget that delegates everything to its inner `content`
/// but records each child's Y-offset during `layout()`.
pub struct LayoutTracker<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    positions: ChildPositions,
}

impl<'a, Message, Theme, Renderer> LayoutTracker<'a, Message, Theme, Renderer> {
    pub fn new(content: Element<'a, Message, Theme, Renderer>, positions: ChildPositions) -> Self {
        Self { content, positions }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for LayoutTracker<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Delegate layout to the inner content (e.g. a Column)
        let node = self.content.as_widget_mut().layout(tree, renderer, limits);

        // After layout: read each child's Y position.
        // These are relative to the content's top (which is what we need
        // because scrollable::absolute_offset is also relative to content top).
        let mut positions = self.positions.borrow_mut();
        positions.clear();
        for child in node.children() {
            positions.push(child.bounds().y);
        }

        node
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

/// Convert a LayoutTracker into an Element.
impl<'a, Message, Theme, Renderer> From<LayoutTracker<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(tracker: LayoutTracker<'a, Message, Theme, Renderer>) -> Self {
        Element::new(tracker)
    }
}

// ---------------------------------------------------------------------------
// Pure function for sticky header lookup — testable without widgets
// ---------------------------------------------------------------------------

/// Given child Y positions and a scroll offset (pixels from top of content),
/// find the index of the topmost visible child.
///
/// Returns the index of the last child whose Y position is <= scroll_offset.
/// This is the child that is at or just above the top of the viewport.
pub fn top_visible_child(child_y_positions: &[f32], scroll_offset: f32) -> Option<usize> {
    if child_y_positions.is_empty() {
        return None;
    }

    // Binary search: find the last position <= scroll_offset
    let mut lo = 0;
    let mut hi = child_y_positions.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if child_y_positions[mid] <= scroll_offset {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }

    // lo is now the first index ABOVE scroll_offset.
    // lo - 1 is the last index AT or BELOW scroll_offset (the topmost visible child).
    if lo > 0 { Some(lo - 1) } else { Some(0) }
}

/// A single sticky header entry: sender index + viewport Y position.
#[derive(Debug, Clone, PartialEq)]
pub struct StickyEntry {
    /// Index into row_senders of this sticky sender.
    pub sender_index: usize,
    /// Y offset of the sticky label in the viewport (pixels from viewport top).
    /// 0.0 = pinned at viewport top.
    pub y_offset: f32,
}

/// Result of sticky header calculation — one primary + multiple outgoing stickies.
///
/// - `primary`: the sender pinned at (or near) the viewport top
/// - `outgoing`: all subsequent sender-group headers that are visible in the
///   viewport, each positioned at the first message row of their group
///   (one row below the header row). These scroll with the content.
#[derive(Debug, Clone, PartialEq)]
pub struct StickyState {
    /// The current sticky pinned at (or near) the viewport top.
    pub primary: StickyEntry,
    /// Sender stickies visible in the viewport below the primary.
    /// Each is positioned at the first message of its group (row after the
    /// sender-header row), scrolling naturally with the content.
    pub outgoing: Vec<StickyEntry>,
}

/// Calculate the sticky header state with multi-sticky support.
///
/// Returns a primary sticky (pinned at viewport top) plus all subsequent
/// sender-group headers that are currently visible in the viewport. Each
/// outgoing sticky is positioned at the first message row of its group
/// (one row below the header row), so it sits right where the sender's
/// messages begin.
///
/// `child_y_positions`: Y offset of each child row (from LayoutTracker)
/// `row_senders`: (sender_name, is_from_me) per row; empty name = separator
/// `scroll_offset`: pixels scrolled from top of content (absolute_offset_reversed)
/// `sticky_height`: height of the sticky label in pixels (unused currently, reserved)
/// `viewport_height`: height of the visible viewport in pixels
///
/// Returns None if no sticky should be shown (at very top, or no data).
pub fn compute_sticky_state(
    child_y_positions: &[f32],
    row_senders: &[(String, bool)],
    scroll_offset: f32,
    _sticky_height: f32,
    viewport_height: f32,
) -> Option<StickyState> {
    if child_y_positions.is_empty() || row_senders.is_empty() {
        return None;
    }

    // Find the topmost visible child
    let top_idx = top_visible_child(child_y_positions, scroll_offset)?;

    // Don't show sticky at the very top
    if top_idx == 0 && scroll_offset < 5.0 {
        return None;
    }

    // Walk backwards from top_idx to find the current sticky sender
    let limit = top_idx.min(row_senders.len().saturating_sub(1));
    let mut sender_index = None;
    for i in (0..=limit).rev() {
        if !row_senders[i].0.is_empty() {
            sender_index = Some(i);
            break;
        }
    }
    let sender_index = sender_index?;

    // Primary sticky: the current sender, pinned at top
    let group_start_y = child_y_positions[sender_index];
    let natural_y = group_start_y - scroll_offset;
    let primary_y = natural_y.max(0.0); // stick to top

    // Collect ALL subsequent sender-group headers visible in the viewport.
    // Each outgoing sticky is positioned at the FIRST MESSAGE of its group
    // (one row below the header), not at the header itself.
    let mut outgoing = Vec::new();
    let pos_len = child_y_positions.len();
    let current_sender = &row_senders[sender_index].0;
    let mut last_sender = current_sender.clone();
    for i in (sender_index + 1)..row_senders.len().min(pos_len) {
        let (ref name, _) = row_senders[i];
        if !name.is_empty() && name != &last_sender {
            // Row i is the first message of a new sender group.
            // Position the outgoing sticky at this row's Y position.
            let viewport_y = child_y_positions[i] - scroll_offset;

            if viewport_y >= viewport_height {
                break; // off screen below — no more visible
            }
            if viewport_y > primary_y {
                outgoing.push(StickyEntry {
                    sender_index: i,
                    y_offset: viewport_y,
                });
            }
            last_sender = name.clone();
        }
    }

    Some(StickyState {
        primary: StickyEntry {
            sender_index,
            y_offset: primary_y,
        },
        outgoing,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_visible_empty() {
        assert_eq!(top_visible_child(&[], 100.0), None);
    }

    #[test]
    fn top_visible_at_zero() {
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0];
        assert_eq!(top_visible_child(&positions, 0.0), Some(0));
    }

    #[test]
    fn top_visible_between_children() {
        // Scrolled to 60px — child at 50px is the topmost visible
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0];
        assert_eq!(top_visible_child(&positions, 60.0), Some(2));
    }

    #[test]
    fn top_visible_exactly_on_child() {
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0];
        assert_eq!(top_visible_child(&positions, 75.0), Some(3));
    }

    #[test]
    fn top_visible_past_all_children() {
        let positions = vec![0.0, 25.0, 50.0];
        assert_eq!(top_visible_child(&positions, 200.0), Some(2));
    }

    #[test]
    fn top_visible_single_child() {
        let positions = vec![0.0];
        assert_eq!(top_visible_child(&positions, 0.0), Some(0));
        assert_eq!(top_visible_child(&positions, 100.0), Some(0));
    }

    #[test]
    fn top_visible_variable_heights() {
        // Simulate: short(25), long(80), short(25), long(80)
        let positions = vec![0.0, 25.0, 105.0, 130.0];
        // At scroll 30 → we're inside the long message (child 1)
        assert_eq!(top_visible_child(&positions, 30.0), Some(1));
        // At scroll 106 → child 2 (short message after the long one)
        assert_eq!(top_visible_child(&positions, 106.0), Some(2));
    }

    // ── compute_sticky_state tests ──────────────────────────────────

    fn senders(names: &[&str]) -> Vec<(String, bool)> {
        names.iter().map(|n| (n.to_string(), false)).collect()
    }

    const VP: f32 = 400.0; // viewport height for tests

    #[test]
    fn sticky_state_empty() {
        assert_eq!(compute_sticky_state(&[], &[], 0.0, 20.0, VP), None);
    }

    #[test]
    fn sticky_state_at_top_no_sticky() {
        let positions = vec![0.0, 25.0, 50.0];
        let senders = senders(&["Alice", "", ""]);
        assert_eq!(compute_sticky_state(&positions, &senders, 0.0, 20.0, VP), None);
    }

    #[test]
    fn sticky_state_scrolled_shows_sender_with_outgoing() {
        // 3 rows from Alice, then 3 from Bob
        // Row 0: Alice msg1, Row 1: Alice msg2, Row 2: Alice msg3
        // Row 3: Bob msg1, Row 4: Bob msg2, Row 5: Bob msg3
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0, 125.0];
        let senders = senders(&["Alice", "", "", "Bob", "", ""]);
        // Scrolled to 30px — Alice sticks to top
        let state = compute_sticky_state(&positions, &senders, 30.0, 20.0, VP).unwrap();
        assert_eq!(state.primary.sender_index, 0); // Alice
        assert_eq!(state.primary.y_offset, 0.0); // pinned
        // Bob's first message is row 3 (y=75), viewport_y = 75-30 = 45
        assert_eq!(state.outgoing.len(), 1);
        assert_eq!(state.outgoing[0].sender_index, 3);
        assert_eq!(state.outgoing[0].y_offset, 45.0);
    }

    #[test]
    fn sticky_state_no_outgoing_when_next_header_off_screen() {
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0, 125.0];
        let senders = senders(&["Alice", "", "", "Bob", "", ""]);
        // Scrolled to 80px — Bob is primary. No sender after Bob → no outgoing.
        let state = compute_sticky_state(&positions, &senders, 80.0, 20.0, VP).unwrap();
        assert_eq!(state.primary.sender_index, 3); // Bob
        assert_eq!(state.primary.y_offset, 0.0); // pinned
        assert!(state.outgoing.is_empty()); // no sender after Bob
    }

    #[test]
    fn sticky_state_multiple_outgoing_visible() {
        // Alice rows 0-2, Bob rows 3-5, Charlie rows 6-8
        let positions = vec![0.0, 25.0, 50.0, 100.0, 125.0, 150.0, 200.0, 225.0, 250.0];
        let senders = senders(&["Alice", "", "", "Bob", "", "", "Charlie", "", ""]);
        let state = compute_sticky_state(&positions, &senders, 10.0, 20.0, VP).unwrap();
        assert_eq!(state.primary.sender_index, 0); // Alice
        // Bob first msg at row 3 (y=100), viewport_y=90
        // Charlie first msg at row 6 (y=200), viewport_y=190
        assert_eq!(state.outgoing.len(), 2);
        assert_eq!(state.outgoing[0].sender_index, 3); // Bob
        assert_eq!(state.outgoing[0].y_offset, 90.0);
        assert_eq!(state.outgoing[1].sender_index, 6); // Charlie
        assert_eq!(state.outgoing[1].y_offset, 190.0);
    }

    #[test]
    fn sticky_state_outgoing_stops_at_viewport_edge() {
        // Alice rows 0-4, Bob rows 5-6, Charlie row 7. VP=400, scroll=10.
        // Charlie's first msg at y=500, viewport_y=490 > 400 → off screen
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0, 500.0, 525.0, 600.0];
        let senders = senders(&["Alice", "", "", "", "", "Bob", "", "Charlie"]);
        let state = compute_sticky_state(&positions, &senders, 10.0, 20.0, VP).unwrap();
        assert_eq!(state.primary.sender_index, 0);
        // Bob at viewport 500-10=490 > 400 → off screen
        assert!(state.outgoing.is_empty());
    }

    #[test]
    fn sticky_state_switches_to_next_sender() {
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0, 125.0];
        let senders = senders(&["Alice", "", "", "Bob", "", ""]);
        // Scrolled to 80px — past Bob's header at 75px → Bob is primary
        let state = compute_sticky_state(&positions, &senders, 80.0, 20.0, VP).unwrap();
        assert_eq!(state.primary.sender_index, 3); // Bob
        assert_eq!(state.primary.y_offset, 0.0);
        assert!(state.outgoing.is_empty()); // no sender after Bob
    }

    #[test]
    fn sticky_state_with_date_separator() {
        let positions = vec![0.0, 25.0, 50.0, 80.0, 105.0];
        let senders = senders(&["Alice", "", "", "Bob", ""]);
        let state = compute_sticky_state(&positions, &senders, 30.0, 20.0, VP).unwrap();
        assert_eq!(state.primary.sender_index, 0); // Alice
        assert_eq!(state.primary.y_offset, 0.0);
        // Bob first msg at row 3 (y=80), viewport_y=50
        assert_eq!(state.outgoing.len(), 1);
        assert_eq!(state.outgoing[0].sender_index, 3);
        assert_eq!(state.outgoing[0].y_offset, 50.0);
    }

    #[test]
    fn sticky_state_outgoing_position_at_first_message() {
        // Row 0: Alice msg1 (y=0), Row 1: Alice msg2 (y=30), Row 2: Alice msg3 (y=60)
        // Row 3: Bob msg1 (y=90), Row 4: Bob msg2 (y=120), Row 5: Bob msg3 (y=150)
        let positions = vec![0.0, 30.0, 60.0, 90.0, 120.0, 150.0];
        let senders = senders(&["Alice", "", "", "Bob", "", ""]);
        let state = compute_sticky_state(&positions, &senders, 10.0, 20.0, VP).unwrap();
        // Bob's first message is row 3 (y=90), viewport_y = 90-10 = 80
        assert_eq!(state.outgoing.len(), 1);
        assert_eq!(state.outgoing[0].sender_index, 3); // Bob
        assert_eq!(state.outgoing[0].y_offset, 80.0);
    }
}
