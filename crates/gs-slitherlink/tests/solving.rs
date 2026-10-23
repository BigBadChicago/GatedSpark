//! Our own fast solver: what it finds, and what it refuses to claim.

mod common;

use common::{add_rectangle, board, nothing_drawn, rectangle_loop};
use gs_core::ruleset::Ruleset;
use gs_slitherlink::board::{CellId, Geometry};
use gs_slitherlink::clues::Clues;
use gs_slitherlink::rules::Slitherlink;
use gs_slitherlink::solve::{solve, Budget, Solved};

fn numbers_for(g: Geometry, drawn: &[bool]) -> Clues {
    let numbers = g
        .cells()
        .map(|cell| {
            let sides = g.edges_of_cell(cell).expect("four sides");
            let used = sides
                .iter()
                .filter(|e| drawn.get(e.0 as usize).copied().unwrap_or(false))
                .count();
            Some(u8::try_from(used).expect("at most four"))
        })
        .collect();
    Clues::new(g, numbers).expect("a grid")
}

#[test]
fn a_fully_numbered_board_has_exactly_one_answer() {
    let g = board(4, 4);
    let answer = rectangle_loop(g, 1, 1, 2, 2);
    let p = Slitherlink::new(Clues::of(&answer));
    let found = solve(&p, Budget::default());
    assert_eq!(found.solved, Solved::One);
    let witness = found.witness.expect("an answer");
    assert!(
        p.is_solved(&witness),
        "and the engine agrees it is finished"
    );
    assert_eq!(witness, answer.as_state(), "and it is the loop we drew");
}

#[test]
fn the_numbers_are_recorded_and_say_what_this_solver_did() {
    // Not an assertion that a fully numbered board needs no guessing. It does not,
    // and finding that out is why this test reads the way it does: our deduction
    // rules are a small honest subset of what a Slitherlink player knows, so a board
    // a person finds trivial can still cost this solver a guess. Measured on
    // 2026-10-23, a fully numbered 5 by 5 costs exactly one. The warning on
    // `Features::decision_points` carries the consequence for difficulty.
    let g = board(5, 5);
    let answer = rectangle_loop(g, 1, 1, 3, 3);
    let p = Slitherlink::new(Clues::of(&answer));
    let found = solve(&p, Budget::default());
    assert_eq!(found.solved, Solved::One);
    assert!(found.features.forced_steps >= 1);
    assert_eq!(found.features.edges, g.edge_count() as u32);
    assert_eq!(found.features.clues_given, 25);
    assert!(
        found.features.propagation_only_edges > 0,
        "the forced deductions settled nothing at all, which would mean they are broken"
    );
    assert!(
        found.features.decision_points <= 4,
        "a fully numbered board should be nearly free, and took {} guesses",
        found.features.decision_points
    );
}

#[test]
fn a_board_with_no_numbers_has_several_answers() {
    // The control. An over-strict solver would answer "one" or "none" everywhere and
    // look like it was working.
    let g = board(3, 3);
    let p = Slitherlink::new(Clues::none(g));
    assert_eq!(solve(&p, Budget::default()).solved, Solved::Several);
}

#[test]
fn a_board_of_noughts_has_no_answer() {
    let g = board(3, 3);
    let p = Slitherlink::new(Clues::new(g, vec![Some(0); g.cell_count()]).expect("a grid"));
    let found = solve(&p, Budget::default());
    assert_eq!(found.solved, Solved::None);
    assert_eq!(found.witness, None, "and it offers no witness");
}

#[test]
fn two_rings_are_refused() {
    // The case the whole family is about, now through our own solver rather than
    // through Z3. The numbers come off a board of two separate rings, which is the
    // only board that fits them, and it is not a legal answer.
    let g = board(4, 4);
    let mut drawn = nothing_drawn(g);
    add_rectangle(g, &mut drawn, 0, 0, 0, 0);
    add_rectangle(g, &mut drawn, 2, 2, 3, 3);
    let p = Slitherlink::new(numbers_for(g, &drawn));
    assert_eq!(solve(&p, Budget::default()).solved, Solved::None);
}

#[test]
fn an_exhausted_budget_is_never_reported_as_one_answer() {
    // The rule that matters most about this solver. "Undecided" and "exactly one" are
    // different answers, and a filter that confused them would wave through puzzles
    // nothing had checked.
    let g = board(6, 6);
    let p = Slitherlink::new(Clues::none(g));
    let tiny = Budget {
        max_nodes: 1,
        max_depth: 1,
    };
    let found = solve(&p, tiny);
    assert!(
        matches!(found.solved, Solved::Undecided | Solved::Several),
        "a one-node budget gave {:?}",
        found.solved
    );
    assert_ne!(found.solved, Solved::One);
    assert_ne!(found.solved, Solved::None);
}

#[test]
fn a_witness_is_a_whole_board() {
    // A stored answer decides every edge, which `publish` requires separately. The
    // solver has to produce one, so this is where that starts.
    let g = board(4, 3);
    let answer = rectangle_loop(g, 0, 0, 2, 1);
    let p = Slitherlink::new(Clues::of(&answer));
    let witness = solve(&p, Budget::default()).witness.expect("an answer");
    assert!(
        g.edges()
            .all(|e| witness.get(e.0).map(|c| !c.is_unknown()) == Ok(true)),
        "every edge is decided"
    );
}

#[test]
fn taking_a_number_away_can_add_an_answer_and_that_is_the_whole_game() {
    // Part three in one test. A fully numbered board has one answer. Strip it right
    // down and more answers appear. Somewhere in between is the puzzle worth playing,
    // and finding that point is what the generator does.
    let g = board(4, 4);
    let answer = rectangle_loop(g, 1, 1, 2, 2);
    let full = Clues::of(&answer);
    assert_eq!(
        solve(&Slitherlink::new(full.clone()), Budget::default()).solved,
        Solved::One
    );

    let mut stripped = full;
    for cell in g.cells() {
        stripped = stripped.without(cell);
    }
    assert_eq!(stripped.given_count(), 0);
    assert_eq!(
        solve(&Slitherlink::new(stripped), Budget::default()).solved,
        Solved::Several
    );
}

#[test]
fn a_number_in_the_wrong_place_makes_the_puzzle_unanswerable() {
    let g = board(3, 3);
    let answer = rectangle_loop(g, 0, 0, 1, 1);
    // Cell (2, 2) is well away from the loop, so its real clue is 0. Claiming 3 there
    // cannot be satisfied by any board that also satisfies the rest.
    let broken = Clues::of(&answer).with(CellId(8), 3);
    assert_eq!(
        solve(&Slitherlink::new(broken), Budget::default()).solved,
        Solved::None
    );
}

#[test]
fn the_solver_and_the_engine_never_disagree_about_an_answer() {
    // Every witness this solver produces must pass the engine's own goal test. If the
    // two ever disagreed, the one that ships is the engine, so the solver would be
    // handing the real verifier a board a player could not finish.
    let shapes: [(u16, u16, u16, u16, u16, u16); 4] = [
        (3, 3, 0, 0, 1, 1),
        (5, 4, 1, 1, 3, 2),
        (6, 2, 0, 0, 5, 1),
        (2, 6, 0, 2, 1, 5),
    ];
    for (w, h, x0, y0, x1, y1) in shapes {
        let g = board(w, h);
        let answer = rectangle_loop(g, x0, y0, x1, y1);
        let p = Slitherlink::new(Clues::of(&answer));
        let found = solve(&p, Budget::default());
        assert_eq!(found.solved, Solved::One, "a {w}x{h} board");
        let witness = found.witness.expect("an answer");
        assert!(p.is_solved(&witness), "a {w}x{h} board: the engine agrees");
    }
}

#[test]
fn nothing_here_can_produce_a_verification_outcome() {
    // The structural claim, as a compile-time fact rather than a comment. `Solved`
    // and `Features` have no conversion into `gs_core::verification::Outcome`, so the
    // fast solver cannot become the gate however it is called. The test is the
    // absence of such a conversion, which is checked by the code below not compiling
    // if one is added — see the doctest in `lib.rs`.
    let g = board(2, 2);
    let answer = rectangle_loop(g, 0, 0, 0, 0);
    let p = Slitherlink::new(Clues::of(&answer));
    let found = solve(&p, Budget::default());
    // The only thing available is a plain answer and some numbers.
    assert_eq!(found.solved, Solved::One);
    assert!(found.features.edges > 0);
}
