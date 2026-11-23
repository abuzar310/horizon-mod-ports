//! Pickup Log: a short list in a corner of the screen of what you just picked up, each line the
//! item's icon and `+12 Cobblestone`. Picking up more of an item adds to its line and starts its
//! time again; a line stays a few seconds, then fades and slides out. The newest line is the one
//! nearest the edge of the screen.
//!
//! How a pickup is told apart from any other change of the inventory: the server sends
//! `minecraft:take_item_entity` when an item on the ground is picked up (the packet carries no
//! fields the mod API passes on, only its type). A gain in the inventory within half a second of
//! one is a pickup; a gain first and the packet up to two ticks later is one too, so the order the
//! two arrive in does not matter. Counts are compared per item kind over the main inventory, the
//! off hand and the armour, so moving or equipping an item nets to nothing. Nothing is listed
//! while a screen is open: there the inventory changes by hand.
use std::sync::{Mutex, MutexGuard};

use mod_api::{
    ClientHost, ClientLevelLoadEvent, ClientPacketEvent, ClientTickPostEvent, Corner,
    EquipmentSlot, Hooks, ItemKey, ItemStack, Registrar, RegistrationContext, RenderGuiPostEvent,
    SettingSpec,
};

/// The mod's id: its settings folder and translation keys.
pub const ID: &str = "pickup_log";
/// The corners in the order the corner setting lists them.
pub const CORNERS: [Corner; 4] = Corner::ALL;
/// The slots counted besides the 36 of the main inventory: the off hand and the armour.
pub const WORN: [EquipmentSlot; 5] = [
    EquipmentSlot::OffHand,
    EquipmentSlot::Head,
    EquipmentSlot::Chest,
    EquipmentSlot::Legs,
    EquipmentSlot::Feet,
];
/// For how many client ticks after a take packet a gain counts as a pickup: half a second.
pub const TAKE_WINDOW: u64 = 10;
/// For how many client ticks a gain waits for a take packet before it is dropped.
pub const PENDING_TICKS: u64 = 2;
/// For how many client ticks after joining or changing world a gain with no take packet is
/// never listed, even with every new item on: the inventory is still arriving.
pub const SETTLE_TICKS: u64 = 10;
/// How long a line takes to fade out, in milliseconds.
pub const FADE_MS: u64 = 500;
/// The most lines the list can be set to.
pub const MAX_ROWS: usize = 10;
/// The WebAssembly mods' panel, as Coordinates HUD draws it: translucent black, white text.
pub const BACKGROUND: u32 = 0x7800_0000;
pub const INK: u32 = 0xFFFF_FFFF;
/// An item icon's size and its inset in its line's box, in GUI pixels.
pub const ICON: f32 = 16.0;
pub const ICON_INSET: f32 = 1.0;
/// The space between an icon and its text.
pub const ICON_GAP: f32 = 2.0;
/// A text line's height and the text's inset, in GUI pixels (a line without an icon).
pub const LINE: f32 = 10.0;
pub const PADDING: f32 = 2.0;
/// The font's glyph height, for centring text beside an icon.
pub const GLYPH: f32 = 8.0;
/// The space between two lines of the list.
pub const ROW_GAP: f32 = 1.0;
/// The gap the game leaves between a corner's boxes and the screen's edge.
pub const MARGIN: f32 = 2.0;

/// The settings on Pickup Log's page in Options > Mods.
pub fn settings() -> [SettingSpec; 6] {
    [
        SettingSpec::choice(
            "corner",
            "pickup_log.setting.corner",
            CORNERS.map(|c| format!("pickup_log.corner.{}", c.name())),
            3,
        )
        .tooltip("pickup_log.setting.corner.tooltip"),
        SettingSpec::range("seconds", "pickup_log.setting.seconds", 1.0, 10.0, 1.0, 4.0)
            .tooltip("pickup_log.setting.seconds.tooltip"),
        SettingSpec::range(
            "max_entries",
            "pickup_log.setting.max_entries",
            1.0,
            10.0,
            1.0,
            6.0,
        )
        .tooltip("pickup_log.setting.max_entries.tooltip"),
        SettingSpec::toggle("icons", "pickup_log.setting.icons", true)
            .tooltip("pickup_log.setting.icons.tooltip"),
        SettingSpec::toggle("background", "pickup_log.setting.background", true)
            .tooltip("pickup_log.setting.background.tooltip"),
        SettingSpec::toggle("all_gains", "pickup_log.setting.all_gains", false)
            .tooltip("pickup_log.setting.all_gains.tooltip"),
    ]
}

/// A 1 to 10 range setting as a whole number: `default` when it can't be read.
pub fn whole_setting(value: Option<f64>, default: u64) -> u64 {
    value
        .filter(|v| v.is_finite())
        .map_or(default, |v| v.round().clamp(1.0, 10.0) as u64)
}

/// How long a line stays before it fades, in milliseconds, from the seconds setting.
pub fn life_ms(seconds: Option<f64>) -> u64 {
    whole_setting(seconds, 4) * 1000
}

/// The most lines shown, from the max_entries setting.
pub fn max_entries(value: Option<f64>) -> usize {
    whole_setting(value, 6) as usize
}

/// How many of each kind `stacks` (kind, count) hold, kinds in the order they first appear.
pub fn tally<K: Copy + Eq>(stacks: impl IntoIterator<Item = (K, u32)>) -> Vec<(K, u64)> {
    let mut out: Vec<(K, u64)> = Vec::new();
    for (key, count) in stacks {
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, total)) => *total += u64::from(count),
            None => out.push((key, u64::from(count))),
        }
    }
    out
}

/// What `after` holds more of than `before`, per kind: a kind new in `after` counts fully, a
/// kind there is less of (or no more of) is left out.
pub fn gains<K: Copy + Eq>(before: &[(K, u64)], after: &[(K, u64)]) -> Vec<(K, u64)> {
    after
        .iter()
        .filter_map(|&(key, now)| {
            let was = before
                .iter()
                .find(|(k, _)| *k == key)
                .map_or(0, |(_, n)| *n);
            (now > was).then_some((key, now - was))
        })
        .collect()
}

/// The stack of a kind (`kind` says which) that changed between two reads: the first in `after`
/// with no equal one in `before`, else the first of the kind. None when `after` has none.
pub fn newest<'a, T: PartialEq>(
    before: &[T],
    after: &'a [T],
    kind: impl Fn(&T) -> bool,
) -> Option<&'a T> {
    let mut first = None;
    for stack in after.iter().filter(|s| kind(s)) {
        if !before.contains(stack) {
            return Some(stack);
        }
        if first.is_none() {
            first = Some(stack);
        }
    }
    first
}

/// Whether a server packet's type is `minecraft:take_item_entity`.
pub fn is_take(namespace: &str, path: &str) -> bool {
    namespace == "minecraft" && path == "take_item_entity"
}

/// Tells pickups from other gains: a gain within [`TAKE_WINDOW`] ticks after a take packet is
/// one; another waits [`PENDING_TICKS`] ticks for a take packet and is dropped without one.
/// Ticks are counted by the client tick hook; a packet heard after tick `t` is heard "at" `t`.
#[derive(Debug)]
pub struct Detector<G> {
    /// The last tick whose gains the last take packet explains.
    take_until: Option<u64>,
    /// The tick the last take packet was heard at.
    last_take: Option<u64>,
    /// Up to this tick a gain with no take packet is never listed.
    settle_until: u64,
    /// Gains waiting for a take packet, with the tick they were seen at.
    pending: Vec<(u64, G)>,
}

impl<G> Default for Detector<G> {
    fn default() -> Self {
        Self {
            take_until: None,
            last_take: None,
            settle_until: 0,
            pending: Vec::new(),
        }
    }
}

impl<G> Detector<G> {
    /// A take packet was heard after client tick `tick`.
    pub fn take(&mut self, tick: u64) {
        self.take_until = Some(tick + TAKE_WINDOW);
        self.last_take = Some(tick);
    }

    /// The inventory is read afresh from `tick` (no earlier read, a new world, the mod turned
    /// on): the gains waiting are dropped, and for [`SETTLE_TICKS`] none without a take packet
    /// is listed, while the inventory is still arriving.
    pub fn reset(&mut self, tick: u64) {
        self.pending.clear();
        self.settle_until = self.settle_until.max(tick + SETTLE_TICKS);
    }

    /// A screen is open: the gains waiting are dropped (the inventory changes by hand there).
    /// No settling time follows, so `/give` typed in chat is listed with every new item on.
    pub fn drop_pending(&mut self) {
        self.pending.clear();
    }

    /// How many gains wait for a take packet.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Client tick `tick` found `gains` in the inventory. Appends to `out` the gains to list now:
    /// these if a take packet came shortly before, or every new one with `all_gains` (outside
    /// the settling time), and earlier ones a take packet has since explained.
    pub fn step(&mut self, tick: u64, gains: Vec<G>, all_gains: bool, out: &mut Vec<G>) {
        let last_take = self.last_take;
        let mut i = 0;
        while i < self.pending.len() {
            let seen = self.pending[i].0;
            if last_take.is_some_and(|t| t >= seen && t <= seen + PENDING_TICKS) {
                out.push(self.pending.remove(i).1);
            } else if seen + PENDING_TICKS < tick {
                // A packet heard from now on is heard at `tick` or later: too late for it.
                self.pending.remove(i);
            } else {
                i += 1;
            }
        }
        let explained = self.take_until.is_some_and(|until| tick <= until);
        let unexplained_listed = all_gains && tick > self.settle_until;
        for gain in gains {
            if explained || unexplained_listed {
                out.push(gain);
            } else {
                self.pending.push((tick, gain));
            }
        }
    }
}

/// `+12 Cobblestone`.
pub fn format_entry(count: u64, name: &str) -> String {
    format!("+{count} {name}")
}

/// One line of the list: an item kind and how many of it were picked up.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<K, S> {
    pub key: K,
    pub count: u64,
    /// The item's name, from its newest stack.
    pub name: String,
    /// Its newest stack, for the icon.
    pub stack: S,
    /// What the line says: [`format_entry`] of the count and the name.
    pub text: String,
    /// When the line was last added to (the game's clock, in milliseconds).
    pub since: u64,
    /// The text's width as last measured, until the text changes.
    pub width: Option<f32>,
}

/// The lines, oldest first.
#[derive(Debug)]
pub struct Log<K, S> {
    entries: Vec<Entry<K, S>>,
}

impl<K, S> Default for Log<K, S> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<K: Copy + Eq, S> Log<K, S> {
    /// `amount` more of `key` picked up at `now`: added to its line if it has one, which becomes
    /// the newest and starts its time again, else a new line; past `max` lines the oldest go.
    pub fn add(&mut self, key: K, amount: u64, name: String, stack: S, now: u64, max: usize) {
        let mut entry = match self.entries.iter().position(|e| e.key == key) {
            Some(i) => {
                let mut entry = self.entries.remove(i);
                entry.count = entry.count.saturating_add(amount);
                entry.name = name;
                entry.stack = stack;
                entry
            }
            None => Entry {
                key,
                count: amount,
                name,
                stack,
                text: String::new(),
                since: now,
                width: None,
            },
        };
        entry.since = now;
        entry.text = format_entry(entry.count, &entry.name);
        entry.width = None;
        self.entries.push(entry);
        self.trim(max);
    }

    /// Keeps the newest `max` lines.
    pub fn trim(&mut self, max: usize) {
        let extra = self.entries.len().saturating_sub(max);
        self.entries.drain(..extra);
    }

    /// Drops the lines that have faded out by `now`: older than `life` plus [`FADE_MS`].
    pub fn expire(&mut self, now: u64, life: u64) {
        self.entries
            .retain(|e| now.saturating_sub(e.since) < life + FADE_MS);
    }

    /// Drops every line.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The lines, oldest first.
    pub fn entries(&self) -> &[Entry<K, S>] {
        &self.entries
    }

    /// The lines, oldest first, to note their widths.
    pub fn entries_mut(&mut self) -> &mut [Entry<K, S>] {
        &mut self.entries
    }
}

/// How opaque a line `age` milliseconds old is: whole for `life`, then fading to nothing over
/// `fade`.
pub fn alpha(age: u64, life: u64, fade: u64) -> f32 {
    if age < life {
        1.0
    } else if age >= life + fade {
        0.0
    } else {
        1.0 - (age - life) as f32 / fade as f32
    }
}

/// `argb` with its alpha scaled by `alpha` (0 to 1).
pub fn faded(argb: u32, alpha: f32) -> u32 {
    let a = ((argb >> 24) as f32 * alpha.clamp(0.0, 1.0)).round() as u32;
    (a << 24) | (argb & 0x00FF_FFFF)
}

/// A line's box around text `text_width` wide, `[w, h]`: with room for the icon, or not.
pub fn row_size(text_width: f32, icons: bool) -> [f32; 2] {
    if icons {
        [
            ICON_INSET + ICON + ICON_GAP + text_width + PADDING,
            ICON + 2.0 * ICON_INSET,
        ]
    } else {
        [PADDING + text_width + PADDING, LINE + PADDING]
    }
}

/// The box the whole list takes, `[w, h]`: as wide as its widest line, its lines `row_height`
/// high one above the other.
pub fn list_size(row_widths: &[f32], row_height: f32) -> [f32; 2] {
    let rows = row_widths.len() as f32;
    let width = row_widths.iter().copied().fold(0.0, f32::max);
    let height = if row_widths.is_empty() {
        0.0
    } else {
        rows * row_height + (rows - 1.0) * ROW_GAP
    };
    [width, height]
}

/// The top left of line `index` (0 the newest) of a list in `corner` whose box is at `at`, `size`
/// big: the newest line nearest the corner's edge, each line against the corner's side.
pub fn row_origin(
    corner: Corner,
    at: [f32; 2],
    size: [f32; 2],
    index: usize,
    row: [f32; 2],
) -> [f32; 2] {
    let x = if corner.left() {
        at[0]
    } else {
        at[0] + size[0] - row[0]
    };
    let step = index as f32 * (row[1] + ROW_GAP);
    let y = if corner.top() {
        at[1] + step
    } else {
        at[1] + size[1] - row[1] - step
    };
    [x, y]
}

/// How far a line `width` wide has slid towards the screen's edge at opacity `alpha`: none
/// while whole, all the way out once faded.
pub fn slide(corner: Corner, width: f32, alpha: f32) -> f32 {
    let distance = (1.0 - alpha.clamp(0.0, 1.0)) * (width + MARGIN);
    if corner.left() { -distance } else { distance }
}

/// Where a line's icon and text go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowParts {
    pub icon: Option<[f32; 2]>,
    pub text: [f32; 2],
}

/// The icon and text of a line whose box is at `origin`, `row` big: the icon on the side of the
/// screen's edge, so the icons line up along it.
pub fn row_parts(corner: Corner, origin: [f32; 2], row: [f32; 2], icons: bool) -> RowParts {
    let [x, y] = origin;
    if !icons {
        return RowParts {
            icon: None,
            text: [x + PADDING, y + PADDING],
        };
    }
    let text_y = y + ((row[1] - GLYPH) / 2.0).floor();
    let icon_y = y + ICON_INSET;
    if corner.left() {
        RowParts {
            icon: Some([x + ICON_INSET, icon_y]),
            text: [x + ICON_INSET + ICON + ICON_GAP, text_y],
        }
    } else {
        RowParts {
            icon: Some([x + row[0] - ICON_INSET - ICON, icon_y]),
            text: [x + PADDING, text_y],
        }
    }
}

/// A gain of an item kind found in the inventory, with the stack that grew.
#[derive(Debug)]
struct Gain {
    key: ItemKey,
    amount: u64,
    stack: ItemStack,
}

/// One read of the inventory: its stacks and the count of each kind.
struct Snapshot {
    stacks: Vec<ItemStack>,
    counts: Vec<(ItemKey, u64)>,
}

#[derive(Default)]
struct State {
    /// Client ticks so far.
    tick: u64,
    /// The last read of the inventory gains are measured from; None until one is taken.
    baseline: Option<Snapshot>,
    detector: Detector<Gain>,
    log: Log<ItemKey, ItemStack>,
}

impl State {
    /// Out of a world, or the mod turned off: everything starts again.
    fn forget(&mut self, tick: u64) {
        self.baseline = None;
        self.detector.reset(tick);
        self.log.clear();
    }
}

#[derive(Default)]
struct PickupLog {
    state: Mutex<State>,
}

impl PickupLog {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[mod_api::horizon_mod]
impl Hooks for PickupLog {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// A take packet opens the window gains count as pickups in. Nothing else is read, and
    /// nothing allocated: every server packet comes through here.
    fn client_packet(&self, _: &mut dyn ClientHost, event: &mut ClientPacketEvent<'_>) {
        let id = &event.packet.id;
        if is_take(id.namespace(), id.path()) {
            let mut state = self.state();
            let tick = state.tick;
            state.detector.take(tick);
        }
    }

    /// A new level (joining, another dimension): the inventory is read afresh.
    fn client_level_load(&self, _: &mut dyn ClientHost, _: &mut ClientLevelLoadEvent) {
        let mut state = self.state();
        let tick = state.tick;
        state.baseline = None;
        state.detector.reset(tick);
    }

    /// Reads the inventory once a client tick and lists the pickups since the last read.
    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        let view: &dyn ClientHost = &*host;
        let mut state = self.state();
        state.tick += 1;
        let tick = state.tick;
        let player = if view.on(ID) {
            view.local_player()
        } else {
            None
        };
        let Some(player) = player else {
            state.forget(tick);
            return;
        };
        let now = view.millis();
        let max = max_entries(view.setting_f64("max_entries"));
        state.log.expire(now, life_ms(view.setting_f64("seconds")));
        state.log.trim(max);
        // The main inventory, then the off hand and the armour.
        let read = player.inventory(view).and_then(|mut stacks| {
            let living = view.living(player.entity())?;
            stacks.extend(
                WORN.into_iter()
                    .filter_map(|slot| living.equipment(view, slot)),
            );
            Some(stacks)
        });
        let Some(stacks) = read else {
            state.baseline = None;
            state.detector.reset(tick);
            return;
        };
        // By item kind only: components (damage, a name) changing is no pickup.
        let counts = tally(stacks.iter().map(|s| (s.item(), s.count())));
        let screen_open = view.observed_screen_id().is_some();
        match state.baseline.take() {
            Some(before) if !screen_open => {
                let found: Vec<Gain> = gains(&before.counts, &counts)
                    .into_iter()
                    .filter_map(|(key, amount)| {
                        let stack = newest(&before.stacks, &stacks, |s| s.item() == key)?;
                        Some(Gain {
                            key,
                            amount,
                            stack: stack.clone(),
                        })
                    })
                    .collect();
                let all_gains = view.setting_bool("all_gains").unwrap_or(false);
                let mut logged = Vec::new();
                state.detector.step(tick, found, all_gains, &mut logged);
                for gain in logged {
                    let name = view.hover_name(&gain.stack);
                    state
                        .log
                        .add(gain.key, gain.amount, name, gain.stack, now, max);
                }
            }
            // A screen open: this read is only the new baseline.
            Some(_) => state.detector.drop_pending(),
            // No read yet: this read is the first baseline.
            None => state.detector.reset(tick),
        }
        state.baseline = Some(Snapshot { stacks, counts });
    }

    /// The list in the corner picked, below what the corner holds already; none while F3's
    /// debug screen takes that corner.
    fn render_gui_post(&self, host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
        let mut state = self.state();
        if state.log.entries().is_empty() || !host.on(ID) {
            return;
        }
        let corner = host
            .setting_choice("corner")
            .map_or(3, |i| i as usize)
            .min(CORNERS.len() - 1);
        let corner = CORNERS[corner];
        if !host.native_corner_shown(corner) {
            return;
        }
        let now = host.millis();
        let life = life_ms(host.setting_f64("seconds"));
        let icons = host.setting_bool("icons").unwrap_or(true);
        let background = host.setting_bool("background").unwrap_or(true);
        let draw = &mut *event.draw;
        // Every measurement before the first drawing, newest line first. Newer lines are
        // younger, so the first faded one ends the list.
        let mut widths = [0.0f32; MAX_ROWS];
        let mut alphas = [0.0f32; MAX_ROWS];
        let mut shown = 0;
        for entry in state.log.entries_mut().iter_mut().rev().take(MAX_ROWS) {
            let a = alpha(now.saturating_sub(entry.since), life, FADE_MS);
            if a <= 0.0 {
                break;
            }
            let text_width = match entry.width {
                Some(width) => width,
                None => {
                    let width = draw.text_width(&entry.text);
                    entry.width = Some(width);
                    width
                }
            };
            widths[shown] = row_size(text_width, icons)[0];
            alphas[shown] = a;
            shown += 1;
        }
        if shown == 0 {
            return;
        }
        let row_height = row_size(0.0, icons)[1];
        let size = list_size(&widths[..shown], row_height);
        let (x, y) = event.corners.place(corner, size[0], size[1]);
        for (i, entry) in state.log.entries().iter().rev().take(shown).enumerate() {
            let row = [widths[i], row_height];
            let mut origin = row_origin(corner, [x, y], size, i, row);
            origin[0] += slide(corner, row[0], alphas[i]);
            if background {
                draw.fill(
                    [origin[0], origin[1], row[0], row[1]],
                    faded(BACKGROUND, alphas[i]),
                );
            }
            let parts = row_parts(corner, origin, row, icons);
            if let Some(at) = parts.icon {
                // False only for an unknown item: the line still shows its text.
                let _ = draw.item_stack(&entry.stack, at, ICON);
            }
            let ink = faded(INK, alphas[i]);
            // Text all but transparent is left out: older games drew it opaque.
            if ink >> 24 >= 4 {
                draw.text(&entry.text, parts.text, ink, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_is_one_the_game_takes() {
        let specs = settings();
        for setting in &specs {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
        let keys: Vec<&str> = specs.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "corner",
                "seconds",
                "max_entries",
                "icons",
                "background",
                "all_gains"
            ]
        );
        assert_eq!(CORNERS[3], Corner::BottomRight);
    }

    #[test]
    fn every_translation_key_is_in_the_language_file() {
        let lang = include_str!("../assets/pickup_log/lang/en_us.json");
        let mut keys: Vec<String> = CORNERS
            .iter()
            .map(|c| format!("pickup_log.corner.{}", c.name()))
            .collect();
        for setting in settings() {
            keys.push(setting.label.clone());
            keys.extend(setting.tooltip.clone());
        }
        for key in keys {
            assert!(lang.contains(&format!("\"{key}\":")), "{key}");
        }
    }

    #[test]
    fn range_settings_read_as_whole_numbers() {
        assert_eq!(life_ms(Some(4.0)), 4000);
        assert_eq!(life_ms(Some(1.4)), 1000);
        assert_eq!(life_ms(Some(99.0)), 10_000);
        assert_eq!(life_ms(None), 4000);
        assert_eq!(life_ms(Some(f64::NAN)), 4000);
        assert_eq!(max_entries(Some(6.0)), 6);
        assert_eq!(max_entries(Some(0.0)), 1);
        assert_eq!(max_entries(None), 6);
    }

    #[test]
    fn tally_merges_stacks_of_a_kind() {
        let counts = tally([("dirt", 64), ("stone", 12), ("dirt", 5), ("dirt", 1)]);
        assert_eq!(counts, [("dirt", 70), ("stone", 12)]);
        assert!(tally::<u32>([]).is_empty());
        // Counts add up past one u32.
        assert_eq!(
            tally([(1, u32::MAX), (1, 1)]),
            [(1, u64::from(u32::MAX) + 1)]
        );
    }

    #[test]
    fn gains_are_only_increases() {
        let before = [("dirt", 70), ("stone", 12), ("sand", 3)];
        let after = [("dirt", 64), ("stone", 20), ("sand", 3), ("apple", 2)];
        // Less dirt is ignored, unchanged sand too; new apples count fully.
        assert_eq!(gains(&before, &after), [("stone", 8), ("apple", 2)]);
        assert!(gains(&after, &after).is_empty());
        assert_eq!(gains(&[], &[("dirt", 1)]), [("dirt", 1)]);
        assert!(gains(&before, &[]).is_empty());
    }

    #[test]
    fn swapping_or_equipping_nets_to_nothing() {
        // A helmet moves from the main inventory to the head slot: the tally sees the same kinds.
        let before = tally([("helmet", 1), ("dirt", 5)]);
        let after = tally([("dirt", 5), ("helmet", 1)]);
        assert!(gains(&before, &after).is_empty());
    }

    #[test]
    fn the_stack_that_grew_names_the_line() {
        let before = [("a", 3), ("b", 1), ("a", 64)];
        let after = [("a", 3), ("b", 1), ("a", 64), ("a", 5)];
        assert_eq!(newest(&before, &after, |s| s.0 == "a"), Some(&("a", 5)));
        // A stack that grew in place.
        let after = [("a", 4), ("b", 1), ("a", 64)];
        assert_eq!(newest(&before, &after, |s| s.0 == "a"), Some(&("a", 4)));
        // Nothing changed: the first of the kind.
        assert_eq!(newest(&before, &before, |s| s.0 == "a"), Some(&("a", 3)));
        assert_eq!(newest(&before, &before, |s| s.0 == "z"), None);
    }

    #[test]
    fn only_the_take_packet_counts() {
        assert!(is_take("minecraft", "take_item_entity"));
        assert!(!is_take("minecraft", "set_entity_data"));
        assert!(!is_take("mymod", "take_item_entity"));
    }

    /// Runs `step` at `tick` and returns what it lists.
    fn step(d: &mut Detector<char>, tick: u64, gains: &[char], all: bool) -> Vec<char> {
        let mut out = Vec::new();
        d.step(tick, gains.to_vec(), all, &mut out);
        out
    }

    #[test]
    fn a_take_then_a_gain_is_listed() {
        let mut d = Detector::default();
        d.take(100);
        assert_eq!(step(&mut d, 101, &['a'], false), ['a']);
        // More gains within the window are listed too.
        assert_eq!(step(&mut d, 105, &['b'], false), ['b']);
        assert_eq!(step(&mut d, 110, &['c'], false), ['c']);
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn a_gain_then_a_take_within_two_ticks_is_listed() {
        let mut d = Detector::default();
        assert!(step(&mut d, 100, &['a'], false).is_empty());
        assert_eq!(d.pending(), 1);
        d.take(100);
        assert_eq!(step(&mut d, 101, &[], false), ['a']);

        // The packet two ticks late still counts.
        let mut d = Detector::default();
        assert!(step(&mut d, 100, &['a'], false).is_empty());
        assert!(step(&mut d, 101, &[], false).is_empty());
        assert!(step(&mut d, 102, &[], false).is_empty());
        d.take(102);
        assert_eq!(step(&mut d, 103, &[], false), ['a']);
    }

    #[test]
    fn a_gain_without_a_take_is_dropped() {
        let mut d = Detector::default();
        assert!(step(&mut d, 100, &['a'], false).is_empty());
        assert!(step(&mut d, 101, &[], false).is_empty());
        assert!(step(&mut d, 102, &[], false).is_empty());
        assert!(step(&mut d, 103, &[], false).is_empty());
        assert_eq!(d.pending(), 0);
        // A take packet too late lists nothing old.
        d.take(103);
        assert!(step(&mut d, 104, &[], false).is_empty());
    }

    #[test]
    fn the_take_window_expires() {
        let mut d = Detector::default();
        d.take(10);
        assert_eq!(step(&mut d, 20, &['a'], false), ['a']);
        assert!(step(&mut d, 21, &['b'], false).is_empty());
        for tick in 22..30 {
            assert!(step(&mut d, tick, &[], false).is_empty());
        }
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn every_new_item_lists_gains_without_a_take_once_settled() {
        let mut d = Detector::default();
        d.reset(0);
        // Just after joining, the inventory arriving is no pickup.
        assert!(step(&mut d, 5, &['a'], true).is_empty());
        assert!(step(&mut d, 8, &[], true).is_empty());
        assert_eq!(d.pending(), 0);
        assert_eq!(step(&mut d, 11, &['b'], true), ['b']);
        // Off, the same gain waits for a packet.
        assert!(step(&mut d, 12, &['c'], false).is_empty());
    }

    #[test]
    fn a_give_typed_in_chat_is_listed_once_chat_closes() {
        let mut d = Detector::default();
        d.reset(0);
        assert!(step(&mut d, 50, &['x'], false).is_empty());
        assert_eq!(d.pending(), 1);
        // Chat open from tick 51 to 60: only baselines, the waiting gain dropped.
        for _ in 51..=60 {
            d.drop_pending();
        }
        assert_eq!(d.pending(), 0);
        // The items of `/give` land two ticks after chat closed: listed, no settling time.
        assert_eq!(step(&mut d, 62, &['g'], true), ['g']);
        // Without every new item on, they wait for a take packet and are dropped.
        assert!(step(&mut d, 63, &['h'], false).is_empty());
        d.drop_pending();
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn a_reset_drops_waiting_gains() {
        let mut d = Detector::default();
        assert!(step(&mut d, 100, &['a'], false).is_empty());
        d.reset(100);
        d.take(100);
        assert!(step(&mut d, 101, &[], false).is_empty());
    }

    #[test]
    fn the_line_reads_plus_count_and_name() {
        assert_eq!(format_entry(12, "Cobblestone"), "+12 Cobblestone");
        assert_eq!(format_entry(1, "Diamond Sword"), "+1 Diamond Sword");
    }

    #[test]
    fn the_log_merges_and_starts_the_time_again() {
        let mut log: Log<u32, ()> = Log::default();
        log.add(1, 12, "Cobblestone".into(), (), 1000, 6);
        log.add(2, 1, "Apple".into(), (), 2000, 6);
        log.add(1, 5, "Cobblestone".into(), (), 3000, 6);
        let entries = log.entries();
        assert_eq!(entries.len(), 2);
        // The merged line is now the newest, with the new time.
        assert_eq!(entries[1].key, 1);
        assert_eq!(entries[1].count, 17);
        assert_eq!(entries[1].text, "+17 Cobblestone");
        assert_eq!(entries[1].since, 3000);
        assert_eq!(entries[0].text, "+1 Apple");
    }

    #[test]
    fn a_new_name_replaces_the_old_and_the_width_is_measured_again() {
        let mut log: Log<u32, &str> = Log::default();
        log.add(1, 1, "Diamond Sword".into(), "plain", 0, 6);
        log.entries_mut()[0].width = Some(80.0);
        log.add(1, 1, "Excalibur".into(), "named", 10, 6);
        let entry = &log.entries()[0];
        assert_eq!(entry.text, "+2 Excalibur");
        assert_eq!(entry.stack, "named");
        assert_eq!(entry.width, None);
    }

    #[test]
    fn the_oldest_lines_go_past_the_most() {
        let mut log: Log<u32, ()> = Log::default();
        for key in 0..5 {
            log.add(key, 1, format!("Item {key}"), (), u64::from(key), 3);
        }
        let keys: Vec<u32> = log.entries().iter().map(|e| e.key).collect();
        assert_eq!(keys, [2, 3, 4]);
        log.trim(1);
        assert_eq!(log.entries().len(), 1);
        assert_eq!(log.entries()[0].key, 4);
    }

    #[test]
    fn lines_expire_after_their_time_and_the_fade() {
        let mut log: Log<u32, ()> = Log::default();
        log.add(1, 1, "A".into(), (), 0, 6);
        log.add(2, 1, "B".into(), (), 3000, 6);
        log.expire(4499, 4000);
        assert_eq!(log.entries().len(), 2);
        log.expire(4500, 4000);
        assert_eq!(log.entries().len(), 1);
        assert_eq!(log.entries()[0].key, 2);
        // A line added to after it expired starts from nothing.
        log.add(1, 3, "A".into(), (), 5000, 6);
        assert_eq!(log.entries()[1].count, 3);
        log.clear();
        assert!(log.entries().is_empty());
    }

    #[test]
    fn alpha_is_whole_then_fades() {
        assert_eq!(alpha(0, 4000, 500), 1.0);
        assert_eq!(alpha(3999, 4000, 500), 1.0);
        assert_eq!(alpha(4000, 4000, 500), 1.0);
        assert!((alpha(4250, 4000, 500) - 0.5).abs() < 1e-6);
        assert_eq!(alpha(4500, 4000, 500), 0.0);
        assert_eq!(alpha(9999, 4000, 500), 0.0);
        // No fade: gone at once.
        assert_eq!(alpha(4000, 4000, 0), 0.0);
    }

    #[test]
    fn colours_fade_by_their_alpha() {
        assert_eq!(faded(INK, 1.0), INK);
        assert_eq!(faded(INK, 0.5), 0x80FF_FFFF);
        assert_eq!(faded(INK, 0.0), 0x00FF_FFFF);
        assert_eq!(faded(BACKGROUND, 0.5), 0x3C00_0000);
        assert_eq!(faded(INK, 2.0), INK);
    }

    #[test]
    fn a_line_fits_its_text_and_icon() {
        assert_eq!(row_size(60.0, true), [81.0, 18.0]);
        assert_eq!(row_size(60.0, false), [64.0, 12.0]);
        assert_eq!(list_size(&[40.0, 50.0], 18.0), [50.0, 37.0]);
        assert_eq!(list_size(&[40.0], 12.0), [40.0, 12.0]);
        assert_eq!(list_size(&[], 12.0), [0.0, 0.0]);
    }

    #[test]
    fn the_newest_line_is_nearest_the_edge_in_every_corner() {
        let size = [50.0, 37.0];
        let (newest, older) = ([40.0, 18.0], [50.0, 18.0]);
        // Bottom right: newest at the bottom, lines against the right side.
        let at = [268.0, 201.0];
        assert_eq!(
            row_origin(Corner::BottomRight, at, size, 0, newest),
            [278.0, 220.0]
        );
        assert_eq!(
            row_origin(Corner::BottomRight, at, size, 1, older),
            [268.0, 201.0]
        );
        // Bottom left: newest at the bottom, against the left side.
        let at = [2.0, 201.0];
        assert_eq!(
            row_origin(Corner::BottomLeft, at, size, 0, newest),
            [2.0, 220.0]
        );
        assert_eq!(
            row_origin(Corner::BottomLeft, at, size, 1, older),
            [2.0, 201.0]
        );
        // Top left: newest at the top.
        let at = [2.0, 2.0];
        assert_eq!(row_origin(Corner::TopLeft, at, size, 0, newest), [2.0, 2.0]);
        assert_eq!(row_origin(Corner::TopLeft, at, size, 1, older), [2.0, 21.0]);
        // Top right: newest at the top, against the right side.
        let at = [268.0, 2.0];
        assert_eq!(
            row_origin(Corner::TopRight, at, size, 0, newest),
            [278.0, 2.0]
        );
        assert_eq!(
            row_origin(Corner::TopRight, at, size, 1, older),
            [268.0, 21.0]
        );
    }

    #[test]
    fn the_icon_sits_on_the_side_of_the_edge() {
        let row = [81.0, 18.0];
        assert_eq!(
            row_parts(Corner::TopLeft, [2.0, 2.0], row, true),
            RowParts {
                icon: Some([3.0, 3.0]),
                text: [21.0, 7.0],
            }
        );
        assert_eq!(
            row_parts(Corner::BottomRight, [100.0, 50.0], row, true),
            RowParts {
                icon: Some([164.0, 51.0]),
                text: [102.0, 55.0],
            }
        );
        // The icon ends one pixel inside the box, two after the text.
        assert_eq!(164.0 + ICON, 100.0 + row[0] - ICON_INSET);
        assert_eq!(
            row_parts(Corner::TopRight, [100.0, 50.0], [64.0, 12.0], false),
            RowParts {
                icon: None,
                text: [102.0, 52.0],
            }
        );
    }

    #[test]
    fn a_fading_line_slides_out_past_the_edge() {
        assert_eq!(slide(Corner::BottomRight, 40.0, 1.0), 0.0);
        assert_eq!(slide(Corner::BottomRight, 40.0, 0.5), 21.0);
        assert_eq!(slide(Corner::TopLeft, 40.0, 0.5), -21.0);
        assert_eq!(slide(Corner::BottomLeft, 40.0, 0.0), -42.0);
    }
}
