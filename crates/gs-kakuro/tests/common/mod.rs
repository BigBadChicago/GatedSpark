//! Shared helpers for this crate's tests.

// Shared by every test file in this crate. Each test binary gets its own copy of this
// module and uses some of it, so every helper is unused somewhere. CI builds with
// warnings as errors, and the alternative is splitting the helpers by which file wants
// them, which is a worse arrangement than this line. Same reasoning as
// `gs-slitherlink/tests/common/mod.rs`.
#![allow(dead_code)]

use gs_core::state::{Cell, State};
use gs_kakuro::layout::{Layout, WhiteId};
use gs_kakuro::rules::Kakuro;
use gs_kakuro::totals::Totals;

/// Build a layout from a picture, one string per row.
///
/// `#` is a block and `.` is a white square. Written as a picture because a block
/// pattern given as a list of booleans is unreadable, and an unreadable fixture is a
/// fixture nobody checks.
#[must_use]
pub fn layout(rows: &[&str]) -> Layout {
    let height = u16::try_from(rows.len()).expect("height fits");
    let width = u16::try_from(rows.first().map_or(0, |r| r.chars().count())).expect("width fits");
    let mut blocks = Vec::new();
    for row in rows {
        assert_eq!(
            row.chars().count(),
            usize::from(width),
            "every row must be the same width"
        );
        for ch in row.chars() {
            match ch {
                '#' => blocks.push(true),
                '.' => blocks.push(false),
                other => panic!("a picture holds only # and ., not {other:?}"),
            }
        }
    }
    Layout::new(width, height, blocks).expect("the picture makes a legal layout")
}

/// A puzzle whose totals are derived from the digits given, in white order.
///
/// Returns the puzzle and the finished board, so a test can start from the answer
/// and take it apart.
#[must_use]
pub fn from_answer(rows: &[&str], digits: &[i16]) -> (Kakuro, State) {
    let layout = layout(rows);
    assert_eq!(
        digits.len(),
        layout.white_count(),
        "one digit per white square"
    );
    let totals = Totals::of(&layout, &|white: WhiteId| {
        digits.get(usize::try_from(white.0).ok()?).copied()
    })
    .expect("a full board implies totals");
    let mut state = State::blank(layout.state_dims());
    for (index, digit) in digits.iter().enumerate() {
        let white = u32::try_from(index).expect("index fits");
        state.set(white, Cell(*digit)).expect("white square exists");
    }
    let puzzle = Kakuro::new(layout, totals).expect("totals fit the runs");
    (puzzle, state)
}
