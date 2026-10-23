//! The rules a player meets: what a move is, and when the puzzle is finished.

mod common;

use common::{board, rectangle, rectangle_loop};
use gs_core::dsl::{Global, Tier};
use gs_core::registry::{Family, FamilyId, Registry};
use gs_core::ruleset::{Illegal, Ruleset};
use gs_core::state::Cell;
use gs_core::timeline::Timeline;
use gs_slitherlink::board::EdgeId;
use gs_slitherlink::clues::Clues;
use gs_slitherlink::loops::Shape;
use gs_slitherlink::rules::{Action, Slitherlink, FAMILY, LINE, NO_LINE};

/// A 3 by 3 board whose answer is the loop round the four top-left cells, with every
/// cell clued. Small enough to reason about by hand.
fn puzzle() -> (Slitherlink, gs_slitherlink::loops::Loop) {
    let g = board(3, 3);
    let answer = rectangle_loop(g, 0, 0, 1, 1);
    (Slitherlink::new(Clues::of(&answer)), answer)
}

fn draw(edge: EdgeId) -> Action {
    Action::Set { edge, value: LINE }
}

#[test]
fn the_answer_finishes_the_puzzle() {
    let (p, answer) = puzzle();
    assert!(p.is_solved(&answer.as_state()));
}

#[test]
fn a_blank_board_is_not_finished() {
    let (p, _) = puzzle();
    assert!(!p.is_solved(&p.blank()));
}

#[test]
fn drawing_the_loop_and_nothing_else_finishes_the_puzzle() {
    // The decision from 2026-10-23, as a player meets it: draw the loop, and the
    // puzzle is done. No need to rule out the other 20 edges one at a time.
    let (p, answer) = puzzle();
    let g = p.geometry();
    let mut t = Timeline::new(&p, p.blank());
    let mut moves = 0;
    for edge in g.edges() {
        if answer.is_drawn(edge) {
            t.play(&p, draw(edge)).expect("drawing a line is legal");
            moves += 1;
        }
    }
    assert_eq!(moves, 8, "the loop round four cells is eight lines");
    assert!(
        p.is_solved(t.state()),
        "finished without ruling anything out"
    );
    assert!(
        g.edges()
            .any(|e| t.state().get(e.0).map(|c| c.is_unknown()).unwrap_or(false)),
        "and most edges are still undecided"
    );
}

#[test]
fn a_right_loop_with_a_wrong_clue_is_not_finished() {
    // Both halves of the goal are needed. Here the lines make a perfectly good loop
    // and one number disagrees with it, so the puzzle is not done.
    let (_, answer) = puzzle();
    let wrong = Clues::of(&answer).with(gs_slitherlink::board::CellId(8), 3);
    let p = Slitherlink::new(wrong);
    assert!(p.shape(&answer.as_state()).is_one_loop());
    assert!(!p.is_solved(&answer.as_state()));
}

#[test]
fn two_loops_that_satisfy_every_number_are_not_finished() {
    // The case the whole global rule exists for, now as a goal test. The clues are
    // derived from a board of two rings, so every number is met exactly — and the
    // board is still refused, because there may only be one loop.
    let g = board(5, 5);
    let mut drawn = rectangle(g, 0, 0, 1, 1);
    common::add_rectangle(g, &mut drawn, 3, 3, 4, 4);
    let two_rings = gs_slitherlink::loops::Loop::of(g, drawn.clone());
    assert!(two_rings.is_err(), "two rings are not a loop");

    // Build the clue grid by hand from the two-ring board, since `Clues::of` will not
    // take something that is not a loop.
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
    let p = Slitherlink::new(Clues::new(g, numbers).expect("a grid"));

    let mut state = p.blank();
    for edge in g.edges() {
        let value = if drawn[edge.0 as usize] {
            LINE
        } else {
            NO_LINE
        };
        state.set(edge.0, value).expect("an edge on the board");
    }
    assert!(
        p.clues_not_met(&state).next().is_none(),
        "every number is met"
    );
    assert_eq!(p.shape(&state), Shape::SeveralLoops(2));
    assert!(!p.is_solved(&state), "and the puzzle is still not finished");
}

#[test]
fn ruling_an_edge_out_is_a_real_move() {
    let (p, _) = puzzle();
    let mut t = Timeline::new(&p, p.blank());
    let edge = EdgeId(0);
    t.play(
        &p,
        Action::Set {
            edge,
            value: NO_LINE,
        },
    )
    .expect("ruling an edge out is a move");
    assert_eq!(t.state().get(edge.0), Ok(NO_LINE));
    t.undo().expect("and it can be undone");
    assert_eq!(t.state().get(edge.0), Ok(Cell::UNKNOWN));
}

#[test]
fn setting_an_edge_to_what_it_already_is_is_not_a_move() {
    let (p, _) = puzzle();
    let mut t = Timeline::new(&p, p.blank());
    t.play(&p, draw(EdgeId(0))).expect("legal");
    assert_eq!(
        t.play(&p, draw(EdgeId(0))),
        Err(gs_core::timeline::TimelineError::Illegal(Illegal::NoOp))
    );
}

#[test]
fn an_edge_off_the_board_is_refused() {
    let (p, _) = puzzle();
    let past_the_end = EdgeId(u32::try_from(p.geometry().edge_count()).expect("small"));
    assert_eq!(
        p.transition(&p.blank(), &draw(past_the_end)),
        Err(Illegal::OutOfBounds)
    );
}

#[test]
fn a_value_that_is_not_a_mark_is_refused() {
    let (p, _) = puzzle();
    let nonsense = Action::Set {
        edge: EdgeId(0),
        value: Cell(7),
    };
    assert!(matches!(
        p.transition(&p.blank(), &nonsense),
        Err(Illegal::NotPermitted(_))
    ));
}

#[test]
fn undo_and_redo_work_on_a_real_puzzle() {
    let (p, answer) = puzzle();
    let g = p.geometry();
    let mut t = Timeline::new(&p, p.blank());
    for edge in g.edges().filter(|e| answer.is_drawn(*e)) {
        t.play(&p, draw(edge)).expect("legal");
    }
    let finished = t.hash();
    assert!(p.is_solved(t.state()));
    t.undo().expect("something to undo");
    assert!(!p.is_solved(t.state()), "one line short is not finished");
    t.redo().expect("something to redo");
    assert_eq!(t.hash(), finished);
    assert!(p.is_solved(t.state()));
}

#[test]
fn the_rules_are_tier_b_and_name_the_loop() {
    // The first thing in this project to produce a global constraint. The DSL
    // reserved the shape on the slice's first day; this is the test that the
    // reservation was real.
    let (p, _) = puzzle();
    let rules = p.dsl_rules();
    assert_eq!(rules.tier(), Tier::B);
    let globals: Vec<_> = rules
        .constraints
        .iter()
        .flat_map(gs_core::dsl::Constraint::globals)
        .collect();
    assert_eq!(globals.len(), 1, "exactly one loop rule, not one per cell");
    assert!(matches!(
        globals[0],
        Global::SingleClosedLoop { value } if *value == LINE.0
    ));
}

#[test]
fn every_clue_becomes_a_count_over_that_cells_own_four_sides() {
    let (p, _) = puzzle();
    let g = p.geometry();
    let counts: Vec<_> = p
        .dsl_rules()
        .constraints
        .into_iter()
        .filter_map(|c| match c {
            gs_core::dsl::Constraint::Count {
                region: gs_core::dsl::Region::Cells(cells),
                count,
                ..
            } => Some((cells, count)),
            _ => None,
        })
        .collect();
    assert_eq!(
        counts.len() as u32,
        p.clues.given_count(),
        "one count per clue"
    );
    for (cells, _) in &counts {
        assert_eq!(cells.len(), 4, "a cell has four sides");
        assert!(
            cells.iter().all(|e| *e < g.edge_count() as u32),
            "and they are all real edges"
        );
    }
}

#[test]
fn a_puzzle_with_no_clues_still_states_the_loop_rule() {
    // A board with nothing on it is a legal puzzle to describe and an impossible one
    // to solve uniquely. What must not happen is for the rules to come out empty,
    // because an empty rule set is a gate with nothing asserted against it.
    let g = board(3, 3);
    let p = Slitherlink::new(Clues::none(g));
    let rules = p.dsl_rules();
    assert_eq!(rules.tier(), Tier::B);
    assert_eq!(
        rules.constraints.len(),
        1,
        "the loop rule, and nothing else"
    );
}

#[test]
fn the_family_installs_like_any_other() {
    let (p, _) = puzzle();
    assert_eq!(Family::id(&p), FAMILY);
    assert_eq!(FAMILY, FamilyId("slitherlink"));
    let mut registry = Registry::empty();
    registry.install(Box::new(p));
    assert_eq!(registry.installed_count(), 1);
    assert!(registry.resolve(FAMILY).is_ok());
    assert!(
        registry.resolve(FamilyId("nonogram")).is_err(),
        "installing one family does not install another"
    );
}

#[test]
fn the_ruleset_version_is_in_the_puzzles_identity() {
    let (p, _) = puzzle();
    assert_eq!(
        Ruleset::id(&p).version,
        gs_slitherlink::rules::RULESET_VERSION
    );
    assert_eq!(Ruleset::id(&p).family, FAMILY);
}
