use std::collections::BTreeMap;

pub const MAX_WARPS: usize = 256;
pub const MAX_PENDING: usize = 128;
pub const MAX_COOLDOWNS: usize = 2048;
pub const PAGE_SIZE: usize = 45;
pub const BATCH: usize = 8;
pub type Id = (u64, u64);
pub type Warps = BTreeMap<String, Location>;

#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub world: String,
    pub pos: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
}

impl Location {
    pub fn valid(&self) -> bool {
        !self.world.is_empty()
            && self.world.len() <= 128
            && !self.world.chars().any(char::is_control)
            && self.pos.0.is_finite()
            && self.pos.0.abs() < 30_000_000.0
            && self.pos.2.is_finite()
            && self.pos.2.abs() < 30_000_000.0
            && self.pos.1.is_finite()
            && (-2048.0..=2048.0).contains(&self.pos.1)
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && (-90.0..=90.0).contains(&self.pitch)
    }
}

pub fn name(input: &str) -> Option<String> {
    (!input.is_empty()
        && input.len() <= 32
        && input
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
    .then(|| input.to_ascii_lowercase())
}

pub fn moved(start: &Location, world: &str, pos: (f64, f64, f64)) -> bool {
    start.world != world
        || !pos.0.is_finite()
        || !pos.1.is_finite()
        || !pos.2.is_finite()
        || (start.pos.0 - pos.0).abs() > 0.01
        || (start.pos.1 - pos.1).abs() > 0.01
        || (start.pos.2 - pos.2).abs() > 0.01
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub delay_seconds: u64,
    pub cooldown_seconds: u64,
    pub max_warps: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            delay_seconds: 3,
            cooldown_seconds: 10,
            max_warps: 128,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, &'static str> {
        if text.len() > 4096 {
            return Err("config_error");
        }
        let mut config = Self::default();
        let mut seen = std::collections::BTreeSet::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
        {
            let (key, value) = line.split_once('=').ok_or("config_error")?;
            let key = key.trim();
            if !seen.insert(key) {
                return Err("config_error");
            }
            let value: u64 = value.trim().parse().map_err(|_| "config_error")?;
            match key {
                "delay_seconds" if value <= 60 => config.delay_seconds = value,
                "cooldown_seconds" if value <= 3600 => config.cooldown_seconds = value,
                "max_warps" if (1..=MAX_WARPS as u64).contains(&value) => {
                    config.max_warps = value as usize
                }
                _ => return Err("config_error"),
            }
        }
        Ok(config)
    }
}

#[derive(Clone, Debug)]
pub struct Pending {
    pub warp: String,
    pub target: Location,
    pub start: Location,
    pub entity: i32,
    pub due: u64,
}

impl Pending {
    pub fn revalidate(
        &self,
        current: &Location,
        saved: Option<&Location>,
        permitted: bool,
        alive: bool,
    ) -> Result<(), &'static str> {
        if !permitted {
            return Err("no_permission");
        }
        if !alive {
            return Err("cancelled");
        }
        if saved != Some(&self.target) {
            return Err("warp_changed");
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

    pub fn succeeded(&mut self, id: Id, now: u64, seconds: u64) {
        if seconds != 0
            && (self.cooldowns.contains_key(&id) || self.cooldowns.len() < MAX_COOLDOWNS)
        {
            self.cooldowns
                .insert(id, now.saturating_add(seconds.saturating_mul(1000)));
        }
    }

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

pub fn page(names: &[String], requested: usize) -> (usize, &[String]) {
    let last = names.len().saturating_sub(1) / PAGE_SIZE;
    let number = requested.min(last);
    let start = number * PAGE_SIZE;
    (number, &names[start..(start + PAGE_SIZE).min(names.len())])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn loc() -> Location {
        Location {
            world: "world".into(),
            pos: (1.5, 64.0, -2.5),
            yaw: 20.0,
            pitch: 0.0,
        }
    }

    fn pending(due: u64) -> Pending {
        Pending {
            warp: "spawn".into(),
            target: loc(),
            start: loc(),
            entity: 42,
            due,
        }
    }

    #[test]
    fn names_are_canonical_bounded_and_cannot_inject_permissions() {
        assert_eq!(name("Spawn_1"), Some("spawn_1".into()));
        for bad in ["", "a.b", "*", "../x", "a b", "a\tb", "é", &"a".repeat(33)] {
            assert_eq!(name(bad), None, "{bad}");
        }
    }

    #[test]
    fn persisted_locations_reject_nonfinite_invalid_or_injectable_data() {
        assert!(loc().valid());
        let mut value = loc();
        for invalid in [f64::NAN, f64::INFINITY, 30_000_000.0, -30_000_000.0] {
            value.pos.0 = invalid;
            assert!(!value.valid());
        }
        value = loc();
        value.pitch = 91.0;
        assert!(!value.valid());
        value = loc();
        value.world = "world\tother".into();
        assert!(!value.valid());
        value = loc();
        value.world = "a".repeat(129);
        assert!(!value.valid());
    }

    #[test]
    fn movement_in_any_axis_or_world_cancels_but_rotation_does_not() {
        assert!(!moved(&loc(), "world", loc().pos));
        assert!(moved(&loc(), "world", (1.52, 64.0, -2.5)));
        assert!(moved(&loc(), "world", (1.5, 64.02, -2.5)));
        assert!(moved(&loc(), "world", (1.5, 64.0, -2.52)));
        assert!(moved(&loc(), "other", loc().pos));
        assert!(moved(&loc(), "world", (f64::NAN, 64.0, -2.5)));
    }

    #[test]
    fn config_is_strict_bounded_and_allows_zero_delay() {
        assert_eq!(
            Config::parse("delay_seconds=0\ncooldown_seconds=30\nmax_warps=256\n").unwrap(),
            Config {
                delay_seconds: 0,
                cooldown_seconds: 30,
                max_warps: 256
            }
        );
        for bad in [
            "delay_seconds=61",
            "cooldown_seconds=3601",
            "max_warps=0",
            "max_warps=257",
            "delay_seconds=-1",
            "x=3",
            "delay_seconds=1\ndelay_seconds=2",
        ] {
            assert!(Config::parse(bad).is_err(), "{bad}");
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
    fn execution_rechecks_permission_deletion_changes_death_and_movement() {
        let wait = pending(1000);
        assert_eq!(wait.revalidate(&loc(), Some(&loc()), true, true), Ok(()));
        assert_eq!(
            wait.revalidate(&loc(), Some(&loc()), false, true),
            Err("no_permission")
        );
        assert_eq!(
            wait.revalidate(&loc(), None, true, true),
            Err("warp_changed")
        );
        assert_eq!(
            wait.revalidate(&loc(), Some(&loc()), true, false),
            Err("cancelled")
        );
        let mut other = loc();
        other.pos.1 += 1.0;
        assert_eq!(
            wait.revalidate(&other, Some(&loc()), true, true),
            Err("moved")
        );
        assert_eq!(
            wait.revalidate(&loc(), Some(&other), true, true),
            Err("warp_changed")
        );
    }

    #[test]
    fn cooldown_is_success_only_and_not_removed_on_disconnect() {
        let mut q = Queue::default();
        q.succeeded((1, 1), 1000, 10);
        assert_eq!(q.begin((1, 1), pending(0), 10999), Err("cooldown"));
        q.begin((1, 1), pending(0), 11000).unwrap();
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

    #[test]
    fn menu_pages_are_stable_bounded_and_clamped() {
        let names: Vec<String> = (0..100).map(|n| format!("w{n:03}")).collect();
        assert_eq!(page(&names, 0).1.len(), PAGE_SIZE);
        assert_eq!(page(&names, 2).1.len(), 10);
        assert_eq!(page(&names, usize::MAX).0, 2);
        assert_eq!(page(&[], 1), (0, &[][..]));
    }
}
