//! Fixtures the tests share: drawing loops on a board by hand.

use gs_slitherlink::board::Geometry;
use gs_slitherlink::loops::Loop;

/// A blank set of drawn edges for a board.
#[must_use]
pub fn nothing_drawn(g: Geometry) -> Vec<bool> {
    vec![false; g.edge_count()]
}

/// Draw the border of the rectangle of cells from `(x0, y0)` to `(x1, y1)`
/// inclusive, onto an existing set of drawn edges.
///
/// Written in cell coordinates because that is how a person describes a loop ("round
/// these four cells"), and the test is then about the rule rather than about edge
/// numbering.
pub fn add_rectangle(g: Geometry, drawn: &mut [bool], x0: u16, y0: u16, x1: u16, y1: u16) {
    let mut mark = |edge: Option<gs_slitherlink::board::EdgeId>| {
        if let Some(edge) = edge {
            if let Some(slot) = drawn.get_mut(edge.0 as usize) {
                *slot = true;
            }
        }
    };
    for x in x0..=x1 {
        mark(g.across(x, y0));
        mark(g.across(x, y1 + 1));
    }
    for y in y0..=y1 {
        mark(g.down(x0, y));
        mark(g.down(x1 + 1, y));
    }
}

/// One rectangular loop round the given cells.
#[must_use]
pub fn rectangle(g: Geometry, x0: u16, y0: u16, x1: u16, y1: u16) -> Vec<bool> {
    let mut drawn = nothing_drawn(g);
    add_rectangle(g, &mut drawn, x0, y0, x1, y1);
    drawn
}

/// A loop round the given cells, already checked.
#[must_use]
pub fn rectangle_loop(g: Geometry, x0: u16, y0: u16, x1: u16, y1: u16) -> Loop {
    Loop::of(g, rectangle(g, x0, y0, x1, y1)).expect("a rectangle is one loop")
}

#[must_use]
pub fn board(w: u16, h: u16) -> Geometry {
    Geometry::new(w, h).expect("a sensible board")
}
