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

/// Result of sticky header calculation.
#[derive(Debug, Clone, PartialEq)]
pub struct StickyState {
    /// Index into row_senders of the current sticky sender.
    pub sender_index: usize,
    /// How many pixels to push the sticky label down from the top.
    /// 0.0 = pinned at top (normal). >0 = being pushed down by the next header.
    pub push_down: f32,
}

/// Calculate the sticky header state: which sender is sticky and how much
/// it should be pushed down by an incoming next-group header.
///
/// `child_y_positions`: Y offset of each child row (from LayoutTracker)
/// `row_senders`: (sender_name, is_from_me) per row; empty name = separator
/// `scroll_offset`: pixels scrolled from top of content (absolute_offset_reversed)
/// `sticky_height`: height of the sticky label in pixels (for push-down calc)
///
/// Returns None if no sticky should be shown (at very top, or no data).
pub fn compute_sticky_state(
    child_y_positions: &[f32],
    row_senders: &[(String, bool)],
    scroll_offset: f32,
    sticky_height: f32,
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

    // Find the NEXT group header after sender_index:
    // scan forward from sender_index+1 to find the next row with a
    // different non-empty sender name.
    let current_sender = &row_senders[sender_index].0;
    let mut next_header_y: Option<f32> = None;
    let pos_len = child_y_positions.len();
    for i in (sender_index + 1)..row_senders.len().min(pos_len) {
        let (ref name, _) = row_senders[i];
        if !name.is_empty() && name != current_sender {
            next_header_y = Some(child_y_positions[i]);
            break;
        }
    }

    // Calculate push-down: if the next header is close to the viewport top,
    // push the current sticky down so it appears to be "pushed" by the incoming header.
    let push_down = if let Some(next_y) = next_header_y {
        // Distance from viewport top to the next header
        let gap = next_y - scroll_offset;
        if gap < sticky_height {
            // Next header is overlapping — push current sticky down
            (sticky_height - gap).max(0.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    Some(StickyState { sender_index, push_down })
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

    #[test]
    fn sticky_state_empty() {
        assert_eq!(compute_sticky_state(&[], &[], 0.0, 20.0), None);
    }

    #[test]
    fn sticky_state_at_top_no_sticky() {
        let positions = vec![0.0, 25.0, 50.0];
        let senders = senders(&["Alice", "", ""]);
        assert_eq!(compute_sticky_state(&positions, &senders, 0.0, 20.0), None);
    }

    #[test]
    fn sticky_state_scrolled_shows_sender() {
        // 3 rows from Alice, then 3 from Bob
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0, 125.0];
        let senders = senders(&["Alice", "", "", "Bob", "", ""]);
        // Scrolled to 30px — past Alice's header, still in Alice's group
        let state = compute_sticky_state(&positions, &senders, 30.0, 20.0);
        assert_eq!(state, Some(StickyState { sender_index: 0, push_down: 0.0 }));
    }

    #[test]
    fn sticky_state_no_push_when_next_header_far() {
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0, 125.0];
        let senders = senders(&["Alice", "", "", "Bob", "", ""]);
        // Scrolled to 30px, next header (Bob) at 75px → gap=45, sticky_h=20 → no push
        let state = compute_sticky_state(&positions, &senders, 30.0, 20.0).unwrap();
        assert_eq!(state.push_down, 0.0);
    }

    #[test]
    fn sticky_state_push_down_when_next_header_close() {
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0, 125.0];
        let senders = senders(&["Alice", "", "", "Bob", "", ""]);
        // Scrolled to 60px, next header (Bob) at 75px → gap=15, sticky_h=20 → push=5
        let state = compute_sticky_state(&positions, &senders, 60.0, 20.0).unwrap();
        assert_eq!(state.sender_index, 0); // still Alice
        assert_eq!(state.push_down, 5.0);
    }

    #[test]
    fn sticky_state_switches_to_next_sender() {
        let positions = vec![0.0, 25.0, 50.0, 75.0, 100.0, 125.0];
        let senders = senders(&["Alice", "", "", "Bob", "", ""]);
        // Scrolled to 80px — past Bob's header at 75px → Bob is now sticky
        let state = compute_sticky_state(&positions, &senders, 80.0, 20.0).unwrap();
        assert_eq!(state.sender_index, 3); // Bob
        assert_eq!(state.push_down, 0.0); // no next header pushing
    }

    #[test]
    fn sticky_state_with_date_separator() {
        // Rows: Alice, Alice, [date sep], Bob, Bob
        let positions = vec![0.0, 25.0, 50.0, 80.0, 105.0];
        let senders = senders(&["Alice", "", "", "Bob", ""]);
        // Scrolled to 30px — in Alice's group
        let state = compute_sticky_state(&positions, &senders, 30.0, 20.0).unwrap();
        assert_eq!(state.sender_index, 0); // Alice
        // Next header (Bob) at 80px, gap=50, no push
        assert_eq!(state.push_down, 0.0);
    }

    #[test]
    fn sticky_state_push_down_with_separator() {
        // Rows: Alice, Alice, [date sep], Bob, Bob
        let positions = vec![0.0, 25.0, 50.0, 80.0, 105.0];
        let senders = senders(&["Alice", "", "", "Bob", ""]);
        // Scrolled to 65px → gap to Bob at 80px = 15, sticky_h=20 → push=5
        let state = compute_sticky_state(&positions, &senders, 65.0, 20.0).unwrap();
        assert_eq!(state.sender_index, 0); // still Alice
        assert_eq!(state.push_down, 5.0);
    }
}
