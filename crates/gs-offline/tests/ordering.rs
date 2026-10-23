//! The shuffled order, and the one number that stands in for a list of everything
//! played.

use gs_offline::{Bookmark, Order};

const A_PLAYER: u64 = 0x5EED_0001;
const ANOTHER_PLAYER: u64 = 0x5EED_0002;

/// Walk a whole order into a list.
fn sequence(order: &Order) -> Vec<u32> {
    (0..order.len())
        .map(|i| order.at(i).expect("in range"))
        .collect()
}

// ---------------------------------------------------------------------------
// What an order is
// ---------------------------------------------------------------------------

#[test]
fn an_order_contains_every_entry_exactly_once() {
    // A shuffle that dropped or repeated an entry would hand a player the same puzzle
    // twice, or never hand them one at all, and neither would be visible in a bookmark.
    let order = Order::for_player(A_PLAYER, 100);
    let mut seen = sequence(&order);
    assert_eq!(seen.len(), 100);
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen, (0..100).collect::<Vec<u32>>());
}

#[test]
fn the_same_player_always_gets_the_same_order() {
    // The reason the order need not be stored anywhere to be agreed on.
    let once = Order::for_player(A_PLAYER, 50);
    let again = Order::for_player(A_PLAYER, 50);
    assert_eq!(once, again);
}

#[test]
fn two_players_get_different_orders() {
    // Not guaranteed by anything but the shuffle; stated as a test because a bug that
    // ignored the seed would give every player the same order, and nothing else here
    // would notice.
    let mine = sequence(&Order::for_player(A_PLAYER, 50));
    let theirs = sequence(&Order::for_player(ANOTHER_PLAYER, 50));
    assert_ne!(mine, theirs);
}

#[test]
fn an_order_is_actually_shuffled() {
    // The anti-vacuity guard for everything above: an `Order` that returned 0, 1, 2…
    // would satisfy every other test in this file.
    let order = Order::for_player(A_PLAYER, 200);
    let in_order: Vec<u32> = (0..200).collect();
    assert_ne!(sequence(&order), in_order);

    // And not merely reversed, or rotated, which a bad shuffle can produce.
    let reversed: Vec<u32> = (0..200).rev().collect();
    assert_ne!(sequence(&order), reversed);
}

#[test]
fn an_empty_pool_is_an_empty_order() {
    let order = Order::for_player(A_PLAYER, 0);
    assert!(order.is_empty());
    assert_eq!(order.at(0), None);
}

#[test]
fn a_pool_of_one_is_an_order_of_one() {
    let order = Order::for_player(A_PLAYER, 1);
    assert_eq!(sequence(&order), vec![0]);
}

// ---------------------------------------------------------------------------
// The promise: what is behind the bookmark never moves
// ---------------------------------------------------------------------------

#[test]
fn growing_the_pool_never_moves_anything_already_in_the_order() {
    // The promise the whole design rests on. If the order could be reshuffled, a
    // bookmark part way along would point at a different puzzle tomorrow than it did
    // today, and the player would be handed something they had already played.
    let mut order = Order::for_player(A_PLAYER, 20);
    let before = sequence(&order);

    order.grow_to(35);
    let after = sequence(&order);

    assert_eq!(after.len(), 35);
    assert_eq!(&after[..20], &before[..], "the first twenty are untouched");
}

#[test]
fn growing_twice_keeps_both_earlier_blocks_in_place() {
    let mut order = Order::for_player(A_PLAYER, 10);
    let first = sequence(&order);
    order.grow_to(20);
    let second = sequence(&order);
    order.grow_to(30);
    let third = sequence(&order);

    assert_eq!(&third[..10], &first[..]);
    assert_eq!(&third[..20], &second[..]);
    let mut all = third.clone();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 30, "still every entry exactly once");
}

#[test]
fn the_new_entries_are_shuffled_too_rather_than_appended_in_order() {
    // Otherwise a player who caught up with the pool would get everything new in
    // publication order from then on, which is the opposite of the point.
    let mut order = Order::for_player(A_PLAYER, 5);
    order.grow_to(105);
    let tail: Vec<u32> = sequence(&order)[5..].to_vec();
    assert_ne!(tail, (5..105).collect::<Vec<u32>>());
}

#[test]
fn two_blocks_of_the_same_size_are_not_shuffled_the_same_way() {
    // A shuffle seeded only from the player would give the second block the same
    // arrangement as the first, which is a pattern a player could notice.
    let mut order = Order::for_player(A_PLAYER, 0);
    order.grow_to(40);
    let first: Vec<u32> = sequence(&order)[..40].to_vec();
    order.grow_to(80);
    let second: Vec<u32> = sequence(&order)[40..].to_vec();

    let first_pattern: Vec<u32> = first.iter().map(|n| n % 40).collect();
    let second_pattern: Vec<u32> = second.iter().map(|n| n % 40).collect();
    assert_ne!(first_pattern, second_pattern);
}

#[test]
fn a_pool_that_appears_to_shrink_is_left_alone() {
    // A pool does not shrink. If one ever seemed to, dropping entries a bookmark might
    // already be past is the worst available response, so this does nothing instead.
    let mut order = Order::for_player(A_PLAYER, 30);
    let before = sequence(&order);
    order.grow_to(10);
    assert_eq!(sequence(&order), before);
    order.grow_to(30);
    assert_eq!(sequence(&order), before);
}

// ---------------------------------------------------------------------------
// The bookmark
// ---------------------------------------------------------------------------

#[test]
fn a_new_bookmark_points_at_the_first_puzzle_in_the_players_order() {
    let order = Order::for_player(A_PLAYER, 10);
    let bookmark = Bookmark::new();

    assert_eq!(bookmark.position(), 0);
    assert_eq!(bookmark.next_in(&order), order.at(0));
    assert_eq!(bookmark.remaining_in(&order), 10);
}

#[test]
fn a_bookmark_walks_the_order_once_and_in_order() {
    let order = Order::for_player(A_PLAYER, 25);
    let mut bookmark = Bookmark::new();
    let mut played = Vec::new();

    while let Some(entry) = bookmark.next_in(&order) {
        played.push(entry);
        bookmark.played(&order);
    }

    assert_eq!(played, sequence(&order));
    assert_eq!(bookmark.remaining_in(&order), 0);
}

#[test]
fn a_bookmark_at_the_end_asks_for_nothing_and_stays_there() {
    // Walking off the end would claim a player had finished puzzles that do not exist,
    // and the next time the pool grew they would be handed whatever landed there.
    let order = Order::for_player(A_PLAYER, 3);
    let mut bookmark = Bookmark::at(3);

    assert_eq!(bookmark.next_in(&order), None);
    bookmark.played(&order);
    bookmark.played(&order);
    assert_eq!(bookmark.position(), 3, "it does not creep past the end");
}

#[test]
fn a_bookmark_read_back_from_beyond_the_end_asks_for_nothing() {
    // A bookmark and its order are stored separately and can be read back in either
    // sequence, so a bookmark can legitimately arrive ahead of the order it belongs to.
    let order = Order::for_player(A_PLAYER, 3);
    let bookmark = Bookmark::at(9_999);
    assert_eq!(bookmark.next_in(&order), None);
    assert_eq!(bookmark.remaining_in(&order), 0, "and never a huge number");
}

#[test]
fn a_bookmark_part_way_along_picks_up_where_it_left_off_after_the_pool_grows() {
    // The two halves of this design working together: the bookmark is one number, the
    // pool grew underneath it, and the player is handed the puzzle they were going to
    // be handed anyway.
    let mut order = Order::for_player(A_PLAYER, 20);
    let mut bookmark = Bookmark::new();
    for _ in 0..7 {
        bookmark.played(&order);
    }
    let was_next = bookmark.next_in(&order).expect("more to play");

    order.grow_to(60);

    assert_eq!(bookmark.position(), 7, "the number did not change");
    assert_eq!(
        bookmark.next_in(&order),
        Some(was_next),
        "and it still points at the same puzzle"
    );
    assert_eq!(bookmark.remaining_in(&order), 53);
}

#[test]
fn nothing_already_played_is_ever_handed_out_again_after_the_pool_grows() {
    // The same claim stated over the whole run rather than one position: play half an
    // order, grow the pool twice, finish it, and check no entry came up twice.
    let mut order = Order::for_player(A_PLAYER, 30);
    let mut bookmark = Bookmark::new();
    let mut played = Vec::new();

    for _ in 0..15 {
        played.push(bookmark.next_in(&order).expect("more"));
        bookmark.played(&order);
    }
    order.grow_to(45);
    for _ in 0..10 {
        played.push(bookmark.next_in(&order).expect("more"));
        bookmark.played(&order);
    }
    order.grow_to(60);
    while let Some(entry) = bookmark.next_in(&order) {
        played.push(entry);
        bookmark.played(&order);
    }

    assert_eq!(played.len(), 60);
    let mut sorted = played.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 60, "no entry was handed out twice");
}

#[test]
fn the_bookmark_is_one_number_and_that_is_the_whole_record() {
    // Stated so that a change making it anything more is a change somebody argued for.
    // One number per level instead of one row per puzzle played is the whole reason
    // this type exists rather than a set of ids.
    assert_eq!(
        std::mem::size_of::<Bookmark>(),
        std::mem::size_of::<u32>(),
        "a bookmark is one number"
    );
}
