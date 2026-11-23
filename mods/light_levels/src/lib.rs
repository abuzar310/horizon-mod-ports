//! Light Levels: press F7 to mark the blocks around you where hostile mobs can spawn.
//!
//! While the marks are on, the mod sweeps a box around the player for spawn spots: a cell whose
//! block below is a full, sturdy floor and whose own block a mob can stand in. A spot is marked
//! when its block light is at most the `limit` setting (0 by default, today's Overworld and End
//! rule): red when its sky light is below 8 (dark by day too: caves, rooms), yellow when it is 8
//! or more (only at night). The marks are crosses, squares or the block light as a number, drawn
//! on the floor's top face and hidden behind anything nearer.
//!
//! It reads only what the client already holds: the blocks around the player and the stored
//! light, the same values F3 shows. Nothing is sent and nothing is automated.
//!
//! The spawn test is the general monster rule, not a per-mob one, and approximates the floor
//! rule from what the SDK gives a WebAssembly mod (see the README):
//! - a floor has collision, an outline that covers its whole top face (a top slab and upside
//!   down stairs do, a bottom slab doesn't), gives off less than light 14, is not in the game's
//!   `minecraft:leaves`, `minecraft:ice` or `minecraft:impermeable` (glass) tags, and is not a
//!   barrier or a magma block;
//! - a mob's feet fit where the block has no collision (a carpet has some, one layer of snow
//!   has none), holds no fluid, and is not in `minecraft:prevent_mob_spawning_inside` (rails),
//!   `minecraft:buttons` or `minecraft:pressure_plates`.
//!
//! The tags are read from the game at runtime, never copied here.
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use mod_api::{
    BlockKey, BlockState, ClientHost, ClientLevelLoadEvent, ClientLevelUnloadEvent,
    ClientTickPostEvent, Hooks, KeyConflictContext, KeyRegistration, LevelRef, LightLayer,
    Registrar, RegistrationContext, RenderLevelStage, RenderLevelStageEvent, SettingSpec,
    WorldDepth, WorldLine, WorldQuad,
};

/// The mod's id: its settings folder, its key's owner and its translation keys.
pub const ID: &str = "light_levels";
/// The id of the mod's key (its line in `horizon-mods.txt`).
pub const KEY: &str = "toggle";
/// The key's name on Options > Controls.
pub const KEY_LABEL: &str = "key.light_levels.toggle";
/// The key it is bound to until the player changes it: F7, which Horizon leaves unbound.
pub const DEFAULT_KEY: &str = "key.keyboard.f7";
/// The most clicks of the key taken in a tick. Its auto-repeat adds about one a tick, so this is
/// never reached by a player; it only bounds the loop.
pub const MAX_CLICKS: u32 = 32;
/// The "remember state" setting's key.
pub const REMEMBER: &str = "remember";

/// Where the marks are drawn: after water and stained glass, so a mark under water or behind
/// glass is not washed out by it.
pub const STAGE: RenderLevelStage = RenderLevelStage::AfterTranslucentBlocks;
/// The most host reads a client tick spends on the sweep (block states and light), checked
/// before each column, so a tick reads at most one column more.
pub const BUDGET: u32 = 4096;
/// What reading a block state the mod hasn't met yet costs on top of the read: its outline,
/// collision, light, fluid and tags.
pub const CLASSIFY_COST: u32 = 8;
/// A new sweep starts at most this many ticks after the last one started, when the player
/// stands still: the marks follow torches placed and blocks broken within a second.
pub const RESTART_TICKS: u32 = 20;
/// The most marks shown, nearest first; fewer in the Number style, whose marks are lines each.
pub const MAX_MARKS: usize = 1024;
pub const MAX_NUMBER_MARKS: usize = 256;
/// The most marks a sweep collects before it stops early: its columns run nearest first, so
/// the ones it skips are the farthest, and the nearest [`MAX_MARKS`] of these are kept.
pub const GATHER: usize = 4 * MAX_MARKS;
/// Sky light from which a spot is dark enough only at night: yellow.
pub const DAYLIGHT_SKY: i32 = 8;
/// A floor giving off this much light or more is no spawn floor (vanilla's default rule).
pub const MAX_FLOOR_EMISSION: u8 = 14;
/// How far above the floor a mark floats, so it doesn't flicker against the face.
pub const LIFT: f64 = 0.02;
/// The highest cover a mark sits on top of (one layer of snow, petals); a taller plant's mark
/// stays on the floor rather than floating over it.
pub const THIN: f64 = 0.25;
/// How far in from the block's edges a cross and a square sit.
pub const CROSS_INSET: f64 = 0.15;
pub const SQUARE_INSET: f64 = 0.1;
/// The opacity of a line (crosses, numbers) and of a square.
pub const LINE_ALPHA: u32 = 0xFF;
pub const SQUARE_ALPHA: u32 = 0x60;

/// Block tags whose blocks are no spawn floor for a monster.
pub const NOT_FLOOR_TAGS: [&str; 3] =
    ["minecraft:leaves", "minecraft:ice", "minecraft:impermeable"];
/// Blocks no monster spawns on that no tag above holds, found by name when a level loads.
pub const NOT_FLOOR_BLOCKS: [&str; 2] = ["minecraft:barrier", "minecraft:magma_block"];
/// Block tags whose blocks keep a mob from spawning in them.
pub const NOT_FEET_TAGS: [&str; 3] = [
    "minecraft:prevent_mob_spawning_inside",
    "minecraft:buttons",
    "minecraft:pressure_plates",
];

/// The settings' defaults and ranges.
pub const DEFAULT_RADIUS: i32 = 12;
pub const MIN_RADIUS: i32 = 4;
pub const MAX_RADIUS: i32 = 24;
pub const DEFAULT_HEIGHT: i32 = 8;
pub const MIN_HEIGHT: i32 = 2;
pub const MAX_HEIGHT: i32 = 16;
pub const DEFAULT_LIMIT: i32 = 0;
pub const MAX_LIMIT: i32 = 15;
pub const DEFAULT_RED: u32 = 0xFF_3030;
pub const DEFAULT_YELLOW: u32 = 0xFF_D030;
pub const DEFAULT_WIDTH: f64 = 2.0;
pub const MIN_WIDTH: f64 = 1.0;
pub const MAX_WIDTH: f64 = 4.0;

/// The mod's key, listed under its name on Options > Controls, where the player can rebind it.
/// It acts only while playing.
pub fn key_registration() -> KeyRegistration {
    KeyRegistration::new(KEY, DEFAULT_KEY)
        .label(KEY_LABEL)
        .context(KeyConflictContext::InGame)
}

/// The settings on Light Levels' page in Options > Mods.
pub fn settings() -> [SettingSpec; 9] {
    [
        SettingSpec::choice(
            "style",
            "light_levels.setting.style",
            Style::ALL.map(Style::label),
            0,
        )
        .tooltip("light_levels.setting.style.tooltip"),
        SettingSpec::range(
            "radius",
            "light_levels.setting.radius",
            f64::from(MIN_RADIUS),
            f64::from(MAX_RADIUS),
            1.0,
            f64::from(DEFAULT_RADIUS),
        )
        .tooltip("light_levels.setting.radius.tooltip"),
        SettingSpec::range(
            "height",
            "light_levels.setting.height",
            f64::from(MIN_HEIGHT),
            f64::from(MAX_HEIGHT),
            1.0,
            f64::from(DEFAULT_HEIGHT),
        )
        .tooltip("light_levels.setting.height.tooltip"),
        SettingSpec::range(
            "limit",
            "light_levels.setting.limit",
            0.0,
            f64::from(MAX_LIMIT),
            1.0,
            f64::from(DEFAULT_LIMIT),
        )
        .tooltip("light_levels.setting.limit.tooltip"),
        SettingSpec::toggle("split_sky", "light_levels.setting.split_sky", true)
            .tooltip("light_levels.setting.split_sky.tooltip"),
        SettingSpec::color("red", "light_levels.setting.red", DEFAULT_RED)
            .tooltip("light_levels.setting.red.tooltip"),
        SettingSpec::color("yellow", "light_levels.setting.yellow", DEFAULT_YELLOW)
            .tooltip("light_levels.setting.yellow.tooltip"),
        SettingSpec::range(
            "width",
            "light_levels.setting.width",
            MIN_WIDTH,
            MAX_WIDTH,
            0.5,
            DEFAULT_WIDTH,
        )
        .tooltip("light_levels.setting.width.tooltip"),
        SettingSpec::toggle(REMEMBER, "light_levels.setting.remember", false)
            .tooltip("light_levels.setting.remember.tooltip"),
    ]
}

/// How a mark looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Two lines from corner to corner.
    Cross,
    /// A translucent square.
    Square,
    /// The spot's block light, in seven-segment digits.
    Number,
}

impl Style {
    /// The styles in the order the style setting lists them.
    pub const ALL: [Style; 3] = [Style::Cross, Style::Square, Style::Number];

    /// The style the setting's option `choice` names; a cross for none or an unknown one.
    pub fn from_choice(choice: Option<u32>) -> Self {
        match choice {
            Some(1) => Style::Square,
            Some(2) => Style::Number,
            _ => Style::Cross,
        }
    }

    /// The option's translation key.
    pub fn label(self) -> &'static str {
        match self {
            Style::Cross => "light_levels.style.cross",
            Style::Square => "light_levels.style.square",
            Style::Number => "light_levels.style.number",
        }
    }

    /// The most marks drawn in this style.
    pub fn cap(self) -> usize {
        match self {
            Style::Number => MAX_NUMBER_MARKS,
            _ => MAX_MARKS,
        }
    }
}

/// When mobs can spawn on a spot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Day and night: the sky light is below 8 (or the sky is not told apart).
    Red,
    /// Only at night: the sky light is 8 or more.
    Yellow,
}

/// Whether a spot with this block and sky light is marked, and how: none when its block light
/// is above `limit`. With `split` off every mark is red.
pub fn classify(block: i32, sky: i32, limit: i32, split: bool) -> Option<Kind> {
    if block > limit {
        return None;
    }
    Some(if split && sky >= DAYLIGHT_SKY {
        Kind::Yellow
    } else {
        Kind::Red
    })
}

/// Whether a block's outline (its boxes `[min x, y, z, max x, y, z]` in its own space) covers
/// its whole top face: a 4 by 4 grid of points on the face each lies in a box reaching the top.
/// True for a full block, a top slab and upside-down stairs; false for a bottom slab, upright
/// stairs, a hopper's or a cauldron's open top, and no outline at all.
pub fn top_face_full(boxes: &[[f64; 6]]) -> bool {
    const SAMPLES: [f64; 4] = [0.125, 0.375, 0.625, 0.875];
    SAMPLES.iter().all(|&x| {
        SAMPLES.iter().all(|&z| {
            boxes
                .iter()
                .any(|b| b[4] >= 0.999 && b[0] <= x && x <= b[3] && b[2] <= z && z <= b[5])
        })
    })
}

/// How far above the floor a mark sits when a mob's feet block has this outline: the top of a
/// thin cover (one layer of snow, petals), 0 for anything taller or no outline.
pub fn cover_top(boxes: &[[f64; 6]]) -> f64 {
    let top = boxes.iter().map(|b| b[4]).fold(0.0, f64::max);
    if top.is_finite() && top > 0.0 && top <= THIN {
        top
    } else {
        0.0
    }
}

/// The height a mark is drawn at, for a spot at `y` whose cover is `raise` high.
pub fn mark_y(y: i32, raise: f64) -> f64 {
    f64::from(y) + raise + LIFT
}

/// What a sweep needs to know of one block state, worked out once per state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cell {
    /// A floor a monster spawns on.
    pub floor: bool,
    /// Room for a monster's feet.
    pub feet: bool,
    /// As the feet block: how far above the floor its mark sits ([`cover_top`]).
    pub raise: f64,
}

/// A spawn spot found: the cell a mob would stand in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub pos: [i32; 3],
    pub kind: Kind,
    /// Its block light, 0 to 15.
    pub light: u8,
    /// How far above the floor the mark sits.
    pub raise: f64,
}

/// The settings that decide what is marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    pub radius: i32,
    pub height: i32,
    pub limit: i32,
    pub split: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            radius: DEFAULT_RADIUS,
            height: DEFAULT_HEIGHT,
            limit: DEFAULT_LIMIT,
            split: true,
        }
    }
}

/// The world as a sweep reads it.
pub trait Reader {
    /// The cell at `pos`; none where the client holds no chunk.
    fn cell(&mut self, pos: [i32; 3]) -> Option<Cell>;
    /// The stored light of one layer at `pos`, 0 to 15.
    fn light(&mut self, layer: LightLayer, pos: [i32; 3]) -> Option<i32>;
    /// The host reads spent so far this tick.
    fn spent(&self) -> u32;
}

/// The columns a sweep reads, as `[x, z]` offsets from the player: every column within
/// `radius` blocks across the ground exactly once, nearest first.
pub fn columns(radius: i32) -> Vec<[i32; 2]> {
    let r = radius.max(0);
    let mut out = Vec::with_capacity(((2 * r + 1) * (2 * r + 1)) as usize);
    for dx in -r..=r {
        for dz in -r..=r {
            if dx * dx + dz * dz <= r * r {
                out.push([dx, dz]);
            }
        }
    }
    out.sort_by_key(|&[dx, dz]| (dx * dx + dz * dz, dz, dx));
    out
}

/// The square of the distance between two cells.
pub fn distance2(a: [i32; 3], b: [i32; 3]) -> i64 {
    (0..3)
        .map(|i| {
            let d = i64::from(a[i]) - i64::from(b[i]);
            d * d
        })
        .sum()
}

/// Reads the column at `x`, `z` from `top` down to `bottom`, each block once: a block read is
/// the floor of the cell above it, read just before. Adds a mark for each spawn spot whose light
/// passes. A column whose chunk the client doesn't hold stops at its first read.
pub fn scan_column(
    x: i32,
    z: i32,
    top: i32,
    bottom: i32,
    params: &Params,
    reader: &mut impl Reader,
    out: &mut Vec<Mark>,
) {
    let mut above: Option<Cell> = None;
    let mut y = top;
    while y >= bottom {
        let Some(cell) = reader.cell([x, y, z]) else {
            return;
        };
        if let Some(feet) = above
            && feet.feet
            && cell.floor
            && let Some(mark) = spot_mark([x, y + 1, z], feet, params, reader)
        {
            out.push(mark);
        }
        above = Some(cell);
        y -= 1;
    }
}

/// The mark for the spot `pos` (feet block `feet`, on a floor), read from its light: the sky
/// light only once the block light passes.
fn spot_mark(pos: [i32; 3], feet: Cell, params: &Params, reader: &mut impl Reader) -> Option<Mark> {
    let block = reader.light(LightLayer::Block, pos)?;
    if block > params.limit {
        return None;
    }
    let sky = if params.split {
        reader.light(LightLayer::Sky, pos)?
    } else {
        0
    };
    let kind = classify(block, sky, params.limit, params.split)?;
    Some(Mark {
        pos,
        kind,
        light: block.clamp(0, 15) as u8,
        raise: feet.raise,
    })
}

/// One sweep of the box around where the player stood when it began, a few columns a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sweep {
    /// The player's feet cell when the sweep began.
    pub origin: [i32; 3],
    /// The world's lowest and highest block `y`.
    pub world: [i32; 2],
    /// The next column to read: an index into the column list.
    pub column: usize,
}

impl Sweep {
    pub fn new(origin: [i32; 3], world: [i32; 2]) -> Self {
        Self {
            origin,
            world,
            column: 0,
        }
    }

    /// The `y` a column is read from and down to: `height` above the feet down to the floor
    /// under the lowest spot, inside the world.
    pub fn span(&self, height: i32) -> (i32, i32) {
        let top = (self.origin[1] + height).min(self.world[1]);
        let bottom = (self.origin[1] - height - 1).max(self.world[0]);
        (top, bottom)
    }

    /// Whether every column has been read.
    pub fn done(&self, columns: &[[i32; 2]]) -> bool {
        self.column >= columns.len()
    }

    /// Reads whole columns, from where the last step stopped, while the reader has spent less
    /// than `budget`, adding the marks found to `out`. Once `out` holds [`GATHER`] marks the
    /// sweep ends early.
    pub fn step(
        &mut self,
        columns: &[[i32; 2]],
        params: &Params,
        budget: u32,
        reader: &mut impl Reader,
        out: &mut Vec<Mark>,
    ) {
        let (top, bottom) = self.span(params.height);
        while reader.spent() < budget && out.len() < GATHER {
            let Some(&[dx, dz]) = columns.get(self.column) else {
                return;
            };
            self.column += 1;
            let (x, z) = (self.origin[0] + dx, self.origin[2] + dz);
            scan_column(x, z, top, bottom, params, reader, out);
        }
        if out.len() >= GATHER {
            self.column = columns.len();
        }
    }
}

/// Whether the player at `feet` has left the box a sweep from `origin` reads.
pub fn left_box(origin: [i32; 3], feet: [i32; 3], params: &Params) -> bool {
    let d = |i: usize| (i64::from(feet[i]) - i64::from(origin[i])).abs();
    d(0) > i64::from(params.radius)
        || d(2) > i64::from(params.radius)
        || d(1) > i64::from(params.height)
}

/// Orders a finished sweep's marks nearest to `origin` first and keeps [`MAX_MARKS`].
pub fn finish(marks: &mut Vec<Mark>, origin: [i32; 3]) {
    marks.sort_by_key(|mark| distance2(mark.pos, origin));
    marks.truncate(MAX_MARKS);
}

/// The sweeps, one after another, and the marks of the last one finished.
#[derive(Debug, Default)]
pub struct Scanner {
    columns: Vec<[i32; 2]>,
    params: Option<Params>,
    sweep: Option<Sweep>,
    next: Vec<Mark>,
    /// The marks shown: the last finished sweep's, nearest first.
    pub shown: Vec<Mark>,
    /// Changes each time `shown` does.
    pub generation: u64,
    /// Where the last sweep began, and the ticks since.
    last: Option<[i32; 3]>,
    ticks: u32,
}

impl Scanner {
    /// One client tick with the player's feet at `feet` in a world from `world[0]` to
    /// `world[1]`. A sweep under way goes on (its marks are in the world's own coordinates, so
    /// they stay right while the player walks); it is dropped only when a setting that decides
    /// the marks changes, the world's height changes or the player leaves its box. A new sweep
    /// begins when none is under way and the player has moved a block since the last began, or
    /// [`RESTART_TICKS`] have passed.
    pub fn tick(
        &mut self,
        feet: [i32; 3],
        world: [i32; 2],
        params: Params,
        budget: u32,
        reader: &mut impl Reader,
    ) {
        self.ticks = self.ticks.saturating_add(1);
        if self.params != Some(params) {
            if self.params.map(|p| p.radius) != Some(params.radius) {
                self.columns = columns(params.radius);
            }
            self.params = Some(params);
            self.abort();
        }
        let stale = self
            .sweep
            .is_some_and(|sweep| sweep.world != world || left_box(sweep.origin, feet, &params));
        if stale {
            self.abort();
        }
        if self.sweep.is_none() {
            let due = self.last != Some(feet) || self.ticks >= RESTART_TICKS;
            if !due {
                return;
            }
            self.sweep = Some(Sweep::new(feet, world));
            self.last = Some(feet);
            self.ticks = 0;
        }
        let Some(sweep) = self.sweep.as_mut() else {
            return;
        };
        sweep.step(&self.columns, &params, budget, reader, &mut self.next);
        if sweep.done(&self.columns) {
            let origin = sweep.origin;
            self.sweep = None;
            finish(&mut self.next, origin);
            std::mem::swap(&mut self.shown, &mut self.next);
            self.next.clear();
            self.generation += 1;
        }
    }

    /// Drops the sweep under way; the next tick begins another.
    fn abort(&mut self) {
        self.sweep = None;
        self.next.clear();
        self.last = None;
    }

    /// Forgets every mark (the marks turned off, another level).
    pub fn clear(&mut self) {
        self.abort();
        if !self.shown.is_empty() {
            self.shown.clear();
            self.generation += 1;
        }
    }
}

/// Whether the marks are on, from the key, and whether a level load is a join.
#[derive(Debug, Default)]
pub struct Toggle {
    pub visible: bool,
    was_down: bool,
    levels: u32,
}

impl Toggle {
    /// The key this tick: whether it is held and the clicks it got. A press flips the marks
    /// once however long the key is held (its auto-repeat adds clicks). True when it flipped.
    pub fn key(&mut self, down: bool, clicks: u32) -> bool {
        let pressed = clicks > 0 && !self.was_down;
        self.was_down = down;
        if pressed {
            self.visible = !self.visible;
        }
        pressed
    }

    /// The client made a level. With none open before, the player is joining a world: the marks
    /// start hidden unless `remember` is on. Changing dimension makes the new level before it
    /// drops the old one, so it is no join and keeps the marks as they are.
    pub fn level_loaded(&mut self, remember: bool) {
        if self.levels == 0 && !remember {
            self.visible = false;
        }
        self.levels = self.levels.saturating_add(1);
    }

    /// The client dropped a level: another dimension's replaced it, or the player left.
    pub fn level_unloaded(&mut self) {
        self.levels = self.levels.saturating_sub(1);
    }
}

/// A packed `0xAARRGGBB` colour from a `0xRRGGBB` colour (its top byte dropped) and an alpha.
pub fn argb(rgb: u32, alpha: u32) -> u32 {
    (alpha.min(0xFF) << 24) | (rgb & 0x00FF_FFFF)
}

/// A whole-number setting's value, the default when there is none or it is not a number, kept in
/// `min..=max`.
pub fn whole(value: Option<f64>, default: i32, min: i32, max: i32) -> i32 {
    value
        .filter(|v| v.is_finite())
        .map_or(default, |v| v.round() as i32)
        .clamp(min, max)
}

/// The line width for the width setting's value, in screen pixels, always one the game takes.
pub fn line_width(value: Option<f64>) -> f32 {
    value
        .filter(|v| v.is_finite())
        .unwrap_or(DEFAULT_WIDTH)
        .clamp(MIN_WIDTH, MAX_WIDTH) as f32
}

/// The settings that decide how the marks look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub style: Style,
    pub red: u32,
    pub yellow: u32,
    pub width: f32,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            style: Style::Cross,
            red: DEFAULT_RED,
            yellow: DEFAULT_YELLOW,
            width: DEFAULT_WIDTH as f32,
        }
    }
}

impl Look {
    /// A mark's packed colour: opaque for lines, translucent for a square.
    pub fn color(&self, kind: Kind) -> u32 {
        let rgb = match kind {
            Kind::Red => self.red,
            Kind::Yellow => self.yellow,
        };
        let alpha = match self.style {
            Style::Square => SQUARE_ALPHA,
            _ => LINE_ALPHA,
        };
        argb(rgb, alpha)
    }
}

/// A cross on the top face of the floor under the spot `pos`, at height `y`: two lines from
/// corner to corner, [`CROSS_INSET`] in from the edges.
pub fn cross_lines(pos: [i32; 3], y: f64) -> [[[f64; 3]; 2]; 2] {
    let (x0, z0) = (
        f64::from(pos[0]) + CROSS_INSET,
        f64::from(pos[2]) + CROSS_INSET,
    );
    let (x1, z1) = (
        f64::from(pos[0]) + 1.0 - CROSS_INSET,
        f64::from(pos[2]) + 1.0 - CROSS_INSET,
    );
    [[[x0, y, z0], [x1, y, z1]], [[x1, y, z0], [x0, y, z1]]]
}

/// A square on the top face of the floor under `pos`, at height `y`, [`SQUARE_INSET`] in from
/// the edges, its corners in order around it.
pub fn square_corners(pos: [i32; 3], y: f64) -> [[f64; 3]; 4] {
    let (x0, z0) = (
        f64::from(pos[0]) + SQUARE_INSET,
        f64::from(pos[2]) + SQUARE_INSET,
    );
    let (x1, z1) = (
        f64::from(pos[0]) + 1.0 - SQUARE_INSET,
        f64::from(pos[2]) + 1.0 - SQUARE_INSET,
    );
    [[x0, y, z0], [x0, y, z1], [x1, y, z1], [x1, y, z0]]
}

/// The seven segments of a digit, each as its two ends in the digit's own box `[u, v]`: u from
/// its left (0) to its right (1), v from its bottom (0) to its top (1). In order: top, top
/// right, bottom right, bottom, bottom left, top left, middle.
pub const SEGMENTS: [[[f64; 2]; 2]; 7] = [
    [[0.0, 1.0], [1.0, 1.0]],
    [[1.0, 1.0], [1.0, 0.5]],
    [[1.0, 0.5], [1.0, 0.0]],
    [[0.0, 0.0], [1.0, 0.0]],
    [[0.0, 0.5], [0.0, 0.0]],
    [[0.0, 1.0], [0.0, 0.5]],
    [[0.0, 0.5], [1.0, 0.5]],
];
/// The segments each digit lights, a bit per segment in [`SEGMENTS`]' order.
pub const DIGITS: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
/// A digit's size on the face, and the gap between two, in blocks.
pub const DIGIT_WIDTH: f64 = 0.25;
pub const DIGIT_HEIGHT: f64 = 0.5;
pub const DIGIT_GAP: f64 = 0.1;

/// The segments digit `digit` lights; none for anything above 9.
pub fn digit_segments(digit: u8) -> u8 {
    DIGITS.get(usize::from(digit)).copied().unwrap_or(0)
}

/// The digits a value is written with (at most 99), and how many.
pub fn split_digits(value: u8) -> ([u8; 2], usize) {
    let value = value.min(99);
    if value >= 10 {
        ([value / 10, value % 10], 2)
    } else {
        ([value, 0], 1)
    }
}

/// The quarter turn nearest a yaw in degrees, the way the player faces: 0 south (+z), 1 west
/// (-x), 2 north (-z), 3 east (+x).
pub fn quadrant(yaw: f32) -> u8 {
    if !yaw.is_finite() {
        return 0;
    }
    ((f64::from(yaw) / 90.0).round() as i64).rem_euclid(4) as u8
}

/// The way a player facing `quadrant` looks, across the ground `[x, z]`.
pub fn forward(quadrant: u8) -> [f64; 2] {
    match quadrant % 4 {
        0 => [0.0, 1.0],
        1 => [-1.0, 0.0],
        2 => [0.0, -1.0],
        _ => [1.0, 0.0],
    }
}

/// The player's right hand when facing `quadrant`, across the ground `[x, z]`.
pub fn right(quadrant: u8) -> [f64; 2] {
    let [x, z] = forward(quadrant);
    [-z, x]
}

/// The lines writing `value` in seven-segment digits on the top face whose lowest corner is
/// `origin` (the face's height in `origin[1]`), centred, upright for a player facing
/// `quadrant`: its top away from them. One digit below 10, two from 10; at most 14 lines.
pub fn number_lines(
    value: u8,
    origin: [f64; 3],
    quadrant: u8,
    mut line: impl FnMut([f64; 3], [f64; 3]),
) {
    let (digits, count) = split_digits(value);
    let total = count as f64 * DIGIT_WIDTH + (count as f64 - 1.0) * DIGIT_GAP;
    let [fx, fz] = forward(quadrant);
    let [rx, rz] = right(quadrant);
    let point = |s: f64, t: f64| {
        [
            origin[0] + 0.5 + s * rx + t * fx,
            origin[1],
            origin[2] + 0.5 + s * rz + t * fz,
        ]
    };
    for (i, &digit) in digits[..count].iter().enumerate() {
        let left = -total / 2.0 + i as f64 * (DIGIT_WIDTH + DIGIT_GAP);
        let lit = digit_segments(digit);
        for (bit, &[[u0, v0], [u1, v1]]) in SEGMENTS.iter().enumerate() {
            if lit & (1 << bit) != 0 {
                line(
                    point(left + u0 * DIGIT_WIDTH, (v0 - 0.5) * DIGIT_HEIGHT),
                    point(left + u1 * DIGIT_WIDTH, (v1 - 0.5) * DIGIT_HEIGHT),
                );
            }
        }
    }
}

/// How many primitives to submit so that only whole marks go and they fit in `room`: `ends`
/// holds where each mark's primitives end, in order.
pub fn fit(ends: &[u32], room: u32) -> usize {
    let whole = ends.partition_point(|&end| end <= room);
    whole.checked_sub(1).map_or(0, |last| ends[last] as usize)
}

/// What the picture was built from: it is built again only when one of these changes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PictureKey {
    generation: u64,
    look: Look,
    quadrant: u8,
}

/// The marks' lines or quads, built once for each set of marks and look, and submitted every
/// frame.
#[derive(Debug, Default)]
pub struct Picture {
    key: Option<PictureKey>,
    pub lines: Vec<WorldLine>,
    pub quads: Vec<WorldQuad>,
    /// Where each mark's primitives end in `lines` (crosses, numbers) or `quads` (squares).
    pub ends: Vec<u32>,
}

impl Picture {
    /// Builds the primitives of the nearest marks the style draws, each tested against the
    /// world's depth.
    pub fn build(&mut self, marks: &[Mark], look: &Look, quadrant: u8) {
        self.lines.clear();
        self.quads.clear();
        self.ends.clear();
        for mark in marks.iter().take(look.style.cap()) {
            let color = look.color(mark.kind);
            let y = mark_y(mark.pos[1], mark.raise);
            match look.style {
                Style::Cross => {
                    for [from, to] in cross_lines(mark.pos, y) {
                        self.lines.push(WorldLine::new(
                            from,
                            to,
                            color,
                            look.width,
                            WorldDepth::Tested,
                        ));
                    }
                    self.ends.push(self.lines.len() as u32);
                }
                Style::Square => {
                    self.quads.push(WorldQuad::new(
                        square_corners(mark.pos, y),
                        color,
                        WorldDepth::Tested,
                    ));
                    self.ends.push(self.quads.len() as u32);
                }
                Style::Number => {
                    let origin = [f64::from(mark.pos[0]), y, f64::from(mark.pos[2])];
                    let lines = &mut self.lines;
                    number_lines(mark.light, origin, quadrant, |from, to| {
                        lines.push(WorldLine::new(
                            from,
                            to,
                            color,
                            look.width,
                            WorldDepth::Tested,
                        ));
                    });
                    self.ends.push(self.lines.len() as u32);
                }
            }
        }
    }
}

/// Whether `block` is in any of `tags`: none when a tag can't be read yet.
fn in_any_tag(host: &dyn ClientHost, block: BlockKey, tags: &[&str]) -> Option<bool> {
    let mut known = true;
    for tag in tags {
        match host.block_in_tag(block, tag) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => known = false,
        }
    }
    known.then_some(false)
}

/// Works out the cell for `state`, met first at `pos` (its collision is read there), and whether
/// every read answered: a cell worked out from a missing read is used once and not kept.
fn read_cell(
    host: &dyn ClientHost,
    state: BlockState,
    pos: [i32; 3],
    not_floor: &[BlockKey],
) -> (Cell, bool) {
    let Some(outline) = host.state_outline(state) else {
        return (Cell::default(), false);
    };
    let solid = !host.block_collision(pos).is_empty();
    let block = state.block();
    let mut complete = true;
    let mut untagged = |tags: &[&str]| match in_any_tag(host, block, tags) {
        Some(tagged) => !tagged,
        None => {
            complete = false;
            false
        }
    };
    let floor = solid
        && top_face_full(&outline)
        && !not_floor.contains(&block)
        && host
            .block_state_light_emission(state)
            .is_some_and(|light| light < MAX_FLOOR_EMISSION)
        && untagged(NOT_FLOOR_TAGS.as_slice());
    let feet = !solid
        && host
            .state_fluid_state(state)
            .is_none_or(|fluid| fluid.is_empty())
        && untagged(NOT_FEET_TAGS.as_slice());
    let raise = if feet { cover_top(&outline) } else { 0.0 };
    (Cell { floor, feet, raise }, complete)
}

/// The cells worked out in this level, one per block state, and the blocks named in
/// [`NOT_FLOOR_BLOCKS`].
#[derive(Default)]
struct Cache {
    cells: HashMap<BlockState, Cell>,
    not_floor: Vec<BlockKey>,
}

impl Cache {
    /// Finds the blocks of [`NOT_FLOOR_BLOCKS`] by name, until all are found.
    fn resolve(&mut self, host: &dyn ClientHost) {
        if self.not_floor.len() < NOT_FLOOR_BLOCKS.len() {
            self.not_floor = NOT_FLOOR_BLOCKS
                .iter()
                .filter_map(|name| host.block_state_id(name))
                .map(|state| state.block())
                .collect();
        }
    }

    fn clear(&mut self) {
        self.cells.clear();
        self.not_floor.clear();
    }
}

/// The level the player is in, read through the host, its cells cached.
struct HostReader<'a> {
    host: &'a dyn ClientHost,
    level: LevelRef,
    cache: &'a mut Cache,
    spent: u32,
}

impl Reader for HostReader<'_> {
    fn cell(&mut self, pos: [i32; 3]) -> Option<Cell> {
        self.spent += 1;
        let state = self.level.block_state(self.host, pos)?;
        if let Some(&cell) = self.cache.cells.get(&state) {
            return Some(cell);
        }
        self.spent += CLASSIFY_COST;
        let (cell, complete) = read_cell(self.host, state, pos, &self.cache.not_floor);
        if complete {
            self.cache.cells.insert(state, cell);
        }
        Some(cell)
    }

    fn light(&mut self, layer: LightLayer, pos: [i32; 3]) -> Option<i32> {
        self.spent += 1;
        self.level.brightness(self.host, layer, pos)
    }

    fn spent(&self) -> u32 {
        self.spent
    }
}

/// The settings that decide what is marked, as the player set them.
fn read_params(host: &dyn ClientHost) -> Params {
    Params {
        radius: whole(
            host.setting_f64("radius"),
            DEFAULT_RADIUS,
            MIN_RADIUS,
            MAX_RADIUS,
        ),
        height: whole(
            host.setting_f64("height"),
            DEFAULT_HEIGHT,
            MIN_HEIGHT,
            MAX_HEIGHT,
        ),
        limit: whole(host.setting_f64("limit"), DEFAULT_LIMIT, 0, MAX_LIMIT),
        split: host.setting_bool("split_sky").unwrap_or(true),
    }
}

/// The settings that decide how the marks look, as the player set them.
fn read_look(host: &dyn ClientHost) -> Look {
    Look {
        style: Style::from_choice(host.setting_choice("style")),
        red: host.setting_color("red").unwrap_or(DEFAULT_RED),
        yellow: host.setting_color("yellow").unwrap_or(DEFAULT_YELLOW),
        width: line_width(host.setting_f64("width")),
    }
}

/// Takes the clicks the mod's key got since the last tick, at most [`MAX_CLICKS`].
fn take_clicks(host: &mut dyn ClientHost) -> u32 {
    let mut clicks = 0;
    while clicks < MAX_CLICKS && host.consume_click(ID, KEY) {
        clicks += 1;
    }
    clicks
}

#[derive(Default)]
struct State {
    toggle: Toggle,
    scanner: Scanner,
    cache: Cache,
    look: Look,
}

#[derive(Default)]
struct LightLevels {
    state: Mutex<State>,
    picture: Mutex<Picture>,
}

impl LightLevels {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn picture(&self) -> MutexGuard<'_, Picture> {
        self.picture.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[mod_api::horizon_mod]
impl Hooks for LightLevels {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.register_key_mapping(key_registration());
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// The key, then a step of the sweep while the marks are on.
    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        // The key's clicks are taken even while the mod is off, so none wait for it.
        let clicks = take_clicks(host);
        let down = host.is_down(ID, KEY);
        let view: &dyn ClientHost = &*host;
        let mut guard = self.state();
        let state = &mut *guard;
        if !view.on(ID) {
            state.scanner.clear();
            return;
        }
        if state.toggle.key(down, clicks) && !state.toggle.visible {
            state.scanner.clear();
        }
        if !state.toggle.visible {
            return;
        }
        state.look = read_look(view);
        let params = read_params(view);
        let Some(feet) = view
            .local_player()
            .and_then(|player| player.entity().position(view))
            .map(|pos| pos.map(|axis| axis.floor() as i32))
        else {
            return;
        };
        let Some(level) = view.level(view.dimension()) else {
            return;
        };
        let Some((min_y, height)) = level.height(view) else {
            return;
        };
        let world = [min_y, min_y + height - 1];
        let State { scanner, cache, .. } = state;
        cache.resolve(view);
        let mut reader = HostReader {
            host: view,
            level,
            cache,
            spent: 0,
        };
        scanner.tick(feet, world, params, BUDGET, &mut reader);
    }

    /// A level loaded (joining a world, or another dimension): its marks and cells start
    /// afresh. Bookkeeping only, so it runs while the mod is off too, to keep joins apart from
    /// dimension changes.
    fn client_level_load(&self, host: &mut dyn ClientHost, _: &mut ClientLevelLoadEvent) {
        let remember = host.setting_bool(REMEMBER).unwrap_or(false);
        let mut state = self.state();
        state.toggle.level_loaded(remember);
        state.scanner.clear();
        state.cache.clear();
    }

    /// A level dropped: replaced by another dimension's, or the player left. Bookkeeping only.
    fn client_level_unload(&self, _: &mut dyn ClientHost, _: &mut ClientLevelUnloadEvent<'_>) {
        let mut state = self.state();
        state.toggle.level_unloaded();
        state.scanner.clear();
    }

    /// The marks, once a view, after the translucent terrain; as many whole marks as the view
    /// has room left for.
    fn render_level_stage(&self, host: &dyn ClientHost, event: &mut RenderLevelStageEvent<'_>) {
        if event.stage != STAGE || !host.on(ID) {
            return;
        }
        let state = self.state();
        if !state.toggle.visible || state.scanner.shown.is_empty() {
            return;
        }
        let look = state.look;
        // Numbers turn to read upright for the player; the other marks look the same any way.
        let quadrant = match look.style {
            Style::Number => host
                .local_player()
                .and_then(|player| player.entity().rotation(host))
                .map_or(0, |[yaw, _]| quadrant(yaw)),
            _ => 0,
        };
        let key = PictureKey {
            generation: state.scanner.generation,
            look,
            quadrant,
        };
        let mut picture = self.picture();
        if picture.key != Some(key) {
            picture.build(&state.scanner.shown, &look, quadrant);
            picture.key = Some(key);
        }
        drop(state);
        let usage = event.draw.usage();
        // A batch the view has no room for is refused whole, so only whole marks that fit go.
        match look.style {
            Style::Square => {
                let n = fit(
                    &picture.ends,
                    usage.quad_capacity.saturating_sub(usage.quads),
                );
                if n > 0 {
                    let _ = event.draw.quads(&picture.quads[..n]);
                }
            }
            _ => {
                let n = fit(
                    &picture.ends,
                    usage.line_capacity.saturating_sub(usage.lines),
                );
                if n > 0 {
                    let _ = event.draw.lines(&picture.lines[..n]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: [f64; 6] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    const BOTTOM_SLAB: [f64; 6] = [0.0, 0.0, 0.0, 1.0, 0.5, 1.0];
    const TOP_SLAB: [f64; 6] = [0.0, 0.5, 0.0, 1.0, 1.0, 1.0];

    const STONE: Cell = Cell {
        floor: true,
        feet: false,
        raise: 0.0,
    };
    const AIR: Cell = Cell {
        floor: false,
        feet: true,
        raise: 0.0,
    };
    const SNOW: Cell = Cell {
        floor: false,
        feet: true,
        raise: 0.125,
    };
    const GLASS: Cell = Cell {
        floor: false,
        feet: false,
        raise: 0.0,
    };

    /// A world for the sweep: each column holds `layers` from the top of the world down (the
    /// world runs from y 0 to 99, the layers end at 99), lit as `block` and `sky` say, and only
    /// the columns within `loaded` of the origin are loaded. Every read is counted.
    struct World {
        layers: Vec<Cell>,
        block: i32,
        sky: i32,
        loaded: i32,
        spent: u32,
        reads: HashMap<[i32; 3], u32>,
    }

    impl World {
        /// Stone below y 64, air from there up.
        fn flat(block: i32, sky: i32) -> Self {
            let layers = (0..100)
                .rev()
                .map(|y| if y < 64 { STONE } else { AIR })
                .collect();
            Self {
                layers,
                block,
                sky,
                loaded: i32::MAX,
                spent: 0,
                reads: HashMap::new(),
            }
        }
    }

    impl Reader for World {
        fn cell(&mut self, pos: [i32; 3]) -> Option<Cell> {
            self.spent += 1;
            *self.reads.entry(pos).or_default() += 1;
            if pos[0].abs() > self.loaded || pos[2].abs() > self.loaded {
                return None;
            }
            let i = usize::try_from(99 - pos[1]).ok()?;
            self.layers.get(i).copied()
        }

        fn light(&mut self, layer: LightLayer, _: [i32; 3]) -> Option<i32> {
            self.spent += 1;
            Some(match layer {
                LightLayer::Block => self.block,
                LightLayer::Sky => self.sky,
            })
        }

        fn spent(&self) -> u32 {
            self.spent
        }
    }

    const WORLD: [i32; 2] = [0, 99];

    fn params(radius: i32, height: i32) -> Params {
        Params {
            radius,
            height,
            ..Params::default()
        }
    }

    #[test]
    fn every_setting_is_one_the_game_takes() {
        let settings = settings();
        for setting in &settings {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
        let keys: Vec<&str> = settings.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "style",
                "radius",
                "height",
                "limit",
                "split_sky",
                "red",
                "yellow",
                "width",
                "remember"
            ]
        );
    }

    #[test]
    fn the_key_is_f7_while_playing() {
        let key = key_registration();
        assert_eq!(key.id, KEY);
        assert_eq!(key.key, "key.keyboard.f7");
        assert_eq!(key.label, KEY_LABEL);
        assert_eq!(key.context, KeyConflictContext::InGame);
        assert_eq!(key.category, "");
    }

    #[test]
    fn styles_follow_the_setting() {
        assert_eq!(Style::from_choice(None), Style::Cross);
        assert_eq!(Style::from_choice(Some(0)), Style::Cross);
        assert_eq!(Style::from_choice(Some(1)), Style::Square);
        assert_eq!(Style::from_choice(Some(2)), Style::Number);
        assert_eq!(Style::from_choice(Some(9)), Style::Cross);
        for (i, style) in Style::ALL.into_iter().enumerate() {
            assert_eq!(Style::from_choice(Some(i as u32)), style);
        }
        assert_eq!(Style::Number.cap(), 256);
        assert_eq!(Style::Cross.cap(), 1024);
        assert_eq!(Style::Square.cap(), 1024);
    }

    #[test]
    fn a_full_top_face_is_a_floor() {
        assert!(top_face_full(&[FULL]));
        assert!(top_face_full(&[TOP_SLAB]));
        assert!(!top_face_full(&[BOTTOM_SLAB]));
        assert!(!top_face_full(&[]));
        // Upright stairs: a bottom slab and a back half on top.
        assert!(!top_face_full(&[
            BOTTOM_SLAB,
            [0.0, 0.5, 0.5, 1.0, 1.0, 1.0]
        ]));
        // Upside-down stairs: a top slab and a front half below.
        assert!(top_face_full(&[TOP_SLAB, [0.0, 0.0, 0.5, 1.0, 0.5, 1.0]]));
        // A full top made of four quarters.
        assert!(top_face_full(&[
            [0.0, 0.0, 0.0, 0.5, 1.0, 0.5],
            [0.5, 0.0, 0.0, 1.0, 1.0, 0.5],
            [0.0, 0.0, 0.5, 0.5, 1.0, 1.0],
            [0.5, 0.0, 0.5, 1.0, 1.0, 1.0],
        ]));
        // A fence post, and a cauldron's rim around an open top.
        assert!(!top_face_full(&[[0.375, 0.0, 0.375, 0.625, 1.5, 0.625]]));
        assert!(!top_face_full(&[
            [0.0, 0.0, 0.0, 1.0, 0.25, 1.0],
            [0.0, 0.0, 0.0, 0.125, 1.0, 1.0],
            [0.875, 0.0, 0.0, 1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 1.0, 1.0, 0.125],
            [0.0, 0.0, 0.875, 1.0, 1.0, 1.0],
        ]));
        // Farmland and paths stop a sixteenth short.
        assert!(!top_face_full(&[[0.0, 0.0, 0.0, 1.0, 0.9375, 1.0]]));
    }

    #[test]
    fn marks_sit_on_thin_covers_only() {
        assert_eq!(cover_top(&[]), 0.0);
        assert_eq!(cover_top(&[[0.0, 0.0, 0.0, 1.0, 0.125, 1.0]]), 0.125);
        // Short grass reaches 13/16: its mark stays on the floor.
        assert_eq!(cover_top(&[[0.125, 0.0, 0.125, 0.875, 0.8125, 0.875]]), 0.0);
        assert_eq!(cover_top(&[[0.0, 0.0, 0.0, 1.0, f64::NAN, 1.0]]), 0.0);
        assert!((mark_y(64, 0.0) - 64.02).abs() < 1e-12);
        assert!((mark_y(64, 0.125) - 64.145).abs() < 1e-12);
        assert!((mark_y(-5, 0.0) - -4.98).abs() < 1e-12);
    }

    #[test]
    fn light_decides_the_colour() {
        assert_eq!(classify(0, 0, 0, true), Some(Kind::Red));
        assert_eq!(classify(0, 7, 0, true), Some(Kind::Red));
        assert_eq!(classify(0, 8, 0, true), Some(Kind::Yellow));
        assert_eq!(classify(0, 15, 0, true), Some(Kind::Yellow));
        assert_eq!(classify(1, 0, 0, true), None);
        assert_eq!(classify(7, 15, 7, true), Some(Kind::Yellow));
        assert_eq!(classify(8, 0, 7, true), None);
        assert_eq!(classify(0, 15, 0, false), Some(Kind::Red));
        assert_eq!(classify(15, 15, 15, false), Some(Kind::Red));
    }

    #[test]
    fn columns_run_nearest_first_each_once() {
        for radius in [0, 1, 4, 12, 24] {
            let list = columns(radius);
            assert_eq!(list[0], [0, 0]);
            let d2 = |[x, z]: [i32; 2]| x * x + z * z;
            for pair in list.windows(2) {
                assert!(d2(pair[0]) <= d2(pair[1]), "{pair:?}");
            }
            let mut expected = 0;
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let inside = dx * dx + dz * dz <= radius * radius;
                    let count = list.iter().filter(|&&c| c == [dx, dz]).count();
                    assert_eq!(count, usize::from(inside), "{dx} {dz}");
                    expected += usize::from(inside);
                }
            }
            assert_eq!(list.len(), expected);
        }
        assert_eq!(columns(-3), vec![[0, 0]]);
    }

    #[test]
    fn a_column_is_read_top_down_once() {
        let mut world = World::flat(0, 15);
        let mut out = Vec::new();
        scan_column(3, -2, 72, 55, &Params::default(), &mut world, &mut out);
        assert_eq!(
            out,
            [Mark {
                pos: [3, 64, -2],
                kind: Kind::Yellow,
                light: 0,
                raise: 0.0
            }]
        );
        assert_eq!(world.reads.len(), 18);
        assert!(world.reads.values().all(|&n| n == 1));
        // 18 block reads and the spot's two light reads.
        assert_eq!(world.spent, 20);
    }

    #[test]
    fn the_light_decides_whether_and_how_a_spot_is_marked() {
        let dark = Params::default();
        let mut out = Vec::new();
        scan_column(0, 0, 70, 60, &dark, &mut World::flat(0, 4), &mut out);
        assert_eq!(out[0].kind, Kind::Red);
        out.clear();
        let mut lit = World::flat(3, 0);
        scan_column(0, 0, 70, 60, &dark, &mut lit, &mut out);
        assert!(out.is_empty());
        assert_eq!(lit.spent, 11 + 1, "no sky read once the block light fails");
        let old = Params { limit: 7, ..dark };
        scan_column(0, 0, 70, 60, &old, &mut World::flat(3, 0), &mut out);
        assert_eq!(out[0].light, 3);
        out.clear();
        let one_colour = Params {
            split: false,
            ..dark
        };
        let mut world = World::flat(0, 15);
        scan_column(0, 0, 70, 60, &one_colour, &mut world, &mut out);
        assert_eq!(out[0].kind, Kind::Red);
        assert_eq!(world.spent, 11 + 1, "no sky read with split off");
    }

    #[test]
    fn caves_mark_every_floor_and_skip_glass_and_snow_counts() {
        let mut world = World::flat(0, 0);
        // From y 99 down: air to y 71, a snow layer at 70 on stone at 69, air at 68 and 67 on
        // glass at 66, air at 65 and 64, then stone.
        for y in 64..=70 {
            world.layers[(99 - y) as usize] = match y {
                70 => SNOW,
                69 => STONE,
                66 => GLASS,
                _ => AIR,
            };
        }
        let mut out = Vec::new();
        scan_column(0, 0, 80, 50, &Params::default(), &mut world, &mut out);
        let spots: Vec<([i32; 3], f64)> = out.iter().map(|m| (m.pos, m.raise)).collect();
        assert_eq!(spots, [([0, 70, 0], 0.125), ([0, 64, 0], 0.0)]);
    }

    #[test]
    fn an_unloaded_column_stops_at_once() {
        let mut world = World::flat(0, 0);
        world.loaded = 5;
        let mut out = Vec::new();
        scan_column(9, 0, 80, 50, &Params::default(), &mut world, &mut out);
        assert!(out.is_empty());
        assert_eq!(world.spent, 1);
    }

    #[test]
    fn a_column_stays_inside_the_world() {
        let sweep = Sweep::new([0, 3, 0], WORLD);
        assert_eq!(sweep.span(8), (11, 0));
        let sweep = Sweep::new([0, 95, 0], WORLD);
        assert_eq!(sweep.span(8), (99, 86));
        let sweep = Sweep::new([0, 64, 0], WORLD);
        assert_eq!(sweep.span(8), (72, 55));
    }

    #[test]
    fn the_sweep_resumes_where_the_budget_stopped_it() {
        let list = columns(6);
        let p = params(6, 4);
        let mut whole = Sweep::new([10, 64, -10], WORLD);
        let mut all = Vec::new();
        whole.step(&list, &p, u32::MAX, &mut World::flat(0, 15), &mut all);
        assert!(whole.done(&list));
        assert_eq!(all.len(), list.len());

        let mut sweep = Sweep::new([10, 64, -10], WORLD);
        let mut parts = Vec::new();
        let mut world = World::flat(0, 15);
        let mut steps = 0;
        while !sweep.done(&list) {
            let before = sweep.column;
            world.spent = 0;
            sweep.step(&list, &p, 25, &mut world, &mut parts);
            // A column is 10 reads and two light reads: a 25-read budget takes three.
            assert_eq!(sweep.column - before, 3usize.min(list.len() - before));
            steps += 1;
        }
        assert_eq!(steps, list.len().div_ceil(3));
        assert_eq!(parts, all, "the same marks in the same order");
        // Every block read exactly once.
        assert_eq!(world.reads.len(), list.len() * 10);
        assert!(world.reads.values().all(|&n| n == 1));
    }

    #[test]
    fn a_sweep_full_of_marks_ends_early() {
        // Floors and spots on every other block: 16 spots a column at height 16.
        let mut world = World::flat(0, 0);
        for (i, layer) in world.layers.iter_mut().enumerate() {
            *layer = if i % 2 == 0 { AIR } else { STONE };
        }
        let list = columns(24);
        let mut sweep = Sweep::new([0, 50, 0], WORLD);
        let mut out = Vec::new();
        sweep.step(&list, &params(24, 16), u32::MAX, &mut world, &mut out);
        assert!(sweep.done(&list));
        assert!(
            out.len() >= GATHER && out.len() < GATHER + 20,
            "{}",
            out.len()
        );
        finish(&mut out, [0, 50, 0]);
        assert_eq!(out.len(), MAX_MARKS);
    }

    #[test]
    fn finished_marks_run_nearest_first() {
        let mark = |pos| Mark {
            pos,
            kind: Kind::Red,
            light: 0,
            raise: 0.0,
        };
        let mut marks = vec![mark([5, 0, 0]), mark([0, 1, 0]), mark([-3, 0, 0])];
        finish(&mut marks, [0, 0, 0]);
        let order: Vec<[i32; 3]> = marks.iter().map(|m| m.pos).collect();
        assert_eq!(order, [[0, 1, 0], [-3, 0, 0], [5, 0, 0]]);
        assert_eq!(distance2([1, 2, 3], [-1, 0, 3]), 8);
    }

    #[test]
    fn the_marks_keep_up_with_a_running_player_at_the_widest_box() {
        let mut scanner = Scanner::default();
        let p = params(MAX_RADIUS, MAX_HEIGHT);
        let mut world = World::flat(0, 15);
        let mut feet = [0, 64, 0];
        for _ in 0..200 {
            world.spent = 0;
            scanner.tick(feet, WORLD, p, BUDGET, &mut world);
            assert!(world.spent < BUDGET + 40, "{}", world.spent);
            feet[0] += 1;
        }
        assert!(scanner.generation >= 8, "{}", scanner.generation);
        // A spot a column, 1,809 columns: the nearest 1,024 are shown.
        assert_eq!(scanner.shown.len(), MAX_MARKS);
        let origin = scanner.shown[0].pos;
        assert_eq!(origin[1], 64);
        assert!(
            scanner
                .shown
                .windows(2)
                .all(|w| distance2(w[0].pos, origin) <= distance2(w[1].pos, origin))
        );
    }

    #[test]
    fn a_still_player_is_swept_again_every_second() {
        let mut scanner = Scanner::default();
        let p = params(4, 4);
        let mut world = World::flat(0, 15);
        scanner.tick([0, 64, 0], WORLD, p, BUDGET, &mut world);
        assert_eq!(scanner.generation, 1, "a small box is swept in one tick");
        assert_eq!(scanner.shown.len(), columns(4).len());
        for _ in 1..RESTART_TICKS {
            world.spent = 0;
            scanner.tick([0, 64, 0], WORLD, p, BUDGET, &mut world);
            assert_eq!(world.spent, 0, "nothing read while standing still");
        }
        scanner.tick([0, 64, 0], WORLD, p, BUDGET, &mut world);
        assert_eq!(scanner.generation, 2);
        // A step to the next block starts a sweep at once.
        scanner.tick([1, 64, 0], WORLD, p, BUDGET, &mut world);
        assert_eq!(scanner.generation, 3);
        assert!(scanner.shown.iter().any(|m| m.pos == [5, 64, 0]));
    }

    #[test]
    fn a_setting_or_a_teleport_drops_the_sweep_under_way() {
        let mut scanner = Scanner::default();
        let wide = params(MAX_RADIUS, MAX_HEIGHT);
        let mut world = World::flat(0, 15);
        scanner.tick([0, 64, 0], WORLD, wide, BUDGET, &mut world);
        assert_eq!(scanner.sweep.map(|s| s.origin), Some([0, 64, 0]));
        // Walking keeps it.
        scanner.tick([3, 64, 0], WORLD, wide, BUDGET, &mut world);
        assert_eq!(scanner.sweep.map(|s| s.origin), Some([0, 64, 0]));
        // A teleport out of its box starts another from there.
        scanner.tick([500, 64, 0], WORLD, wide, BUDGET, &mut world);
        assert_eq!(scanner.sweep.map(|s| s.origin), Some([500, 64, 0]));
        // So does a changed limit.
        let lit = Params { limit: 7, ..wide };
        scanner.tick([501, 64, 0], WORLD, lit, BUDGET, &mut world);
        assert_eq!(scanner.sweep.map(|s| s.origin), Some([501, 64, 0]));
        assert_eq!(scanner.generation, 0);
        assert!(left_box([0, 64, 0], [25, 64, 0], &wide));
        assert!(!left_box([0, 64, 0], [24, 80, -24], &wide));
        assert!(left_box([0, 64, 0], [0, 81, 0], &wide));
        scanner.clear();
        assert!(scanner.sweep.is_none());
    }

    #[test]
    fn clearing_forgets_the_marks() {
        let mut scanner = Scanner::default();
        let mut world = World::flat(0, 15);
        scanner.tick([0, 64, 0], WORLD, params(4, 4), BUDGET, &mut world);
        assert!(!scanner.shown.is_empty());
        scanner.clear();
        assert!(scanner.shown.is_empty());
        assert_eq!(scanner.generation, 2);
        scanner.clear();
        assert_eq!(scanner.generation, 2, "nothing changed");
    }

    #[test]
    fn a_press_flips_the_marks_once() {
        let mut toggle = Toggle::default();
        assert!(toggle.key(true, 1));
        assert!(toggle.visible);
        // Held: the auto-repeat's clicks change nothing.
        assert!(!toggle.key(true, 1));
        assert!(!toggle.key(true, 2));
        assert!(toggle.visible);
        assert!(!toggle.key(false, 0));
        // A tap inside one tick.
        assert!(toggle.key(false, 1));
        assert!(!toggle.visible);
        assert!(!toggle.key(false, 0));
        assert!(!toggle.visible);
    }

    #[test]
    fn joining_a_world_hides_the_marks_unless_remembered() {
        let mut toggle = Toggle::default();
        toggle.level_loaded(false);
        toggle.key(false, 1);
        assert!(toggle.visible);
        // Another dimension: its level loads before the old one unloads.
        toggle.level_loaded(false);
        toggle.level_unloaded();
        assert!(toggle.visible);
        // Leaving, then joining again.
        toggle.level_unloaded();
        toggle.level_loaded(false);
        assert!(!toggle.visible);
        toggle.key(false, 1);
        toggle.level_unloaded();
        toggle.level_loaded(true);
        assert!(toggle.visible, "remembered");
        // An unload with nothing loaded changes nothing.
        let mut fresh = Toggle::default();
        fresh.level_unloaded();
        fresh.level_loaded(true);
        assert!(!fresh.visible);
    }

    #[test]
    fn colours_pack_under_their_alpha() {
        assert_eq!(argb(0xFF_3030, LINE_ALPHA), 0xFFFF_3030);
        assert_eq!(argb(0xAB12_3456, 0x60), 0x6012_3456);
        assert_eq!(argb(0x12_3456, 0x1FF), 0xFF12_3456);
        let mut look = Look::default();
        assert_eq!(look.color(Kind::Red), 0xFFFF_3030);
        assert_eq!(look.color(Kind::Yellow), 0xFFFF_D030);
        look.style = Style::Square;
        assert_eq!(look.color(Kind::Red), 0x60FF_3030);
    }

    #[test]
    fn settings_read_within_their_ranges() {
        assert_eq!(whole(None, 12, 4, 24), 12);
        assert_eq!(whole(Some(7.4), 12, 4, 24), 7);
        assert_eq!(whole(Some(99.0), 12, 4, 24), 24);
        assert_eq!(whole(Some(f64::NAN), 12, 4, 24), 12);
        assert_eq!(line_width(None), 2.0);
        assert_eq!(line_width(Some(3.5)), 3.5);
        assert_eq!(line_width(Some(0.0)), 1.0);
        assert_eq!(line_width(Some(f64::INFINITY)), 2.0);
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
    }

    #[test]
    fn a_cross_runs_corner_to_corner_inside_the_face() {
        let [a, b] = cross_lines([2, 64, -3], 64.02);
        assert!(close(a[0], [2.15, 64.02, -2.85]) && close(a[1], [2.85, 64.02, -2.15]));
        assert!(close(b[0], [2.85, 64.02, -2.85]) && close(b[1], [2.15, 64.02, -2.15]));
        let corners = square_corners([2, 64, -3], 64.02);
        let expected = [
            [2.1, 64.02, -2.9],
            [2.1, 64.02, -2.1],
            [2.9, 64.02, -2.1],
            [2.9, 64.02, -2.9],
        ];
        for (got, want) in corners.iter().zip(expected) {
            assert!(close(*got, want), "{corners:?}");
        }
        // Each corner shares one coordinate with the next: a square, in order around it.
        for i in 0..4 {
            let (p, q) = (corners[i], corners[(i + 1) % 4]);
            assert!((p[0] == q[0]) != (p[2] == q[2]), "{p:?} {q:?}");
        }
    }

    #[test]
    fn digits_light_their_segments() {
        let counts: Vec<u32> = (0..10).map(|d| digit_segments(d).count_ones()).collect();
        assert_eq!(counts, [6, 2, 5, 5, 4, 5, 6, 3, 7, 6]);
        assert_eq!(digit_segments(8), 0x7F);
        assert_eq!(digit_segments(1), 0b0000110, "the two right segments");
        assert_eq!(digit_segments(7), 0b0000111);
        assert_eq!(digit_segments(0) & 0x40, 0, "0 has no middle bar");
        assert_eq!(digit_segments(10), 0);
        assert_eq!(split_digits(7), ([7, 0], 1));
        assert_eq!(split_digits(15), ([1, 5], 2));
        assert_eq!(split_digits(200), ([9, 9], 2));
    }

    #[test]
    fn numbers_stay_on_their_face() {
        for value in 0..=15u8 {
            for quadrant in 0..4 {
                let mut lines = Vec::new();
                number_lines(value, [10.0, 64.02, -4.0], quadrant, |a, b| {
                    lines.push([a, b])
                });
                let (digits, count) = split_digits(value);
                let lit: u32 = digits[..count]
                    .iter()
                    .map(|&d| digit_segments(d).count_ones())
                    .sum();
                assert_eq!(lines.len(), lit as usize);
                assert!(lines.len() <= 14);
                for p in lines.iter().flatten() {
                    assert!(p[0] > 10.1 && p[0] < 10.9, "{value} {quadrant} {p:?}");
                    assert!(p[2] > -3.9 && p[2] < -3.1, "{value} {quadrant} {p:?}");
                    assert_eq!(p[1], 64.02);
                }
                // Two digits spread wider than one, across the player's right hand.
                let [rx, rz] = right(quadrant);
                let across: Vec<f64> = lines
                    .iter()
                    .flatten()
                    .map(|p| (p[0] - 10.5) * rx + (p[2] + 3.5) * rz)
                    .collect();
                let spread = across.iter().cloned().fold(f64::MIN, f64::max)
                    - across.iter().cloned().fold(f64::MAX, f64::min);
                if count == 2 {
                    // 10 to 15 start with a 1, drawn on its digit's right edge only.
                    assert!(spread > DIGIT_WIDTH + 0.05, "{value} {spread}");
                } else if value != 1 {
                    assert!((spread - DIGIT_WIDTH).abs() < 1e-9, "{value} {spread}");
                }
            }
        }
    }

    #[test]
    fn numbers_read_upright_for_the_player() {
        // Facing south (+z), a 1's two segments sit on the player's right (-x) and run along z.
        let mut lines = Vec::new();
        number_lines(1, [0.0, 0.0, 0.0], 0, |a, b| lines.push([a, b]));
        assert_eq!(lines.len(), 2);
        for [a, b] in &lines {
            assert!((a[0] - 0.375).abs() < 1e-9 && (b[0] - 0.375).abs() < 1e-9);
            assert!(a[2] != b[2]);
        }
        // A 7's top bar lies away from the player: farthest along the way they face.
        for quadrant in 0..4 {
            let mut lines = Vec::new();
            number_lines(7, [0.0, 0.0, 0.0], quadrant, |a, b| lines.push([a, b]));
            let [fx, fz] = forward(quadrant);
            let along = |p: [f64; 3]| (p[0] - 0.5) * fx + (p[2] - 0.5) * fz;
            let top = lines[0];
            assert!((along(top[0]) - 0.25).abs() < 1e-9, "{quadrant}");
            assert!((along(top[1]) - 0.25).abs() < 1e-9, "{quadrant}");
        }
        assert_eq!(right(0), [-1.0, 0.0]);
        assert_eq!(right(3), [0.0, 1.0]);
    }

    #[test]
    fn the_yaw_turns_to_the_nearest_quarter() {
        assert_eq!(quadrant(0.0), 0);
        assert_eq!(quadrant(44.0), 0);
        assert_eq!(quadrant(46.0), 1);
        assert_eq!(quadrant(90.0), 1);
        assert_eq!(quadrant(180.0), 2);
        assert_eq!(quadrant(-90.0), 3);
        assert_eq!(quadrant(270.0), 3);
        assert_eq!(quadrant(-180.0), 2);
        assert_eq!(quadrant(725.0), 0);
        assert_eq!(quadrant(f32::NAN), 0);
    }

    #[test]
    fn only_whole_marks_that_fit_are_submitted() {
        assert_eq!(fit(&[], 100), 0);
        assert_eq!(fit(&[2, 4, 6], 100), 6);
        assert_eq!(fit(&[2, 4, 6], 6), 6);
        assert_eq!(fit(&[2, 4, 6], 5), 4);
        assert_eq!(fit(&[2, 4, 6], 1), 0);
        assert_eq!(fit(&[7, 9, 16], 15), 9);
        assert_eq!(fit(&[2, 4, 6], 0), 0);
    }

    #[test]
    fn the_picture_holds_each_style() {
        let marks: Vec<Mark> = (0..2000)
            .map(|i| Mark {
                pos: [i, 64, 0],
                kind: if i % 2 == 0 { Kind::Red } else { Kind::Yellow },
                light: (i % 16) as u8,
                raise: 0.0,
            })
            .collect();
        let mut picture = Picture::default();
        let mut look = Look::default();
        picture.build(&marks, &look, 0);
        assert_eq!(picture.lines.len(), 2 * MAX_MARKS);
        assert_eq!(picture.ends.len(), MAX_MARKS);
        assert_eq!(picture.ends[..3], [2, 4, 6]);
        assert!(picture.quads.is_empty());
        let first = picture.lines[0];
        assert_eq!(first.color, 0xFFFF_3030);
        assert_eq!(first.width, 2.0);
        assert_eq!(first.depth, WorldDepth::Tested);
        assert!((first.from[1] - 64.02).abs() < 1e-12);
        assert_eq!(picture.lines[2].color, 0xFFFF_D030);

        look.style = Style::Square;
        picture.build(&marks, &look, 0);
        assert!(picture.lines.is_empty());
        assert_eq!(picture.quads.len(), MAX_MARKS);
        assert_eq!(picture.quads[0].color, 0x60FF_3030);
        assert_eq!(picture.quads[0].depth, WorldDepth::Tested);

        look.style = Style::Number;
        look.width = 1.5;
        picture.build(&marks, &look, 2);
        assert_eq!(picture.ends.len(), MAX_NUMBER_MARKS);
        assert_eq!(*picture.ends.last().unwrap() as usize, picture.lines.len());
        assert!(picture.lines.len() <= 14 * MAX_NUMBER_MARKS);
        assert!(picture.lines.iter().all(|l| l.width == 1.5));
        // Light 0 at the first mark: a 0's six segments.
        assert_eq!(picture.ends[0], 6);
        for pair in picture.ends.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }
}
