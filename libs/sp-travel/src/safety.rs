//! One teleport boundary: look at a loaded destination, then make the one teleport call and check the arrival.
//!
//! The look reads the feet block, the head block and the floor of the destination column, only when its chunk is
//! loaded: feet and head free of collision, liquid and hazard, the floor a full solid block. It never loads a chunk.
//! A script whose destinations are a player's own (a home on a slab, the place he died) turns the look off; the
//! arrival is checked either way.
use crate::Location;

#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub passable: bool,
    pub floor: bool,
}

pub trait Destination {
    fn loaded(&mut self, chunk: (i32, i32)) -> bool;
    fn block(&mut self, position: (i32, i32, i32)) -> Block;
    fn apply(&mut self, target: &Location) -> Result<(), &'static str>;
    fn current(&mut self) -> Location;
}

#[cfg(any(test, target_arch = "wasm32"))]
fn hazardous(name: &str) -> bool {
    matches!(
        name.strip_prefix("minecraft:").unwrap_or(name),
        "lava"
            | "fire"
            | "soul_fire"
            | "cactus"
            | "magma_block"
            | "powder_snow"
            | "campfire"
            | "soul_campfire"
            | "sweet_berry_bush"
            | "wither_rose"
            | "pointed_dripstone"
            | "nether_portal"
            | "end_portal"
            | "end_gateway"
    )
}

/// Travels to `target`; with `look` a loaded destination is inspected first and refused when it is not safe.
pub fn travel(
    host: &mut impl Destination,
    target: &Location,
    look: bool,
) -> Result<(), &'static str> {
    if !target.valid() {
        return Err("invalid_location");
    }
    let (x, y, z) = (
        target.pos.0.floor() as i32,
        target.pos.1.floor() as i32,
        target.pos.2.floor() as i32,
    );
    if look && host.loaded((x.div_euclid(16), z.div_euclid(16))) {
        // A full-block floor cannot support a saved position floating above its top.
        if target.pos.1 - target.pos.1.floor() > 0.001 {
            return Err("unsafe_target");
        }
        let feet = host.block((x, y, z));
        let head = host.block((x, y + 1, z));
        let floor = host.block((x, y - 1, z));
        if !feet.passable || !head.passable || !floor.floor {
            return Err("unsafe_target");
        }
    }
    host.apply(target)?;
    let current = host.current();
    if current.world != target.world
        || !current.valid()
        || (current.pos.0 - target.pos.0).abs() > 0.05
        || (current.pos.1 - target.pos.1).abs() > 0.05
        || (current.pos.2 - target.pos.2).abs() > 0.05
    {
        return Err("teleport_failed");
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn teleport(
    server: &strawberry_plugin_api::Server,
    player: &strawberry_plugin_api::Player,
    target: &Location,
    look: bool,
) -> Result<(), &'static str> {
    use strawberry_plugin_api::{Player, common::BlockPos, world::World};

    struct Live<'a> {
        server: &'a strawberry_plugin_api::Server,
        player: &'a Player,
        world: World,
    }
    impl Destination for Live<'_> {
        fn loaded(&mut self, chunk: (i32, i32)) -> bool {
            self.world.get_chunk(chunk.0, chunk.1).is_some()
        }
        fn block(&mut self, position: (i32, i32, i32)) -> Block {
            let state = self.world.get_block_state(BlockPos {
                x: position.0,
                y: position.1,
                z: position.2,
            });
            let safe = !state.is_liquid && !hazardous(&state.block_name);
            Block {
                passable: safe && state.collision_shapes.is_empty(),
                floor: safe && state.is_solid && state.is_full_cube,
            }
        }
        fn apply(&mut self, target: &Location) -> Result<(), &'static str> {
            let world = self
                .server
                .get_world_by_name(&target.world)
                .ok_or("world_unavailable")?;
            self.player
                .teleport(target.pos, Some(target.yaw), Some(target.pitch), world);
            Ok(())
        }
        fn current(&mut self) -> Location {
            Location {
                world: self.player.get_world().get_name(),
                pos: self.player.get_position(),
                yaw: self.player.get_yaw(),
                pitch: self.player.get_pitch(),
            }
        }
    }
    let world = server
        .get_world_by_name(&target.world)
        .ok_or("world_unavailable")?;
    let mut live = Live {
        server,
        player,
        world,
    };
    travel(&mut live, target, look)?;
    player.set_velocity((0.0, 0.0, 0.0));
    player.as_entity().set_fall_distance(0.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::tests::loc;

    const AIR: Block = Block {
        passable: true,
        floor: false,
    };
    const STONE: Block = Block {
        passable: false,
        floor: true,
    };

    struct Fake {
        loaded: bool,
        blocks: [Block; 3],
        reads: Vec<(i32, i32, i32)>,
        chunks: Vec<(i32, i32)>,
        calls: usize,
        cancelled: bool,
        current: Location,
    }

    impl Fake {
        fn new(loaded: bool) -> Self {
            let mut current = loc();
            current.pos.0 += 10.0;
            Self {
                loaded,
                blocks: [AIR, AIR, STONE],
                reads: vec![],
                chunks: vec![],
                calls: 0,
                cancelled: false,
                current,
            }
        }
    }

    impl Destination for Fake {
        fn loaded(&mut self, chunk: (i32, i32)) -> bool {
            self.chunks.push(chunk);
            self.loaded
        }
        fn block(&mut self, pos: (i32, i32, i32)) -> Block {
            let index = match pos.1 {
                64 => 0,
                65 => 1,
                63 => 2,
                _ => panic!("unexpected block"),
            };
            self.reads.push(pos);
            self.blocks[index]
        }
        fn apply(&mut self, target: &Location) -> Result<(), &'static str> {
            self.calls += 1;
            if !self.cancelled {
                self.current = target.clone();
            }
            Ok(())
        }
        fn current(&mut self) -> Location {
            self.current.clone()
        }
    }

    #[test]
    fn loaded_safe_column_reads_feet_head_floor_and_teleports_once() {
        let mut host = Fake::new(true);
        assert_eq!(travel(&mut host, &loc(), true), Ok(()));
        assert_eq!(host.reads, [(1, 64, -3), (1, 65, -3), (1, 63, -3)]);
        assert_eq!(host.chunks, [(0, -1)]);
        assert_eq!(host.calls, 1);
        assert_eq!(host.current, loc());
    }

    #[test]
    fn unsafe_loaded_feet_head_and_floor_never_teleport() {
        for index in 0..3 {
            let mut host = Fake::new(true);
            host.blocks[index] = if index == 2 { AIR } else { STONE };
            assert_eq!(travel(&mut host, &loc(), true), Err("unsafe_target"));
            assert_eq!(host.calls, 0);
        }
    }

    #[test]
    fn unloaded_destination_skips_all_block_reads_and_still_teleports() {
        let mut host = Fake::new(false);
        host.blocks = [STONE, STONE, AIR];
        assert_eq!(travel(&mut host, &loc(), true), Ok(()));
        assert!(host.reads.is_empty());
        assert_eq!(host.calls, 1);
    }

    #[test]
    fn without_the_look_a_loaded_destination_is_not_read_and_the_arrival_is_still_checked() {
        let mut host = Fake::new(true);
        host.blocks = [STONE, STONE, AIR];
        assert_eq!(travel(&mut host, &loc(), false), Ok(()));
        assert!(host.reads.is_empty() && host.chunks.is_empty());
        assert_eq!(host.calls, 1);
        let mut host = Fake::new(true);
        host.cancelled = true;
        assert_eq!(travel(&mut host, &loc(), false), Err("teleport_failed"));
    }

    #[test]
    fn rejected_or_redirected_teleport_is_not_reported_as_arrival() {
        let mut host = Fake::new(true);
        host.cancelled = true;
        assert_eq!(travel(&mut host, &loc(), true), Err("teleport_failed"));
        assert_eq!(host.calls, 1);
    }

    #[test]
    fn a_different_world_is_not_a_success_even_at_identical_coordinates() {
        let mut host = Fake::new(false);
        host.cancelled = true;
        host.current = loc();
        host.current.world = "other".into();
        assert_eq!(travel(&mut host, &loc(), true), Err("teleport_failed"));
    }

    #[test]
    fn loaded_midair_target_is_rejected_but_unloaded_staff_position_is_used() {
        let mut target = loc();
        target.pos.1 += 0.5;
        let mut host = Fake::new(true);
        assert_eq!(travel(&mut host, &target, true), Err("unsafe_target"));
        assert_eq!(host.calls, 0);
        let mut host = Fake::new(false);
        assert_eq!(travel(&mut host, &target, true), Ok(()));
    }

    #[test]
    fn invalid_destination_does_not_cross_the_host_boundary() {
        let mut host = Fake::new(false);
        let mut target = loc();
        target.pos.0 = f64::NAN;
        assert_eq!(travel(&mut host, &target, true), Err("invalid_location"));
        assert!(host.chunks.is_empty());
        assert_eq!(host.calls, 0);
    }

    #[test]
    fn hazardous_blocks_are_rejected_with_or_without_a_namespace() {
        for name in [
            "lava",
            "fire",
            "soul_fire",
            "cactus",
            "magma_block",
            "powder_snow",
            "campfire",
            "soul_campfire",
            "sweet_berry_bush",
            "wither_rose",
            "pointed_dripstone",
            "nether_portal",
            "end_portal",
            "end_gateway",
        ] {
            assert!(hazardous(name), "{name}");
            assert!(hazardous(&format!("minecraft:{name}")), "{name}");
        }
        assert!(!hazardous("minecraft:stone"));
        assert!(!hazardous("minecraft:air"));
    }
}
