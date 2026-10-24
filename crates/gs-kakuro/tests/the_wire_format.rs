//! The bytes a package carries, and everything they refuse.
//!
//! This format is stricter than either other family's, and the reason is worth stating
//! where the tests are. A Kakuro's block pattern and its totals are not two independent
//! lists: reading the blocks produces the runs, and the totals only mean anything in
//! the order those runs come out. So a file that is wrong about the blocks is not a file
//! with a wrong board — it is a file whose totals now belong to different runs, which
//! would read as a perfectly legal *different* puzzle.
//!
//! That is why so much of this file is about refusals. The signature is over these
//! bytes, not over what they mean, so bytes that decode to the wrong thing decode to
//! something that still verifies.

mod common;

use common::{from_answer, layout};
use gs_kakuro::clues::{from_canonical_bytes, DecodeError, BLOCK, WHITE};
use gs_kakuro::rules::Kakuro;
use gs_kakuro::totals::Totals;

/// A small board with a known answer, used throughout.
fn a_puzzle() -> Kakuro {
    // 2 by 2, all white. Four runs: two rows and two columns.
    let (puzzle, _) = from_answer(&["..", ".."], &[1, 5, 2, 6]);
    puzzle
}

#[test]
fn a_puzzle_survives_the_round_trip() {
    let puzzle = a_puzzle();
    let bytes = puzzle.canonical_bytes();
    let back = from_canonical_bytes(&bytes).expect("what we wrote reads back");
    assert_eq!(back, puzzle);
}

#[test]
fn a_board_with_blocks_in_it_survives_the_round_trip() {
    // The case that matters: the blocks are what make the runs, so a board whose shape
    // is interesting is the one worth round-tripping.
    let (puzzle, _) = from_answer(&[".#.", "...", ".#."], &[3, 7, 1, 2, 4, 5, 6]);
    let back = from_canonical_bytes(&puzzle.canonical_bytes()).expect("reads back");
    assert_eq!(back, puzzle);
    assert_eq!(back.layout.run_count(), puzzle.layout.run_count());
}

#[test]
fn writing_the_same_puzzle_twice_gives_the_same_bytes() {
    // The fingerprint is over these bytes, so a format that wrote a puzzle two ways
    // would give one puzzle two fingerprints and the store would hold it twice.
    let puzzle = a_puzzle();
    assert_eq!(puzzle.canonical_bytes(), puzzle.canonical_bytes());
}

#[test]
fn two_boards_of_the_same_size_with_different_blocks_write_differently() {
    let (open, _) = from_answer(&["...", "..."], &[1, 2, 3, 4, 5, 6]);
    let (cut, _) = from_answer(&[".#.", "..."], &[1, 2, 3, 4, 5]);
    assert_ne!(open.canonical_bytes(), cut.canonical_bytes());
}

#[test]
fn a_block_byte_is_not_zero_and_neither_is_a_white_one() {
    // A page of zeroes is what a truncated or zero-filled download looks like. If
    // `WHITE` were zero it would decode as a legal all-white board, which is a real
    // Kakuro, so nothing downstream would notice.
    assert_ne!(BLOCK, 0);
    assert_ne!(WHITE, 0);
    assert_ne!(BLOCK, WHITE);

    let puzzle = a_puzzle();
    let mut bytes = puzzle.canonical_bytes();
    // Zero the block pattern, keeping the header and the totals.
    for byte in bytes.iter_mut().skip(8).take(4) {
        *byte = 0;
    }
    assert!(matches!(
        from_canonical_bytes(&bytes),
        Err(DecodeError::NotABlockOrWhite { .. })
    ));
}

#[test]
fn a_byte_nobody_wrote_is_refused_rather_than_read_as_white() {
    let puzzle = a_puzzle();
    let mut bytes = puzzle.canonical_bytes();
    bytes[8 + 2] = 0x42;
    match from_canonical_bytes(&bytes) {
        Err(DecodeError::NotABlockOrWhite { square, byte }) => {
            assert_eq!(square, 2);
            assert_eq!(byte, 0x42);
        }
        other => panic!("expected a refusal naming the square, got {other:?}"),
    }
}

#[test]
fn bytes_that_stop_part_way_through_are_refused() {
    let puzzle = a_puzzle();
    let bytes = puzzle.canonical_bytes();
    // Every prefix short of the whole thing. None of them may decode: a prefix of a
    // board is not a smaller board.
    for cut in 0..bytes.len() {
        assert!(
            from_canonical_bytes(&bytes[..cut]).is_err(),
            "{cut} bytes of {} decoded, and should not have",
            bytes.len()
        );
    }
}

#[test]
fn extra_bytes_after_a_whole_puzzle_are_refused() {
    let puzzle = a_puzzle();
    let mut bytes = puzzle.canonical_bytes();
    bytes.push(0);
    // A trailing byte makes the totals an odd number of bytes long, so this is caught
    // as a truncated total rather than as trailing rubbish. Either is a refusal, which
    // is the whole claim; the test says which one so that a change to the layout of the
    // format shows up here rather than passing by accident.
    assert!(matches!(
        from_canonical_bytes(&bytes),
        Err(DecodeError::Truncated { field: "totals" })
    ));

    bytes.push(0);
    // Two extra bytes read as one more total than there are runs.
    assert!(matches!(
        from_canonical_bytes(&bytes),
        Err(DecodeError::TotalCountDisagreesWithRuns { .. })
    ));
}

#[test]
fn a_stated_size_that_does_not_match_the_squares_is_refused() {
    let puzzle = a_puzzle();
    let mut bytes = puzzle.canonical_bytes();
    // Claim nine squares on a board stated as 2 by 2.
    bytes[4] = 9;
    match from_canonical_bytes(&bytes) {
        Err(DecodeError::CountDisagreesWithSize { expected, found }) => {
            assert_eq!(expected, 4);
            assert_eq!(found, 9);
        }
        other => panic!("expected a size disagreement, got {other:?}"),
    }
}

#[test]
fn a_width_and_height_that_would_wrap_are_refused() {
    // Both at 65535, whose product overflows a 32-bit count. Multiplied in `usize`
    // after widening, so it cannot come out small and then agree with a small stated
    // count — which would have let a two-byte file claim to be a board of four
    // thousand million squares.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&u16::MAX.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    assert!(matches!(
        from_canonical_bytes(&bytes),
        Err(DecodeError::CountDisagreesWithSize { .. })
    ));
}

#[test]
fn a_different_number_of_totals_than_runs_is_refused() {
    // The refusal this format exists for. The blocks say four runs; the file gives
    // three totals. Lining up what fits and ignoring the rest would produce a legal
    // puzzle that nobody made and that still passes its signature.
    let puzzle = a_puzzle();
    let bytes = puzzle.canonical_bytes();
    let shortened = &bytes[..bytes.len() - 2];
    match from_canonical_bytes(shortened) {
        Err(DecodeError::TotalCountDisagreesWithRuns { runs, totals }) => {
            assert_eq!(runs, 4);
            assert_eq!(totals, 3);
        }
        other => panic!("expected a run and total disagreement, got {other:?}"),
    }
}

#[test]
fn changing_one_block_can_give_a_different_legal_puzzle_and_the_format_cannot_tell() {
    // **What this format does not do**, written down so nobody expects it to.
    //
    // Adding a block does not always change how many runs there are. On a 3 by 2 board
    // blocking the top-left corner leaves five runs, the same five it started with: the
    // top row shortens, the left column shortens to one square, and nothing is created
    // or destroyed. The old totals then fit the new runs, and the file decodes — into a
    // puzzle nobody made.
    //
    // The run count check in `from_canonical_bytes` catches a file whose totals cannot
    // *possibly* belong to its blocks. It cannot catch one where they can, and no check
    // over these bytes could, because the result is a legal Kakuro.
    //
    // What catches it is a level up and is not this crate's: the package's fingerprint
    // is computed over exactly these bytes and the signature is over the fingerprint,
    // so a byte changed after signing fails `gs_core::package::open` before any of this
    // runs. That is the whole reason the payload is a byte string with one spelling
    // rather than a structure a reader reconstructs.
    let (puzzle, _) = from_answer(&["...", "..."], &[1, 2, 3, 4, 5, 6]);
    let mut bytes = puzzle.canonical_bytes();
    assert_eq!(bytes[8], WHITE);
    bytes[8] = BLOCK;

    let changed = from_canonical_bytes(&bytes).expect("it is a legal board, which is the point");
    assert_ne!(changed, puzzle, "it is a different puzzle");
    assert_eq!(
        changed.layout.run_count(),
        puzzle.layout.run_count(),
        "and the run count did not give it away, which is why the signature has to"
    );
    assert_ne!(
        changed.canonical_bytes(),
        puzzle.canonical_bytes(),
        "the bytes differ, so the fingerprint differs, so the signature fails"
    );
}

#[test]
fn a_total_no_run_could_make_is_refused() {
    // Reached through the format rather than through `Totals::new`, because that is the
    // path a signed file takes. A run of two squares can make 3 at the least and 17 at
    // the most, so 99 is not a hard puzzle, it is a number that means nothing.
    let layout = layout(&[".."]);
    let totals = Totals::new(&layout, vec![3, 1, 2]);
    assert!(
        totals.is_ok(),
        "the sane case must build, or this proves nothing"
    );

    let puzzle = a_puzzle();
    let mut bytes = puzzle.canonical_bytes();
    let first_total = 8 + 4;
    bytes[first_total..first_total + 2].copy_from_slice(&99_u16.to_le_bytes());
    assert!(matches!(
        from_canonical_bytes(&bytes),
        Err(DecodeError::Totals(_))
    ));
}

#[test]
fn every_refusal_says_something_a_person_could_read() {
    // These sentences reach a player through the page's "this puzzle is correctly
    // signed, but its board does not read" message, so a variant name is not good
    // enough. Checked as a property rather than one message at a time: the point is
    // that none of them is empty or a debug dump.
    let puzzle = a_puzzle();
    let bytes = puzzle.canonical_bytes();
    let mut broken: Vec<Vec<u8>> = vec![
        bytes[..3].to_vec(),
        bytes[..bytes.len() - 2].to_vec(),
        Vec::new(),
    ];
    let mut bad_byte = bytes.clone();
    bad_byte[8] = 7;
    broken.push(bad_byte);
    let mut bad_size = bytes.clone();
    bad_size[4] = 9;
    broken.push(bad_size);

    for bad in broken {
        let why = from_canonical_bytes(&bad).expect_err("these are all broken");
        let said = why.to_string();
        assert!(!said.is_empty(), "{why:?} said nothing");
        assert!(
            said.chars().next().is_some_and(char::is_lowercase),
            "{said:?} should read as part of a sentence, not start with a capital"
        );
    }
}
