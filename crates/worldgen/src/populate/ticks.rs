//! `WorldServer`'s pending tick list (`pendingTickListEntriesTreeSet` and
//! `pendingTickListEntriesHashSet` over `NextTickListEntry`): what `updateBlockTick` keeps when a
//! tick is not immediate, and what `tickUpdates` takes back out when it is due.
//!
//! Population never holds one, so a tick scheduled there is dropped as before and population stays
//! bit-exact. `sim::terrain::Terrain` lends its list to the world around edits and its own tick.

use std::collections::{BTreeSet, HashSet};

/// A block position, `(x, y, z)`.
pub type At = (i32, i32, i32);

/// `NextTickListEntry` in its `compareTo` order: scheduled time, priority, then the entry id.
type Entry = (i64, i32, u64, At, u16);

/// `WorldServer.tickUpdates` takes at most this many entries a tick.
pub const MAX_PER_TICK: usize = 65536;

#[derive(Default)]
pub struct TickList {
    queue: BTreeSet<Entry>,
    /// `NextTickListEntry.equals`: one entry per position and block.
    pending: HashSet<(At, u16)>,
    /// `NextTickListEntry.nextTickEntryID`
    next_id: u64,
    /// `WorldInfo.getWorldTotalTime`
    pub now: i64,
}

impl TickList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// The pending branch of `WorldServer.updateBlockTick(pos, block, delay, priority)`: the entry
    /// goes in unless the same block is already pending at `at`.
    pub fn schedule(&mut self, at: At, block: u16, delay: i32, priority: i32) {
        if self.pending.insert((at, block)) {
            self.queue.insert((self.now + delay as i64, priority, self.next_id, at, block));
            self.next_id += 1;
        }
    }

    /// `tickUpdates(false)`, the first loop: up to [`MAX_PER_TICK`] entries due by `now`, in
    /// order, out of both sets.
    pub fn take_due(&mut self) -> Vec<(At, u16)> {
        let mut out = Vec::new();
        while out.len() < MAX_PER_TICK {
            let Some(&first) = self.queue.first() else { break };
            if first.0 > self.now {
                break;
            }
            self.queue.remove(&first);
            self.pending.remove(&(first.3, first.4));
            out.push((first.3, first.4));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_entries_come_out_in_order_once() {
        let mut t = TickList::new();
        t.schedule((0, 64, 0), 8, 5, 0);
        t.schedule((0, 64, 0), 8, 1, 0); // already pending: ignored
        t.schedule((1, 64, 0), 10, 30, 0);
        t.schedule((2, 64, 0), 8, 5, 0);
        assert_eq!(t.len(), 3);
        t.now = 4;
        assert!(t.take_due().is_empty());
        t.now = 5;
        assert_eq!(t.take_due(), vec![((0, 64, 0), 8), ((2, 64, 0), 8)]);
        t.schedule((0, 64, 0), 8, 5, 0); // gone from the set, so it can come back
        t.now = 30;
        assert_eq!(t.take_due(), vec![((0, 64, 0), 8), ((1, 64, 0), 10)]);
        assert!(t.is_empty());
    }
}
