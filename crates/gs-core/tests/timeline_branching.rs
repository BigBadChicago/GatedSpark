//! Undo, redo and branching.
//!
//! The requirement under test is the architecture plan's: "new actions after undo
//! create a branch instead of silently corrupting history". The usual
//! implementation — truncate the redo stack on the next move — fails this, and loses
//! work the player did.

mod common;

use common::{paint, PaintRules};
use gs_core::ruleset::Illegal;
use gs_core::state::{Cell, Dims, State};
use gs_core::timeline::{NodeId, Timeline, TimelineError};

fn session() -> (PaintRules, Timeline<PaintRules>) {
    let rules = PaintRules::default();
    let timeline = Timeline::new(&rules, State::blank(Dims::new(3, 3)));
    (rules, timeline)
}

#[test]
fn undo_then_redo_returns_to_the_same_state() {
    let (rules, mut t) = session();
    t.play(&rules, paint(0, 1)).expect("legal");
    let after = t.hash();
    let state_after = t.state().clone();

    t.undo().expect("something to undo");
    assert_ne!(t.hash(), after);
    assert_eq!(t.cursor(), NodeId::ROOT);

    t.redo().expect("something to redo");
    assert_eq!(t.hash(), after);
    assert_eq!(*t.state(), state_after);
}

#[test]
fn undoing_at_the_root_is_an_error_not_a_silent_no_op() {
    let (_rules, mut t) = session();
    assert_eq!(t.undo(), Err(TimelineError::AtRoot));
}

#[test]
fn redoing_with_no_future_is_an_error() {
    let (_rules, mut t) = session();
    assert_eq!(t.redo(), Err(TimelineError::NoRedo));
}

#[test]
fn an_action_after_undo_branches_and_keeps_the_old_future() {
    let (rules, mut t) = session();
    let first = t.play(&rules, paint(4, 1)).expect("legal");
    let first_hash = t.hash();

    t.undo().expect("undo");
    // A different move from the same position.
    let second = t.play(&rules, paint(4, 2)).expect("legal");
    let second_hash = t.hash();
    assert_ne!(first_hash, second_hash);
    assert_ne!(first, second);

    // Both futures still exist.
    t.undo().expect("undo");
    assert_eq!(t.branches_at_cursor(), 2);

    // And the original one is still reachable, unaltered.
    t.redo_into(first).expect("the first branch is still there");
    assert_eq!(t.hash(), first_hash);
    assert_eq!(t.state().get(4).expect("in bounds"), Cell(1));
}

#[test]
fn redo_follows_the_branch_most_recently_visited() {
    let (rules, mut t) = session();
    let first = t.play(&rules, paint(0, 1)).expect("legal");
    t.undo().expect("undo");
    let second = t.play(&rules, paint(0, 2)).expect("legal");
    let second_hash = t.hash();

    t.undo().expect("undo");
    // Visit the older branch, then come back.
    t.redo_into(first).expect("older branch");
    t.undo().expect("undo");
    assert_eq!(
        t.redo().expect("redo"),
        first,
        "redo follows the last visited branch"
    );

    // Visit the newer one, and redo should follow that instead.
    t.undo().expect("undo");
    t.redo_into(second).expect("newer branch");
    t.undo().expect("undo");
    assert_eq!(t.redo().expect("redo"), second);
    assert_eq!(t.hash(), second_hash);
}

#[test]
fn goto_any_node_agrees_with_walking_there() {
    let (rules, mut t) = session();
    // Build a tree: root -> a -> b, and root -> a -> c.
    t.play(&rules, paint(0, 1)).expect("legal");
    let b = t.play(&rules, paint(1, 1)).expect("legal");
    let b_hash = t.hash();
    let b_state = t.state().clone();
    t.undo().expect("undo");
    let c = t.play(&rules, paint(1, 2)).expect("legal");
    let c_hash = t.hash();

    // Jumping across the tree, not just up and down one path.
    t.goto(b).expect("reachable");
    assert_eq!(t.hash(), b_hash);
    assert_eq!(*t.state(), b_state);

    t.goto(c).expect("reachable");
    assert_eq!(t.hash(), c_hash);

    t.goto(NodeId::ROOT).expect("reachable");
    assert_eq!(t.depth(), 0);
    assert!(t.state().get(0).expect("in bounds").is_unknown());
}

#[test]
fn a_move_that_changes_nothing_is_refused() {
    let (rules, mut t) = session();
    t.play(&rules, paint(2, 5)).expect("legal");
    // Painting the same value again would add a node with an empty delta, which
    // would make the undo count disagree with what the player actually did.
    assert_eq!(
        t.play(&rules, paint(2, 5)),
        Err(TimelineError::Illegal(Illegal::NoOp))
    );
    assert_eq!(t.depth(), 1);
}

#[test]
fn an_action_outside_the_board_is_refused() {
    let (rules, mut t) = session();
    assert_eq!(
        t.play(&rules, paint(99, 1)),
        Err(TimelineError::Illegal(Illegal::OutOfBounds))
    );
    assert_eq!(t.depth(), 0);
}

#[test]
fn a_long_session_undoes_all_the_way_back_to_the_initial_state() {
    let (rules, mut t) = session();
    let initial = t.state().clone();
    let initial_hash = t.hash();
    for i in 0..9u32 {
        t.play(&rules, paint(i, i16::try_from(i).expect("small") + 1))
            .expect("legal");
    }
    assert!(rules_solved(&rules, &t));
    for _ in 0..9 {
        t.undo().expect("undo");
    }
    assert_eq!(*t.state(), initial);
    assert_eq!(t.hash(), initial_hash);
}

fn rules_solved(rules: &PaintRules, t: &Timeline<PaintRules>) -> bool {
    use gs_core::ruleset::Ruleset;
    rules.is_solved(t.state())
}
