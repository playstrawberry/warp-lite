//! What is warp-lite's own: the warp list's bounds, its settings and its menu pages. The location, the wait and
//! the cooldown are `sp-travel`'s, shared with the other teleport scripts.
pub use sp_travel::{Location, files::Locations as Warps, queue::Pending, settings::Field};
// Only the wasm-only runtime reads these; a host build (the unit tests) would flag them unused.
#[cfg(target_arch = "wasm32")]
pub use sp_travel::{Id, name, queue::MAX_PENDING};

pub const MAX_WARPS: usize = 256;
pub const PAGE_SIZE: usize = 45;

/// The settings file's keys, bounds and defaults, in the order of [`Config`]'s fields.
pub const FIELDS: [Field; 3] = [
    Field {
        key: "delay_seconds",
        min: 0,
        max: 60,
        default: 3,
    },
    Field {
        key: "cooldown_seconds",
        min: 0,
        max: 3600,
        default: 10,
    },
    Field {
        key: "max_warps",
        min: 1,
        max: MAX_WARPS as u64,
        default: 128,
    },
];

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
        let values = sp_travel::settings::parse(text, &FIELDS)?;
        let [delay_seconds, cooldown_seconds, max_warps] = values.as_slice() else {
            return Err("config_error");
        };
        Ok(Self {
            delay_seconds: *delay_seconds,
            cooldown_seconds: *cooldown_seconds,
            max_warps: usize::try_from(*max_warps).map_err(|_| "config_error")?,
        })
    }
}

/// The recheck when a wait ends, in this order: the permission, the player's life, the warp as it is saved now,
/// the place he asked from.
pub fn revalidate(
    pending: &Pending,
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
    if saved != Some(&pending.target) {
        return Err("warp_changed");
    }
    pending.still(current, alive)
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
            label: "spawn".into(),
            target: loc(),
            start: loc(),
            entity: 42,
            due,
        }
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
    fn execution_rechecks_permission_deletion_changes_death_and_movement() {
        let wait = pending(1000);
        assert_eq!(revalidate(&wait, &loc(), Some(&loc()), true, true), Ok(()));
        assert_eq!(
            revalidate(&wait, &loc(), Some(&loc()), false, true),
            Err("no_permission")
        );
        assert_eq!(
            revalidate(&wait, &loc(), None, true, true),
            Err("warp_changed")
        );
        assert_eq!(
            revalidate(&wait, &loc(), Some(&loc()), true, false),
            Err("cancelled")
        );
        let mut other = loc();
        other.pos.1 += 1.0;
        assert_eq!(
            revalidate(&wait, &other, Some(&loc()), true, true),
            Err("moved")
        );
        assert_eq!(
            revalidate(&wait, &loc(), Some(&other), true, true),
            Err("warp_changed")
        );
    }

    #[test]
    fn the_default_settings_file_reads_back_as_the_defaults() {
        let text = sp_travel::settings::default_text(&FIELDS);
        assert_eq!(
            text,
            "delay_seconds=3\ncooldown_seconds=10\nmax_warps=128\n"
        );
        assert_eq!(Config::parse(&text).unwrap(), Config::default());
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
