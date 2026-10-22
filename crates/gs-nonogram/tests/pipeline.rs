//! Clue derivation, the line solver, and the properties the slice rests on.

use gs_core::ruleset::{Illegal, Ruleset};
use gs_core::state::{Cell, Dims, State};
use gs_core::timeline::{replay, Timeline};
use gs_nonogram::clues::{runs_of, Clues, Pattern};
use gs_nonogram::line::Mark;
use gs_nonogram::rules::{Action, Nonogram, EMPTY, FILLED};
use gs_nonogram::solve::{from_state, solve, Analysis, Budget, Solved};

/// A 5x5 cross. Line-solvable, and the clues admit only this board.
fn cross() -> Pattern {
    Pattern::from_rows(&["..#..", "..#..", "#####", "..#..", "..#.."]).expect("rectangular")
}

fn puzzle_from(pattern: &Pattern) -> Nonogram {
    Nonogram::new(Clues::of(pattern))
}

#[test]
fn run_lengths_are_derived_the_one_way() {
    assert_eq!(runs_of(&[]), Vec::<u16>::new());
    assert_eq!(runs_of(&[false, false]), Vec::<u16>::new());
    assert_eq!(runs_of(&[true, true, false, true]), vec![2, 1]);
    assert_eq!(runs_of(&[true]), vec![1]);
    // An all-empty line is the empty list, never `[0]`: two spellings of one thing is
    // how two layers come to disagree.
    assert!(runs_of(&[false, false, false]).is_empty());
}

#[test]
fn clues_derived_from_a_pattern_describe_that_pattern() {
    let pattern = cross();
    let clues = Clues::of(&pattern);
    assert_eq!(
        clues.rows,
        vec![vec![1], vec![1], vec![5], vec![1], vec![1]]
    );
    assert_eq!(
        clues.columns,
        vec![vec![1], vec![1], vec![5], vec![1], vec![1]]
    );
    assert!(clues.totals_agree());
}

#[test]
fn the_cross_is_solvable_by_propagation_alone_and_is_unique() {
    let puzzle = puzzle_from(&cross());
    let Analysis {
        solved,
        features,
        witness,
    } = solve(&puzzle, Budget::default());

    assert_eq!(solved, Solved::One);
    assert!(features.line_solvable(), "the cross needs no guessing");
    assert_eq!(features.decision_points, 0);
    assert_eq!(features.max_search_depth, 0);
    assert_eq!(features.forced_ratio_per_mille(), 1000);

    // The witness is the pattern the clues were derived from.
    let witness = witness.expect("a solution was found");
    let expected = cross();
    let marks = from_state(&witness);
    for y in 0..5u16 {
        for x in 0..5u16 {
            let i = usize::from(y) * 5 + usize::from(x);
            let want = if expected.is_filled(x, y) {
                Mark::Filled
            } else {
                Mark::Empty
            };
            assert_eq!(marks[i], want, "cell ({x},{y})");
        }
    }
}

#[test]
fn an_ambiguous_clue_set_is_reported_as_several_not_as_unique() {
    // The classic ambiguity: a 2x2 board with one filled cell per row and column has
    // two solutions, the two diagonals. Catching this is the entire purpose of the
    // uniqueness check, and a filter that called it unique would be the worst
    // possible failure.
    let clues = Clues {
        dims: Dims::new(2, 2),
        rows: vec![vec![1], vec![1]],
        columns: vec![vec![1], vec![1]],
    };
    let analysis = solve(&Nonogram::new(clues), Budget::default());
    assert_eq!(analysis.solved, Solved::Several);
    assert!(
        !analysis.features.line_solvable(),
        "an ambiguous puzzle cannot be line-solvable"
    );
}

#[test]
fn an_unsatisfiable_clue_set_is_reported_as_none() {
    // Row totals say three filled cells, column totals say none.
    let clues = Clues {
        dims: Dims::new(3, 1),
        rows: vec![vec![3]],
        columns: vec![vec![], vec![], vec![]],
    };
    let analysis = solve(&Nonogram::new(clues), Budget::default());
    assert_eq!(analysis.solved, Solved::None);
    assert!(analysis.witness.is_none());
}

#[test]
fn an_exhausted_budget_is_undecided_and_never_unique() {
    // A board with no clue information at all: every cell is free, so the search
    // cannot settle uniqueness, and a budget of almost nothing guarantees it stops
    // early. `Undecided` is the only honest answer. This is the adversarial case for
    // the whole policy: a timeout must never be reported as a proof.
    let n = 6u16;
    let clues = Clues {
        dims: Dims::new(n, n),
        rows: (0..n).map(|_| vec![1, 1]).collect(),
        columns: (0..n).map(|_| vec![1, 1]).collect(),
    };
    let analysis = solve(
        &Nonogram::new(clues),
        Budget {
            max_nodes: 2,
            max_depth: 1,
        },
    );
    assert_eq!(analysis.solved, Solved::Undecided);
    assert_ne!(analysis.solved, Solved::One);
}

#[test]
fn a_witness_replays_through_the_real_engine() {
    // The architecture plan's runtime-consistency check, in miniature: the solution
    // the solver found is played move by move through the same code a player's moves
    // go through, and the goal predicate must agree at the end.
    let puzzle = puzzle_from(&cross());
    let witness = solve(&puzzle, Budget::default())
        .witness
        .expect("a solution");

    let mut timeline = Timeline::new(&puzzle, State::blank(puzzle.clues.dims));
    for i in 0..25u32 {
        let value = witness.get(i).expect("in bounds");
        timeline
            .play(&puzzle, Action::Set { index: i, value })
            .expect("every cell of a solution is a legal move");
    }

    assert!(puzzle.is_solved(timeline.state()));
    assert_eq!(*timeline.state(), witness);

    // And the recorded session replays to the same hashes.
    let log = timeline.action_log().expect("log");
    let replayed = replay(&puzzle, &log).expect("replay");
    assert_eq!(replayed.hash(), timeline.hash());
}

#[test]
fn the_goal_predicate_checks_the_clues_not_a_remembered_answer() {
    // A board that is complete and wrong must not be solved. Comparing against one
    // stored solution would also reject a *second valid* answer, which is the
    // solver's question, asked before publication.
    let puzzle = puzzle_from(&cross());
    let mut wrong = State::blank(puzzle.clues.dims);
    for i in 0..25u32 {
        wrong.set(i, FILLED).expect("in bounds");
    }
    assert!(!puzzle.is_solved(&wrong));

    let mut blank_but_decided = State::blank(puzzle.clues.dims);
    for i in 0..25u32 {
        blank_but_decided.set(i, EMPTY).expect("in bounds");
    }
    assert!(!puzzle.is_solved(&blank_but_decided));
}

#[test]
fn an_incomplete_board_is_not_solved_however_consistent_it_is() {
    let puzzle = puzzle_from(&cross());
    let mut partial = State::blank(puzzle.clues.dims);
    // The centre row, correct as far as it goes.
    for x in 0..5u32 {
        partial.set(10 + x, FILLED).expect("in bounds");
    }
    assert!(!puzzle.is_solved(&partial));
}

#[test]
fn only_the_three_cell_values_are_legal_moves() {
    let puzzle = puzzle_from(&cross());
    let state = State::blank(puzzle.clues.dims);

    assert!(puzzle
        .transition(
            &state,
            &Action::Set {
                index: 0,
                value: FILLED
            }
        )
        .is_ok());
    assert!(puzzle
        .transition(
            &state,
            &Action::Set {
                index: 0,
                value: EMPTY
            }
        )
        .is_ok());
    // Clearing a cell is a real move, not the absence of one.
    assert_eq!(
        puzzle.transition(
            &state,
            &Action::Set {
                index: 0,
                value: Cell::UNKNOWN
            }
        ),
        Err(Illegal::NoOp),
        "the cell is already unknown"
    );
    assert_eq!(
        puzzle.transition(
            &state,
            &Action::Set {
                index: 0,
                value: Cell(7)
            }
        ),
        Err(Illegal::NotPermitted("a cell is filled, empty or unknown"))
    );
    assert_eq!(
        puzzle.transition(
            &state,
            &Action::Set {
                index: 99,
                value: FILLED
            }
        ),
        Err(Illegal::OutOfBounds)
    );
}
