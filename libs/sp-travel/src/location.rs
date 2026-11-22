/// A player: the two halves of his UUID.
pub type Id = (u64, u64);

/// A saved place: world, exact position and facing.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub world: String,
    pub pos: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
}

impl Location {
    /// Whether the place can be stored and travelled to: a plain world name, finite numbers inside the world's
    /// bounds. Checked on every location read from a file or taken from a player.
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

/// The stored form of a destination's name: 1 to 32 ASCII letters, digits, `_` or `-`, lowercase. Anything else is
/// no name, so a name can never carry a path, a permission wildcard or a separator of the storage file.
pub fn name(input: &str) -> Option<String> {
    (!input.is_empty()
        && input.len() <= 32
        && input
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
    .then(|| input.to_ascii_lowercase())
}

/// Whether a waiting player left the place he asked from: another world, or more than 0.01 block on any axis.
/// Looking around is not moving.
pub fn moved(start: &Location, world: &str, pos: (f64, f64, f64)) -> bool {
    start.world != world
        || !pos.0.is_finite()
        || !pos.1.is_finite()
        || !pos.2.is_finite()
        || (start.pos.0 - pos.0).abs() > 0.01
        || (start.pos.1 - pos.1).abs() > 0.01
        || (start.pos.2 - pos.2).abs() > 0.01
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
}
