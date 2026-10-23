//! Deriving clues from a loop, and reading a clue grid back from bytes.

mod common;

use common::{board, rectangle_loop};
use gs_slitherlink::board::CellId;
use gs_slitherlink::clues::{Clues, CluesError, DecodeError, NO_CLUE};

#[test]
fn a_loop_round_one_cell_gives_that_cell_a_four() {
    let g = board(3, 3);
    let clues = Clues::of(&rectangle_loop(g, 1, 1, 1, 1));
    assert_eq!(clues.get(1, 1), Some(4));
    // Its four neighbours each share one side with it.
    assert_eq!(clues.get(0, 1), Some(1));
    assert_eq!(clues.get(2, 1), Some(1));
    assert_eq!(clues.get(1, 0), Some(1));
    assert_eq!(clues.get(1, 2), Some(1));
    // A cell touching it only at a corner shares no side.
    assert_eq!(clues.get(0, 0), Some(0));
}

#[test]
fn a_cell_inside_a_bigger_loop_gets_a_nought() {
    let g = board(5, 5);
    let clues = Clues::of(&rectangle_loop(g, 1, 1, 3, 3));
    assert_eq!(clues.get(2, 2), Some(0), "fully inside, no side used");
    assert_eq!(clues.get(1, 1), Some(2), "a corner of the loop");
    assert_eq!(clues.get(2, 1), Some(1), "on the top edge of the loop");
}

#[test]
fn derivation_numbers_every_cell_and_the_numbers_add_up() {
    // Every side of the loop is shared by at most two cells, and a side on the rim of
    // the board by one. So the clues must sum to the loop's length plus the number of
    // loop edges that have a cell on both sides. Rather than restate that arithmetic,
    // check the weaker fact that holds for any loop: the clues total at least the
    // loop's length and no clue is above four.
    let g = board(6, 4);
    let the_loop = rectangle_loop(g, 1, 1, 4, 2);
    let clues = Clues::of(&the_loop);
    assert_eq!(clues.given_count(), 24, "every cell is numbered");
    let total: u32 = clues.given().map(|(_, clue)| u32::from(clue)).sum();
    assert!(total >= the_loop.edges_drawn(), "{total} < length");
    assert!(clues.given().all(|(_, clue)| clue <= 4));
}

#[test]
fn an_empty_grid_is_a_thing_that_can_be_built() {
    // The generator removes clues, and this is where that would end up if nothing
    // stopped it. It must be buildable so that the thing stopping it is a check and
    // not a type error.
    let g = board(4, 4);
    let clues = Clues::none(g);
    assert_eq!(clues.given_count(), 0);
    assert_eq!(clues.given().count(), 0);
}

#[test]
fn taking_a_clue_away_and_putting_it_back_is_a_round_trip() {
    let g = board(3, 3);
    let full = Clues::of(&rectangle_loop(g, 0, 0, 1, 1));
    let cell = CellId(4);
    let clue = full.at(cell).expect("a clue");
    let fewer = full.without(cell);
    assert_eq!(fewer.at(cell), None);
    assert_eq!(fewer.given_count(), full.given_count() - 1);
    assert_eq!(fewer.with(cell, clue), full);
}

#[test]
fn a_clue_above_four_is_refused() {
    let g = board(2, 2);
    let mut numbers = vec![Some(0); 4];
    numbers[2] = Some(5);
    assert_eq!(
        Clues::new(g, numbers),
        Err(CluesError::TooLarge { cell: 2, clue: 5 })
    );
}

#[test]
fn a_grid_of_the_wrong_size_is_refused() {
    let g = board(3, 3);
    assert_eq!(
        Clues::new(g, vec![None; 8]),
        Err(CluesError::WrongLength {
            expected: 9,
            found: 8
        })
    );
}

#[test]
fn bytes_round_trip_including_noughts_and_gaps() {
    // Noughts and gaps together are the case an encoding gets wrong, because the
    // tempting shortcut is to write a missing clue as zero.
    let g = board(4, 3);
    let full = Clues::of(&rectangle_loop(g, 1, 0, 2, 1));
    let sparse = full
        .without(CellId(0))
        .without(CellId(7))
        .without(CellId(11));
    for clues in [&full, &sparse] {
        let bytes = clues.canonical_bytes();
        assert_eq!(
            Clues::from_canonical_bytes(&bytes).as_ref(),
            Ok(clues),
            "a clue grid survives the trip into bytes and back"
        );
    }
    assert_ne!(
        full.canonical_bytes(),
        sparse.canonical_bytes(),
        "a grid with a clue removed is not the same bytes"
    );
}

#[test]
fn a_nought_and_a_gap_are_different_bytes() {
    // The whole reason `NO_CLUE` is 255. If a gap were written as zero, a puzzle with
    // a nought in it and a puzzle with nothing there would be one puzzle, and they
    // are not: a nought is a strong clue.
    let g = board(2, 1);
    let nought = Clues::new(g, vec![Some(0), None]).expect("a grid");
    let gap = Clues::new(g, vec![None, None]).expect("a grid");
    assert_ne!(nought.canonical_bytes(), gap.canonical_bytes());
    assert_eq!(nought.canonical_bytes().last(), Some(&NO_CLUE));
}

#[test]
fn a_zero_filled_page_does_not_decode_as_a_blank_board() {
    // A page of zeroes decodes as a board of noughts, if it decodes at all — and a
    // board of noughts has no loop, so the solver refuses it. What must not happen is
    // for zeroes to read as "no clues anywhere", because that is the shape of a file
    // nobody wrote.
    let zeroes = vec![0u8; 16];
    match Clues::from_canonical_bytes(&zeroes) {
        Err(_) => {}
        Ok(clues) => assert!(
            clues.given_count() > 0,
            "zero bytes must not read as a board with no clues"
        ),
    }
}

#[test]
fn an_unknown_byte_is_a_damaged_file_not_a_blank_cell() {
    let g = board(2, 2);
    let mut bytes = Clues::none(g).canonical_bytes();
    let last = bytes.len() - 1;
    bytes[last] = 9;
    assert_eq!(
        Clues::from_canonical_bytes(&bytes),
        Err(DecodeError::NotAClue { cell: 3, byte: 9 })
    );
}

#[test]
fn bytes_left_over_are_refused() {
    let g = board(2, 2);
    let mut bytes = Clues::none(g).canonical_bytes();
    bytes.push(NO_CLUE);
    assert_eq!(
        Clues::from_canonical_bytes(&bytes),
        Err(DecodeError::TrailingBytes(1))
    );
}

#[test]
fn a_truncated_grid_is_refused() {
    let g = board(3, 3);
    let bytes = Clues::none(g).canonical_bytes();
    for cut in 0..bytes.len() {
        assert!(
            Clues::from_canonical_bytes(&bytes[..cut]).is_err(),
            "{cut} bytes of a {} byte grid must not decode",
            bytes.len()
        );
    }
}

#[test]
fn a_stated_count_that_disagrees_with_the_size_is_refused() {
    // The count comes from the size, not from the file, so a file claiming a
    // different number of cells is a refusal rather than a new shape.
    let g = board(3, 3);
    let mut bytes = Clues::none(g).canonical_bytes();
    bytes[4] = 8;
    assert_eq!(
        Clues::from_canonical_bytes(&bytes),
        Err(DecodeError::CountDisagreesWithSize {
            expected: 9,
            found: 8
        })
    );
}
