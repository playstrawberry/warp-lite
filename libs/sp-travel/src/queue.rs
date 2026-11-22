//! The wait before a travel and the cooldown after an arrival. Plain data with hard caps; the timer is `runtime`'s.
use crate::{Id, Location, moved};
use std::collections::BTreeMap;

/// Players that may wait at once; one more is told the script is busy.
pub const MAX_PENDING: usize = 128;
/// Cooldowns kept at once.
pub const MAX_COOLDOWNS: usize = 2048;
/// Travels completed in one timer call; the rest follow in the next.
pub const BATCH: usize = 8;

/// One accepted request that has not travelled yet.
#[derive(Clone, Debug)]
pub struct Pending {
    /// What the destination is called in the messages (a warp's or a home's name).
    pub label: String,
    pub target: Location,
    /// Where the player stood when he asked.
    pub start: Location,
    /// The player's entity id, to match a damage event.
    pub entity: i32,
    /// When the wait ends, in milliseconds of the script's clock.
    pub due: u64,
}

impl Pending {
    /// The checks every travel repeats when its wait ends: the player is alive and still where he asked from.
    pub fn still(&self, current: &Location, alive: bool) -> Result<(), &'static str> {
        if !alive {
            return Err("cancelled");
        }
        if moved(&self.start, &current.world, current.pos) {
            return Err("moved");
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Queue {
    pub pending: BTreeMap<Id, Pending>,
    pub cooldowns: BTreeMap<Id, u64>,
}

impl Queue {
    /// Accepts a request, unless the player already waits, is cooling down, or the caps are reached. A second
    /// request never replaces the first one's destination or deadline.
    pub fn begin(&mut self, id: Id, pending: Pending, now: u64) -> Result<(), &'static str> {
        self.cooldowns.retain(|_, until| *until > now);
        if self.pending.contains_key(&id) {
            return Err("already_waiting");
        }
        if self.cooldowns.contains_key(&id) {
            return Err("cooldown");
        }
        if self.pending.len() >= MAX_PENDING
            || self.cooldowns.len() + self.pending.len() >= MAX_COOLDOWNS
        {
            return Err("busy");
        }
        self.pending.insert(id, pending);
        Ok(())
    }

    /// Takes out up to [`BATCH`] requests whose wait is over.
    pub fn due(&mut self, now: u64) -> Vec<(Id, Pending)> {
        let ids: Vec<Id> = self
            .pending
            .iter()
            .filter(|(_, p)| p.due <= now)
            .take(BATCH)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.pending.remove(&id).map(|p| (id, p)))
            .collect()
    }

    /// Starts the cooldown of a player who arrived. A refused or cancelled request never starts one.
    pub fn succeeded(&mut self, id: Id, now: u64, seconds: u64) {
        if seconds != 0
            && (self.cooldowns.contains_key(&id) || self.cooldowns.len() < MAX_COOLDOWNS)
        {
            self.cooldowns
                .insert(id, now.saturating_add(seconds.saturating_mul(1000)));
        }
    }

    /// Drops the request of the player with this entity id (he was hurt) and names him.
    pub fn cancel_entity(&mut self, entity: i32) -> Option<Id> {
        let id = self
            .pending
            .iter()
            .find(|(_, p)| p.entity == entity)
            .map(|(id, _)| *id)?;
        self.pending.remove(&id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::tests::loc;

    fn pending(due: u64) -> Pending {
        Pending {
            label: "spawn".into(),
            target: loc(),
            start: loc(),
            entity: 42,
            due,
        }
    }

    #[test]
    fn duplicate_requests_cannot_replace_the_origin_or_deadline() {
        let mut q = Queue::default();
        q.begin((1, 1), pending(3000), 0).unwrap();
        assert_eq!(q.begin((1, 1), pending(0), 1), Err("already_waiting"));
        assert!(q.due(2999).is_empty());
        assert_eq!(q.due(3000).len(), 1);
        assert!(q.due(3000).is_empty());
    }

    #[test]
    fn cancelling_drops_the_request_without_starting_a_cooldown() {
        let mut q = Queue::default();
        q.begin((1, 1), pending(0), 0).unwrap();
        q.pending.remove(&(1, 1));
        assert!(q.due(0).is_empty());
        assert!(q.cooldowns.is_empty());
    }

    #[test]
    fn damage_only_cancels_the_matching_player_once() {
        let mut q = Queue::default();
        q.begin((1, 1), pending(1000), 0).unwrap();
        assert_eq!(q.cancel_entity(99), None);
        assert_eq!(q.cancel_entity(42), Some((1, 1)));
        assert_eq!(q.cancel_entity(42), None);
        assert!(q.cooldowns.is_empty());
    }

    #[test]
    fn the_end_of_a_wait_rechecks_death_and_movement() {
        let wait = pending(1000);
        assert_eq!(wait.still(&loc(), true), Ok(()));
        assert_eq!(wait.still(&loc(), false), Err("cancelled"));
        let mut other = loc();
        other.pos.1 += 1.0;
        assert_eq!(wait.still(&other, true), Err("moved"));
        other = loc();
        other.world = "nether".into();
        assert_eq!(wait.still(&other, true), Err("moved"));
    }

    #[test]
    fn cooldown_is_success_only_and_not_removed_on_disconnect() {
        let mut q = Queue::default();
        q.succeeded((1, 1), 1000, 10);
        assert_eq!(q.begin((1, 1), pending(0), 10999), Err("cooldown"));
        q.begin((1, 1), pending(0), 11000).unwrap();
        q.succeeded((2, 2), 0, 0);
        assert!(!q.cooldowns.contains_key(&(2, 2)), "no cooldown configured");
    }

    #[test]
    fn work_and_retained_state_have_hard_caps() {
        let mut q = Queue::default();
        for n in 0..MAX_PENDING {
            q.begin((0, n as u64), pending(0), 0).unwrap();
        }
        assert_eq!(q.begin((0, MAX_PENDING as u64), pending(0), 0), Err("busy"));
        assert_eq!(q.due(0).len(), BATCH);
        q.pending.clear();
        for n in 0..MAX_COOLDOWNS {
            q.succeeded((0, n as u64), 0, 10);
        }
        assert_eq!(q.begin((1, 0), pending(0), 1), Err("busy"));
        q.begin((1, 0), pending(0), 10_000).unwrap();
        assert!(q.cooldowns.is_empty());
    }
}
