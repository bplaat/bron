/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! How water and lava flow, like in Minecraft. Fluid falls down, and spreads sideways from
//! sources and from flowing fluid that can't fall, losing a level per block. Two water
//! sources fill the gap between them with a new source, and lava that touches water hardens.

use crate::world::{self, Fluid, FluidState};

/// The game ticks between updates of water, which flows faster than lava.
pub const WATER_DELAY: u32 = 5;
/// The game ticks between updates of lava.
pub const LAVA_DELAY: u32 = 30;
// Lava sources harden into gray stone, flowing lava into stone.
const HARDENED_SOURCE: u8 = 38;
const HARDENED_FLOW: u8 = 3;

const SIDES: [[i64; 3]; 4] = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];

const fn offset(position: [i64; 3], [dx, dy, dz]: [i64; 3]) -> [i64; 3] {
    [position[0] + dx, position[1] + dy, position[2] + dz]
}

// Whether fluid can flow into a block of a material, plants wash away.
const fn replaceable(material: u8) -> bool {
    material == 0 || world::is_sprite(material)
}

/// The material a block of fluid, or of air next to fluid, turns into, `None` when it stays
/// or when its neighbors are unknown. `block` gives the materials around it.
pub fn flow(position: [i64; 3], mut block: impl FnMut([i64; 3]) -> Option<u8>) -> Option<u8> {
    let current = block(position)?;
    let state = world::fluid(current);
    let above = block(offset(position, [0, 1, 0]))?;
    let below = block(offset(position, [0, -1, 0]))?;
    let mut sides = [0; 4];
    for (side, direction) in sides.iter_mut().zip(SIDES) {
        *side = block(offset(position, direction))?;
    }

    if let Some(FluidState {
        fluid: Fluid::Lava,
        source,
        ..
    }) = state
        && (world::is_water(above) || sides.iter().any(|&side| world::is_water(side)))
    {
        return Some(if source {
            HARDENED_SOURCE
        } else {
            HARDENED_FLOW
        });
    }
    if state.is_some_and(|state| state.source) || state.is_none() && !replaceable(current) {
        return None;
    }

    let next = if let Some(falling) = world::fluid(above) {
        Some(falling.fluid.falling())
    } else {
        spread(position, &sides, below, &mut block)
    };
    match next {
        Some(material) if material != current => Some(material),
        None if state.is_some() => Some(0),
        _ => None,
    }
}

// The fluid that spreads into a block from its sides, the highest level wins.
fn spread(
    position: [i64; 3],
    sides: &[u8; 4],
    below: u8,
    block: &mut impl FnMut([i64; 3]) -> Option<u8>,
) -> Option<u8> {
    let water_sources = sides.iter().filter(|&&side| side == world::WATER).count();
    let supported = !replaceable(below) && world::fluid(below).is_none_or(|state| state.source);
    if water_sources >= 2 && supported {
        return Some(world::WATER);
    }
    let mut best: Option<(u8, Fluid)> = None;
    for (&side, direction) in sides.iter().zip(SIDES) {
        let Some(state) = world::fluid(side) else {
            continue;
        };
        // Flowing fluid spreads sideways only where it can't fall further.
        if !state.source {
            let under = block(offset(offset(position, direction), [0, -1, 0]));
            let blocked = under.is_none_or(|under| {
                !replaceable(under)
                    && world::fluid(under)
                        .is_none_or(|below| below.source || below.fluid != state.fluid)
            });
            if !blocked {
                continue;
            }
        }
        let level = if state.source || state.falling {
            state.fluid.reach()
        } else {
            state.level - 1
        };
        if level > 0 && best.is_none_or(|(best, _)| level > best) {
            best = Some((level, state.fluid));
        }
    }
    best.map(|(level, fluid)| fluid.flowing(level))
}

/// The game ticks until a block next to the fluid of a material updates.
pub const fn delay(material: u8) -> u32 {
    if world::is_lava(material) {
        LAVA_DELAY
    } else {
        WATER_DELAY
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use super::*;

    // A world of stone below y 10 and air above, with some blocks set.
    struct Test(BTreeMap<[i64; 3], u8>);

    impl Test {
        fn new(blocks: &[([i64; 3], u8)]) -> Self {
            Self(blocks.iter().copied().collect())
        }

        fn block(&self, position: [i64; 3]) -> Option<u8> {
            Some(
                self.0
                    .get(&position)
                    .copied()
                    .unwrap_or(if position[1] < 10 { 3 } else { 0 }),
            )
        }

        // Updates every block until nothing changes, like the server does.
        fn settle(&mut self) {
            for _ in 0..100 {
                let mut changes = alloc::vec::Vec::new();
                for x in -12..=12 {
                    for z in -12..=12 {
                        for y in 5..20 {
                            if let Some(material) = flow([x, y, z], |p| self.block(p)) {
                                changes.push(([x, y, z], material));
                            }
                        }
                    }
                }
                if changes.is_empty() {
                    return;
                }
                self.0.extend(changes);
            }
            panic!("Fluid did not settle");
        }
    }

    #[test]
    fn water_spreads_seven_blocks_and_lava_three() {
        for fluid in [Fluid::Water, Fluid::Lava] {
            let mut world = Test::new(&[([0, 10, 0], fluid.source())]);
            world.settle();
            let reach = i64::from(fluid.reach());
            assert_eq!(world.block([1, 10, 0]), Some(fluid.flowing(fluid.reach())));
            assert_eq!(world.block([reach, 10, 0]), Some(fluid.flowing(1)));
            assert_eq!(world.block([reach + 1, 10, 0]), Some(0));
            assert_eq!(world.block([0, 11, 0]), Some(0));
        }
    }

    #[test]
    fn fluid_falls_before_spreading_and_dries_up_without_a_source() {
        // A source on a pillar above a floor.
        let mut world = Test::new(&[([0, 14, 0], world::WATER), ([0, 13, 0], 3)]);
        world.settle();
        let falling = Fluid::Water.falling();
        assert_eq!(world.block([1, 14, 0]), Some(Fluid::Water.flowing(7)));
        assert_eq!(world.block([1, 13, 0]), Some(falling));
        assert_eq!(world.block([1, 10, 0]), Some(falling));
        assert_eq!(world.block([2, 10, 0]), Some(Fluid::Water.flowing(7)));
        world.0.insert([0, 14, 0], 0);
        world.settle();
        assert!(
            world
                .0
                .values()
                .all(|&material| world::fluid(material).is_none())
        );
    }

    #[test]
    fn two_water_sources_make_a_third_and_lava_hardens_in_water() {
        let mut world = Test::new(&[([0, 10, 0], world::WATER), ([2, 10, 0], world::WATER)]);
        assert_eq!(flow([1, 10, 0], |p| world.block(p)), Some(world::WATER));
        world.0.insert([1, 10, 1], world::LAVA);
        assert_eq!(flow([1, 10, 1], |p| world.block(p)), None);
        world.0.insert([1, 10, 0], world::WATER);
        assert_eq!(flow([1, 10, 1], |p| world.block(p)), Some(HARDENED_SOURCE));
        world.0.insert([5, 10, 5], Fluid::Lava.flowing(2));
        world.0.insert([5, 11, 5], Fluid::Water.flowing(1));
        assert_eq!(flow([5, 10, 5], |p| world.block(p)), Some(HARDENED_FLOW));
        // Solid blocks and unknown neighbors never change.
        assert_eq!(flow([0, 9, 0], |p| world.block(p)), None);
        assert_eq!(flow([3, 10, 0], |_| None), None);
        assert_eq!(delay(world::LAVA), LAVA_DELAY);
        assert_eq!(delay(0), WATER_DELAY);
    }
}
