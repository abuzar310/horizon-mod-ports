//! Block Outline: the outline of the block you are looking at, drawn in your own colour, opacity
//! and thickness in place of the game's thin black one.
//!
//! The game asks `block_highlight` before it builds its own outline, only after its own gates
//! (the HUD shown, a game mode that may build, no entity targeted), so the outline shows exactly
//! when the game's would. The mod answers with the block's outline (`state_outline`, the same
//! boxes the game outlines) and cancels the game's; the outline is drawn a moment later in the
//! same frame, at the first world stage after the one where the game draws its own.
//!
//! A block with no collision boxes (flowers, torches, grass) keeps the game's outline: many of
//! them are moved a little at random inside their block, which the outline read does not include.
//! So does a block whose collision sits off its outline's centre across the ground, in case the
//! collision read carries such a move. A solid block moved at random (bamboo, pointed dripstone)
//! whose collision read does not carry it is outlined where it would be unmoved.
use std::sync::Mutex;

use mod_api::{
    BlockHighlightEvent, ClientHost, Flow, Hooks, Registrar, RegistrationContext, RenderLevelStage,
    RenderLevelStageEvent, SettingSpec, WorldDepth, WorldLine,
};

/// The mod's id: its settings folder and translation keys.
pub const ID: &str = "block_outline";
/// The colour before the player changes it, `0xRRGGBB`: cyan.
pub const DEFAULT_COLOR: u32 = 0x00_FFFF;
/// The opacity before the player changes it, and its range.
pub const DEFAULT_ALPHA: f64 = 0.8;
pub const MIN_ALPHA: f64 = 0.1;
pub const MAX_ALPHA: f64 = 1.0;
/// The line width before the player changes it, and its range, in screen pixels.
pub const DEFAULT_THICKNESS: f64 = 2.0;
pub const MIN_THICKNESS: f64 = 1.0;
pub const MAX_THICKNESS: f64 = 4.0;
/// How far each box grows on every side, in blocks, so its edges sit just off the block's faces
/// and don't flicker against them.
pub const GROW: f64 = 0.002;
/// The most boxes the mod outlines; a busier shape keeps the game's outline.
pub const MAX_BOXES: usize = 32;
/// The lines of [`MAX_BOXES`] boxes: the line buffer, reserved once, never grows.
pub const MAX_LINES: usize = MAX_BOXES * 12;
/// How far apart two centres can be and still be the same, in blocks.
pub const SAME_CENTRE: f64 = 1.0e-4;
/// Where the outline is drawn: the game draws its own after entities and before translucent
/// terrain (after it for a translucent block), so this is the first stage after it.
pub const STAGE: RenderLevelStage = RenderLevelStage::AfterTranslucentBlocks;

/// The settings on Block Outline's page in Options > Mods.
pub fn settings() -> [SettingSpec; 3] {
    [
        SettingSpec::color("color", "block_outline.setting.color", DEFAULT_COLOR)
            .tooltip("block_outline.setting.color.tooltip"),
        SettingSpec::range(
            "alpha",
            "block_outline.setting.alpha",
            MIN_ALPHA,
            MAX_ALPHA,
            0.05,
            DEFAULT_ALPHA,
        )
        .tooltip("block_outline.setting.alpha.tooltip"),
        SettingSpec::range(
            "thickness",
            "block_outline.setting.thickness",
            MIN_THICKNESS,
            MAX_THICKNESS,
            0.5,
            DEFAULT_THICKNESS,
        )
        .tooltip("block_outline.setting.thickness.tooltip"),
    ]
}

/// A packed `0xAARRGGBB` colour from a `0xRRGGBB` colour (anything above its 24 bits is dropped)
/// and an opacity from 0 to 1 (clamped; not a number counts as 0).
pub fn argb(rgb: u32, alpha: f64) -> u32 {
    let alpha = if alpha.is_nan() {
        0.0
    } else {
        alpha.clamp(0.0, 1.0)
    };
    let a = (alpha * 255.0).round() as u32;
    (a << 24) | (rgb & 0x00FF_FFFF)
}

/// The line width for the thickness setting's value: the default when there is none or it is not
/// a number, kept within the setting's range so the game always takes it.
pub fn width(thickness: Option<f64>) -> f32 {
    thickness
        .filter(|t| t.is_finite())
        .unwrap_or(DEFAULT_THICKNESS)
        .clamp(MIN_THICKNESS, MAX_THICKNESS) as f32
}

/// A box `[min x, y, z, max x, y, z]` grown by `by` on every side.
pub fn grow(b: [f64; 6], by: f64) -> [f64; 6] {
    [
        b[0] - by,
        b[1] - by,
        b[2] - by,
        b[3] + by,
        b[4] + by,
        b[5] + by,
    ]
}

/// A box in the block at `pos`'s own space, moved into the world.
pub fn at(pos: [i32; 3], b: [f64; 6]) -> [f64; 6] {
    let [x, y, z] = pos.map(f64::from);
    [b[0] + x, b[1] + y, b[2] + z, b[3] + x, b[4] + y, b[5] + z]
}

/// The 12 edges of a box, each as its two ends: four along x, four along y, four along z.
pub fn edges(b: [f64; 6]) -> [[[f64; 3]; 2]; 12] {
    let [x0, y0, z0, x1, y1, z1] = b;
    [
        [[x0, y0, z0], [x1, y0, z0]],
        [[x0, y1, z0], [x1, y1, z0]],
        [[x0, y0, z1], [x1, y0, z1]],
        [[x0, y1, z1], [x1, y1, z1]],
        [[x0, y0, z0], [x0, y1, z0]],
        [[x1, y0, z0], [x1, y1, z0]],
        [[x0, y0, z1], [x0, y1, z1]],
        [[x1, y0, z1], [x1, y1, z1]],
        [[x0, y0, z0], [x0, y0, z1]],
        [[x1, y0, z0], [x1, y0, z1]],
        [[x0, y1, z0], [x0, y1, z1]],
        [[x1, y1, z0], [x1, y1, z1]],
    ]
}

/// The box around all of `boxes`; none for no boxes or a coordinate that is not finite.
pub fn bounds(boxes: &[[f64; 6]]) -> Option<[f64; 6]> {
    let first = *boxes.first()?;
    let mut all = first;
    for b in boxes {
        if !b.iter().all(|v| v.is_finite()) {
            return None;
        }
        for axis in 0..3 {
            all[axis] = all[axis].min(b[axis]);
            all[axis + 3] = all[axis + 3].max(b[axis + 3]);
        }
    }
    Some(all)
}

/// Whether the mod draws this block's outline in place of the game's, from the block's outline
/// boxes and its collision boxes (both in its own space). Not for no outline or too many boxes to
/// draw, nor for a block with no collision boxes, nor for one whose collision sits off its
/// outline's centre across the ground: such a collision may carry a random move inside the block
/// that the outline read lacks. A collision box taller or lower than the outline on the same
/// ground (a fence, a snow layer) is fine: only the outline is drawn.
pub fn takes_over(outline: &[[f64; 6]], collision: &[[f64; 6]]) -> bool {
    if outline.len() > MAX_BOXES {
        return false;
    }
    let (Some(shape), Some(solid)) = (bounds(outline), bounds(collision)) else {
        return false;
    };
    let centre = |b: [f64; 6], axis: usize| (b[axis] + b[axis + 3]) / 2.0;
    [0, 2]
        .iter()
        .all(|&axis| (centre(shape, axis) - centre(solid, axis)).abs() <= SAME_CENTRE)
}

/// Adds the lines that outline `boxes` (in the block at `pos`'s own space, each grown by
/// [`GROW`]) to `out`: one line of `width` pixels per edge, tested against the world's depth.
pub fn push_lines(
    out: &mut Vec<WorldLine>,
    pos: [i32; 3],
    boxes: &[[f64; 6]],
    color: u32,
    width: f32,
) {
    for &b in boxes.iter().take(MAX_BOXES) {
        for [from, to] in edges(grow(at(pos, b), GROW)) {
            out.push(WorldLine::new(from, to, color, width, WorldDepth::Tested));
        }
    }
}

/// The block whose outline this frame draws: where it is and its outline boxes in its own space.
struct Target {
    pos: [i32; 3],
    boxes: Vec<[f64; 6]>,
}

struct BlockOutline {
    /// Set by `block_highlight` and taken by the drawing stage of the same frame, so a frame the
    /// game outlines nothing in draws nothing.
    target: Mutex<Option<Target>>,
    /// The lines a frame submits, built into the same buffer every frame.
    lines: Mutex<Vec<WorldLine>>,
}

impl Default for BlockOutline {
    fn default() -> Self {
        Self {
            target: Mutex::new(None),
            lines: Mutex::new(Vec::with_capacity(MAX_LINES)),
        }
    }
}

impl BlockOutline {
    fn target(&self) -> std::sync::MutexGuard<'_, Option<Target>> {
        self.target.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lines(&self) -> std::sync::MutexGuard<'_, Vec<WorldLine>> {
        self.lines.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[mod_api::horizon_mod]
impl Hooks for BlockOutline {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// The game is about to outline the block at `event.pos`: take it over when the mod can draw
    /// it, else leave the game's outline.
    fn block_highlight(&self, host: &dyn ClientHost, event: &mut BlockHighlightEvent) -> Flow {
        let target = if host.on(ID) {
            host.state_outline(event.state)
                .filter(|outline| takes_over(outline, &host.block_collision(event.pos)))
                .map(|boxes| Target {
                    pos: event.pos,
                    boxes,
                })
        } else {
            None
        };
        let flow = if target.is_some() {
            Flow::Cancel
        } else {
            Flow::Continue
        };
        *self.target() = target;
        flow
    }

    /// The outline of the block taken over this frame, in the player's colour and thickness.
    fn render_level_stage(&self, host: &dyn ClientHost, event: &mut RenderLevelStageEvent<'_>) {
        if event.stage != STAGE {
            return;
        }
        let Some(target) = self.target().take() else {
            return;
        };
        if !host.on(ID) {
            return;
        }
        let color = argb(
            host.setting_color("color").unwrap_or(DEFAULT_COLOR),
            host.setting_f64("alpha").unwrap_or(DEFAULT_ALPHA),
        );
        let line_width = width(host.setting_f64("thickness"));
        let mut lines = self.lines();
        lines.clear();
        push_lines(&mut lines, target.pos, &target.boxes, color, line_width);
        // A batch the view has no room left for is refused whole: no outline this frame.
        let _ = event.draw.lines(lines.as_slice());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: [f64; 6] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
    }

    #[test]
    fn argb_packs_the_colour_under_the_opacity() {
        assert_eq!(argb(DEFAULT_COLOR, DEFAULT_ALPHA), 0xCC00_FFFF);
        assert_eq!(argb(0x12_3456, 1.0), 0xFF12_3456);
        assert_eq!(argb(0x12_3456, 0.0), 0x0012_3456);
        assert_eq!(argb(0xFF, 0.5), 0x8000_00FF);
        assert_eq!(argb(0xFF, 0.1), 0x1A00_00FF);
    }

    #[test]
    fn argb_drops_alpha_bits_of_the_colour_and_clamps_the_opacity() {
        assert_eq!(argb(0xAB12_3456, 1.0), 0xFF12_3456);
        assert_eq!(argb(0x12_3456, 2.0), 0xFF12_3456);
        assert_eq!(argb(0x12_3456, -1.0), 0x0012_3456);
        assert_eq!(argb(0x12_3456, f64::NAN), 0x0012_3456);
        assert_eq!(argb(0x12_3456, f64::INFINITY), 0xFF12_3456);
    }

    #[test]
    fn the_width_is_always_one_the_game_takes() {
        assert_eq!(width(None), 2.0);
        assert_eq!(width(Some(3.5)), 3.5);
        assert_eq!(width(Some(0.0)), 1.0);
        assert_eq!(width(Some(9.0)), 4.0);
        assert_eq!(width(Some(f64::NAN)), 2.0);
        assert_eq!(width(Some(f64::INFINITY)), 2.0);
    }

    #[test]
    fn growing_moves_every_side_out() {
        let grown = grow([0.25, 0.0, 0.5, 0.75, 0.5, 1.0], GROW);
        let expected = [0.248, -0.002, 0.498, 0.752, 0.502, 1.002];
        for (got, want) in grown.iter().zip(expected) {
            assert!((got - want).abs() < 1e-12, "{grown:?}");
        }
        // A flat box (a carpet's top, a pressure plate pressed) gets a thickness of its own.
        let flat = grow([0.0, 1.0, 0.0, 1.0, 1.0, 1.0], GROW);
        assert!(flat[4] > flat[1]);
        assert_eq!(grow(FULL, 0.0), FULL);
    }

    #[test]
    fn a_box_is_moved_to_its_block() {
        assert_eq!(
            at([-3, 64, 10], [0.0, 0.0, 0.0, 1.0, 0.5, 1.0]),
            [-3.0, 64.0, 10.0, -2.0, 64.5, 11.0]
        );
    }

    #[test]
    fn a_box_has_twelve_edges_along_the_three_axes() {
        let b = [1.0, 2.0, 3.0, 2.0, 4.0, 6.0];
        let edges = edges(b);
        let mut along = [0; 3];
        for [from, to] in edges {
            let moved: Vec<usize> = (0..3).filter(|&a| from[a] != to[a]).collect();
            assert_eq!(moved.len(), 1, "{from:?} {to:?}");
            let axis = moved[0];
            along[axis] += 1;
            // Each edge runs the box's whole length on its axis, from the low side.
            assert_eq!(from[axis], b[axis]);
            assert_eq!(to[axis], b[axis + 3]);
            // And lies on the box's sides on the other two.
            for other in (0..3).filter(|&a| a != axis) {
                assert!(from[other] == b[other] || from[other] == b[other + 3]);
            }
        }
        assert_eq!(along, [4, 4, 4]);
    }

    #[test]
    fn every_corner_meets_three_edges_and_no_edge_repeats() {
        let edges = edges(FULL);
        for corner in 0..8u32 {
            let point = [0, 1, 2].map(|a| f64::from((corner >> a) & 1));
            let meeting = edges
                .iter()
                .filter(|[from, to]| *from == point || *to == point)
                .count();
            assert_eq!(meeting, 3, "{point:?}");
        }
        for (i, a) in edges.iter().enumerate() {
            for b in &edges[i + 1..] {
                assert_ne!(a, b);
            }
        }
        let length: f64 = edges
            .iter()
            .map(|[from, to]| (0..3).map(|a| (to[a] - from[a]).abs()).sum::<f64>())
            .sum();
        assert_eq!(length, 12.0);
    }

    #[test]
    fn bounds_cover_every_box() {
        assert_eq!(bounds(&[]), None);
        assert_eq!(
            bounds(&[
                [0.0, 0.0, 0.0, 1.0, 0.5, 1.0],
                [0.5, 0.5, 0.0, 1.0, 1.0, 0.5]
            ]),
            Some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
        );
        assert_eq!(bounds(&[[0.0, 0.0, 0.0, f64::NAN, 1.0, 1.0]]), None);
    }

    #[test]
    fn a_solid_block_is_taken_over() {
        assert!(takes_over(&[FULL], &[FULL]));
        // Stairs: two boxes, the same as their collision.
        let stairs = [
            [0.0, 0.0, 0.0, 1.0, 0.5, 1.0],
            [0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        ];
        assert!(takes_over(&stairs, &stairs));
    }

    #[test]
    fn a_block_with_no_outline_or_no_collision_keeps_the_games() {
        assert!(!takes_over(&[], &[FULL]));
        assert!(!takes_over(&[], &[]));
        // A torch or a flower: an outline, but nothing to bump into.
        let torch = [0.4375, 0.0, 0.4375, 0.5625, 0.625, 0.5625];
        assert!(!takes_over(&[torch], &[]));
    }

    #[test]
    fn a_taller_collision_on_the_same_ground_still_draws_the_outline() {
        // A fence post: outlined one block tall, collided with one and a half.
        let post = [0.375, 0.0, 0.375, 0.625, 1.0, 0.625];
        let collision = [0.375, 0.0, 0.375, 0.625, 1.5, 0.625];
        assert!(takes_over(&[post], &[collision]));
        // A snow layer: collided with lower than outlined.
        assert!(takes_over(
            &[[0.0, 0.0, 0.0, 1.0, 0.25, 1.0]],
            &[[0.0, 0.0, 0.0, 1.0, 0.125, 1.0]]
        ));
    }

    #[test]
    fn a_collision_off_the_outlines_centre_keeps_the_games() {
        // A stalk moved at random inside its block: its collision moved, its outline read not.
        let outline = [0.3125, 0.0, 0.3125, 0.6875, 1.0, 0.6875];
        let moved_x = [0.5, 0.0, 0.40625, 0.6875, 1.0, 0.59375];
        let moved_z = [0.40625, 0.0, 0.25, 0.59375, 1.0, 0.4375];
        assert!(!takes_over(&[outline], &[moved_x]));
        assert!(!takes_over(&[outline], &[moved_z]));
        // Moved only up or down is not across the ground.
        let lower = [0.40625, -0.2, 0.40625, 0.59375, 0.8, 0.59375];
        assert!(takes_over(&[outline], &[lower]));
    }

    #[test]
    fn a_broken_or_too_busy_shape_keeps_the_games() {
        assert!(!takes_over(&[[0.0, 0.0, 0.0, f64::NAN, 1.0, 1.0]], &[FULL]));
        assert!(!takes_over(
            &[FULL],
            &[[f64::INFINITY, 0.0, 0.0, 1.0, 1.0, 1.0]]
        ));
        let busy = vec![FULL; MAX_BOXES + 1];
        assert!(!takes_over(&busy, &[FULL]));
        let most = vec![FULL; MAX_BOXES];
        assert!(takes_over(&most, &[FULL]));
    }

    #[test]
    fn the_lines_outline_each_grown_box_in_the_world() {
        let mut out = Vec::with_capacity(MAX_LINES);
        let reserved = out.capacity();
        let boxes = [
            [0.0, 0.0, 0.0, 1.0, 0.5, 1.0],
            [0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        ];
        push_lines(&mut out, [10, 64, -5], &boxes, 0xCC00_FFFF, 2.0);
        assert_eq!(out.len(), 24);
        for line in &out {
            assert_eq!(line.color, 0xCC00_FFFF);
            assert_eq!(line.width, 2.0);
            assert_eq!(line.depth, WorldDepth::Tested);
            for p in [line.from, line.to] {
                assert!((9.997..=11.003).contains(&p[0]), "{p:?}");
                assert!((63.997..=65.003).contains(&p[1]), "{p:?}");
                assert!((-5.003..=-3.997).contains(&p[2]), "{p:?}");
            }
        }
        assert!((out[0].from[0] - (10.0 - GROW)).abs() < 1e-9);
        assert!((out[0].to[0] - (11.0 + GROW)).abs() < 1e-9);
        // The most boxes ever drawn fill the buffer reserved at the start, never past it.
        out.clear();
        push_lines(&mut out, [0, 0, 0], &vec![FULL; MAX_BOXES + 5], 0, 1.0);
        assert_eq!(out.len(), MAX_LINES);
        assert_eq!(out.capacity(), reserved);
    }
}
