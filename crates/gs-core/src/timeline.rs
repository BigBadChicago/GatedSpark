//! The immutable timeline: undo, redo, branching, and replay.
//!
//! The architecture plan's requirement that matters most here is the last one:
//! *"new actions after undo create a branch instead of silently corrupting
//! history"*. A timeline that truncates the redo stack on a new move is the usual
//! implementation and it loses work the player did; this one keeps both children and
//! remembers which was taken most recently, so redo still has an obvious meaning.
//!
//! **Deferred, not enforced:** periodic full-state checkpoints, which the plan lists
//! in the authoritative record. They are a storage and seek optimisation, and the
//! save format they belong to is M4. Until then [`Timeline::goto`] walks the tree,
//! which is correct and is what the checkpointed version will be tested against.

use crate::hash::StateHash;
use crate::ruleset::{Illegal, Ruleset, RulesetId};
use crate::state::{Delta, DeltaError, State};

/// A position in the timeline. `NodeId(0)` is the root: the initial state, before
/// any action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    pub const ROOT: NodeId = NodeId(0);
}

#[derive(Debug, Clone)]
struct Node<A> {
    parent: Option<NodeId>,
    /// The action that led here from `parent`, and its effect.
    arrival: Option<Arrival<A>>,
    /// Children in the order they were first created.
    children: Vec<NodeId>,
    /// The child a redo from here should take: the most recently visited one. Keeps
    /// redo predictable after the player explores two branches.
    preferred_child: Option<NodeId>,
    /// The hash of the state at this node, recorded when the node was created.
    hash: StateHash,
    depth: u32,
}

#[derive(Debug, Clone)]
struct Arrival<A> {
    action: A,
    delta: Delta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineError {
    /// There is nothing to undo; the cursor is at the root.
    AtRoot,
    /// There is nothing to redo along any remembered branch.
    NoRedo,
    Illegal(Illegal),
    /// The recorded history no longer matches the state. Never repaired silently.
    Delta(DeltaError),
    /// A replay produced a different state from the one recorded.
    HashMismatch {
        at: NodeId,
        recorded: StateHash,
        recomputed: StateHash,
    },
    /// The record was produced under different semantics.
    RulesetMismatch {
        record: RulesetId,
        engine: RulesetId,
    },
    UnknownNode(NodeId),
}

/// An action log: the authoritative record of a session, in the plan's terms.
///
/// Ordered actions plus the ruleset that interpreted them. No timestamps: nothing
/// that decides what a session *was* may depend on a wall clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionLog<A> {
    pub ruleset: RulesetId,
    pub initial: State,
    pub actions: Vec<A>,
    /// The state hash each action was recorded as producing, in the same order.
    pub hashes: Vec<StateHash>,
}

/// A session: the initial state, the tree of everything done to it, and a cursor.
pub struct Timeline<R: Ruleset> {
    ruleset_id: RulesetId,
    initial: State,
    current: State,
    nodes: Vec<Node<R::Action>>,
    cursor: NodeId,
}

impl<R: Ruleset> core::fmt::Debug for Timeline<R> {
    /// A summary, not the tree. The action type is the family's and need not be
    /// printable, and a full dump of a long session is not what a failing assertion
    /// wants to read.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Timeline")
            .field("ruleset", &self.ruleset_id)
            .field("cursor", &self.cursor)
            .field("depth", &self.depth())
            .field("nodes", &self.nodes.len())
            .field("hash", &self.hash().to_hex())
            .finish()
    }
}

impl<R: Ruleset> Timeline<R> {
    pub fn new(ruleset: &R, initial: State) -> Self {
        let hash = StateHash::of(&initial);
        Timeline {
            ruleset_id: ruleset.id(),
            current: initial.clone(),
            initial,
            nodes: vec![Node {
                parent: None,
                arrival: None,
                children: Vec::new(),
                preferred_child: None,
                hash,
                depth: 0,
            }],
            cursor: NodeId::ROOT,
        }
    }

    #[must_use]
    pub fn state(&self) -> &State {
        &self.current
    }

    #[must_use]
    pub fn cursor(&self) -> NodeId {
        self.cursor
    }

    #[must_use]
    pub fn hash(&self) -> StateHash {
        self.node(self.cursor)
            .expect("cursor is always a real node")
            .hash
    }

    #[must_use]
    pub fn depth(&self) -> u32 {
        self.node(self.cursor)
            .expect("cursor is always a real node")
            .depth
    }

    /// How many distinct branches leave the cursor. One is the ordinary case; more
    /// than one means the player undid and did something else.
    #[must_use]
    pub fn branches_at_cursor(&self) -> usize {
        self.node(self.cursor).map_or(0, |n| n.children.len())
    }

    fn node(&self, id: NodeId) -> Option<&Node<R::Action>> {
        self.nodes.get(id.0 as usize)
    }

    /// Play an action. Creates a new child of the cursor and moves there.
    ///
    /// After an undo this *adds* a branch; it does not discard the one that was
    /// already there.
    pub fn play(&mut self, ruleset: &R, action: R::Action) -> Result<NodeId, TimelineError> {
        let delta = ruleset
            .transition(&self.current, &action)
            .map_err(TimelineError::Illegal)?;
        if delta.is_empty() {
            return Err(TimelineError::Illegal(Illegal::NoOp));
        }
        delta
            .apply(&mut self.current)
            .map_err(TimelineError::Delta)?;

        let id = NodeId(u32::try_from(self.nodes.len()).expect("node count fits in u32"));
        let depth = self.depth() + 1;
        self.nodes.push(Node {
            parent: Some(self.cursor),
            arrival: Some(Arrival { action, delta }),
            children: Vec::new(),
            preferred_child: None,
            hash: StateHash::of(&self.current),
            depth,
        });
        let parent = self.cursor.0 as usize;
        self.nodes[parent].children.push(id);
        self.nodes[parent].preferred_child = Some(id);
        self.cursor = id;
        Ok(id)
    }

    /// Step back one action, by applying the inverse delta.
    pub fn undo(&mut self) -> Result<NodeId, TimelineError> {
        let node = self
            .node(self.cursor)
            .ok_or(TimelineError::UnknownNode(self.cursor))?;
        let parent = node.parent.ok_or(TimelineError::AtRoot)?;
        let inverse = node
            .arrival
            .as_ref()
            .expect("a node with a parent arrived by an action")
            .delta
            .inverse();
        inverse
            .apply(&mut self.current)
            .map_err(TimelineError::Delta)?;
        self.nodes[parent.0 as usize].preferred_child = Some(self.cursor);
        self.cursor = parent;
        self.verify_cursor_hash()
    }

    /// Step forward along the most recently visited branch.
    pub fn redo(&mut self) -> Result<NodeId, TimelineError> {
        let node = self
            .node(self.cursor)
            .ok_or(TimelineError::UnknownNode(self.cursor))?;
        let child = node
            .preferred_child
            .or_else(|| node.children.first().copied())
            .ok_or(TimelineError::NoRedo)?;
        self.step_to_child(child)
    }

    /// Step forward along a named branch, for a player choosing between two futures.
    pub fn redo_into(&mut self, child: NodeId) -> Result<NodeId, TimelineError> {
        let node = self
            .node(self.cursor)
            .ok_or(TimelineError::UnknownNode(self.cursor))?;
        if !node.children.contains(&child) {
            return Err(TimelineError::UnknownNode(child));
        }
        self.step_to_child(child)
    }

    fn step_to_child(&mut self, child: NodeId) -> Result<NodeId, TimelineError> {
        let delta = self
            .node(child)
            .ok_or(TimelineError::UnknownNode(child))?
            .arrival
            .as_ref()
            .expect("a child arrived by an action")
            .delta
            .clone();
        delta
            .apply(&mut self.current)
            .map_err(TimelineError::Delta)?;
        self.nodes[self.cursor.0 as usize].preferred_child = Some(child);
        self.cursor = child;
        self.verify_cursor_hash()
    }

    /// Move to any node, by walking to the common ancestor and forward again.
    ///
    /// Correct but not fast: this is the reference implementation the checkpointed
    /// version will be tested against when the save format lands.
    pub fn goto(&mut self, target: NodeId) -> Result<NodeId, TimelineError> {
        if self.node(target).is_none() {
            return Err(TimelineError::UnknownNode(target));
        }
        let up = self.path_to_root(self.cursor)?;
        let down = self.path_to_root(target)?;
        let common = up
            .iter()
            .find(|id| down.contains(id))
            .copied()
            .unwrap_or(NodeId::ROOT);
        while self.cursor != common {
            self.undo()?;
        }
        let mut forward: Vec<NodeId> = down
            .iter()
            .take_while(|id| **id != common)
            .copied()
            .collect();
        forward.reverse();
        for id in forward {
            self.step_to_child(id)?;
        }
        Ok(self.cursor)
    }

    /// From `id` up to the root, inclusive, nearest first.
    fn path_to_root(&self, id: NodeId) -> Result<Vec<NodeId>, TimelineError> {
        let mut path = vec![id];
        let mut at = id;
        while let Some(parent) = self.node(at).ok_or(TimelineError::UnknownNode(at))?.parent {
            path.push(parent);
            at = parent;
        }
        Ok(path)
    }

    /// Check the live state against the hash recorded for the cursor.
    ///
    /// Called after every move of the cursor. A mismatch means the delta machinery
    /// and the recorded history have diverged, and the only honest response is to say
    /// so rather than carry on with a state nobody recorded.
    fn verify_cursor_hash(&self) -> Result<NodeId, TimelineError> {
        let node = self
            .node(self.cursor)
            .ok_or(TimelineError::UnknownNode(self.cursor))?;
        let recomputed = StateHash::of(&self.current);
        if recomputed != node.hash {
            return Err(TimelineError::HashMismatch {
                at: self.cursor,
                recorded: node.hash,
                recomputed,
            });
        }
        Ok(self.cursor)
    }

    /// The authoritative record of the path from the root to the cursor.
    pub fn action_log(&self) -> Result<ActionLog<R::Action>, TimelineError>
    where
        R::Action: Clone,
    {
        let mut path = self.path_to_root(self.cursor)?;
        path.reverse();
        let mut actions = Vec::new();
        let mut hashes = Vec::new();
        for id in path.iter().skip(1) {
            let node = self.node(*id).ok_or(TimelineError::UnknownNode(*id))?;
            let arrival = node
                .arrival
                .as_ref()
                .expect("a non-root node arrived by an action");
            actions.push(arrival.action.clone());
            hashes.push(node.hash);
        }
        Ok(ActionLog {
            ruleset: self.ruleset_id,
            initial: self.initial.clone(),
            actions,
            hashes,
        })
    }
}

/// Replay a record through the real engine.
///
/// This is the architecture plan's "runtime consistency" check and the thing that
/// makes a solver witness worth anything: the solver's answer is replayed through the
/// same code a player's moves go through, and the hashes must match at every step.
///
/// It refuses a record produced under different semantics rather than attempting it,
/// because a replay that silently reinterprets old actions is worse than no replay.
pub fn replay<R: Ruleset>(
    ruleset: &R,
    log: &ActionLog<R::Action>,
) -> Result<Timeline<R>, TimelineError> {
    if log.ruleset != ruleset.id() {
        return Err(TimelineError::RulesetMismatch {
            record: log.ruleset,
            engine: ruleset.id(),
        });
    }
    let mut timeline = Timeline::new(ruleset, log.initial.clone());
    for (i, action) in log.actions.iter().enumerate() {
        let id = timeline.play(ruleset, action.clone())?;
        if let Some(recorded) = log.hashes.get(i) {
            let recomputed = timeline.hash();
            if recomputed != *recorded {
                return Err(TimelineError::HashMismatch {
                    at: id,
                    recorded: *recorded,
                    recomputed,
                });
            }
        }
    }
    Ok(timeline)
}
