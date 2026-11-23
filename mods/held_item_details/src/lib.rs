//! Held Item Details: a few short centred lines above the hotbar about the item in the selected
//! slot: how much durability it has left, how many of it you carry, and its enchantments. The
//! lines sit above the spot where the game shows the item's name and the action bar message, so
//! they cover neither. The item is read 20 times a second (each client tick); each frame only
//! fits the lines to the screen and draws them.
use std::sync::{Mutex, MutexGuard};

use mod_api::{
    ClientHost, ClientTickPostEvent, ComponentId, Hand, Hooks, ItemResource, ItemStack, Registrar,
    RegistrationContext, RenderGuiPostEvent, SettingSpec, Tag,
};

/// The mod's id: its settings folder and translation keys.
pub const ID: &str = "held_item_details";
/// Text colours, ARGB: the game's white, yellow and red, and its tooltip grey for enchantments.
pub const WHITE: u32 = 0xFFFF_FFFF;
pub const YELLOW: u32 = 0xFFFF_FF55;
pub const RED: u32 = 0xFFFF_5555;
pub const GREY: u32 = 0xFFAA_AAAA;
/// A line's height in GUI pixels: the font's 9 and one between lines.
pub const LINE: f32 = 10.0;
/// How far above the bottom of the screen the game's own text above the hotbar reaches: the
/// selected item's name starts 59 GUI pixels up, the action bar message 72 up (both fixed,
/// whatever the health and food bars do). The lines go above both.
pub const CLEAR_ABOVE_BOTTOM: f32 = 72.0;
/// The gap between the lowest line and the game's text, in GUI pixels.
pub const GAP: f32 = 2.0;
/// The room kept free on each side of the screen when a line is shortened, in GUI pixels.
pub const SIDE: f32 = 4.0;
/// The most enchantments the enchantment line names.
pub const MAX_ENCHANTMENTS: usize = 6;
/// What a shortened line ends with.
pub const ELLIPSIS: &str = "...";

/// The settings on Held Item Details's page in Options > Mods.
pub fn settings() -> [SettingSpec; 4] {
    [
        SettingSpec::toggle("shown", "held_item_details.setting.shown", true)
            .tooltip("held_item_details.setting.shown.tooltip"),
        SettingSpec::toggle("durability", "held_item_details.setting.durability", true)
            .tooltip("held_item_details.setting.durability.tooltip"),
        SettingSpec::toggle("count", "held_item_details.setting.count", true)
            .tooltip("held_item_details.setting.count.tooltip"),
        SettingSpec::toggle(
            "enchantments",
            "held_item_details.setting.enchantments",
            true,
        )
        .tooltip("held_item_details.setting.enchantments.tooltip"),
    ]
}

/// One line to draw: its text and colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub argb: u32,
}

/// Which lines the player asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Show {
    pub durability: bool,
    pub count: bool,
    pub enchantments: bool,
}

/// The words the lines use, in the player's language.
#[derive(Clone, Debug, PartialEq)]
pub struct Labels {
    /// `Durability`.
    pub durability: String,
    /// `in inventory`.
    pub in_inventory: String,
}

impl Default for Labels {
    fn default() -> Self {
        Self {
            durability: "Durability".into(),
            in_inventory: "in inventory".into(),
        }
    }
}

/// What the lines say about the held stack.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Held {
    /// Damage taken and the most it can take, for an item that wears out.
    pub durability: Option<(u32, u32)>,
    /// The held stack's count.
    pub count: u32,
    /// How many like it the inventory holds, the held stack included.
    pub total: u64,
    /// Its enchantments as the game names them (`Sharpness V`), in the game's order.
    pub enchantments: Vec<String>,
}

/// The lines for `held`, top to bottom: enchantments, count, durability (the one nearest the
/// item's name). A line the player turned off, or with nothing to say, is left out.
pub fn lines(held: &Held, show: Show, labels: &Labels) -> Vec<Line> {
    let mut out = Vec::new();
    if show.enchantments
        && let Some(text) = enchantment_line(&held.enchantments)
    {
        out.push(Line { text, argb: GREY });
    }
    if show.count
        && let Some(text) = count_line(held.count, held.total, &labels.in_inventory)
    {
        out.push(Line { text, argb: WHITE });
    }
    if show.durability
        && let Some((damage, max)) = held.durability
        && let Some(line) = durability_line(&labels.durability, damage, max)
    {
        out.push(line);
    }
    out
}

/// `Durability 123 / 250`: what is left of `max` after `damage`, coloured by
/// [`durability_color`]. None for an item with no durability (`max` 0).
pub fn durability_line(label: &str, damage: u32, max: u32) -> Option<Line> {
    if max == 0 {
        return None;
    }
    let left = max.saturating_sub(damage);
    Some(Line {
        text: format!("{label} {left} / {max}"),
        argb: durability_color(left, max),
    })
}

/// Red under a tenth of `max` left, yellow under a quarter, white otherwise.
pub fn durability_color(left: u32, max: u32) -> u32 {
    let (left, max) = (u64::from(left), u64::from(max));
    if left * 10 < max {
        RED
    } else if left * 4 < max {
        YELLOW
    } else {
        WHITE
    }
}

/// `x37 (214 in inventory)` when the inventory holds more like the held stack than the stack
/// itself, `x37` for a stack of more than one, nothing for a single item.
pub fn count_line(count: u32, total: u64, in_inventory: &str) -> Option<String> {
    if total > u64::from(count) {
        Some(format!("x{count} ({total} {in_inventory})"))
    } else if count > 1 {
        Some(format!("x{count}"))
    } else {
        None
    }
}

/// How many like the held stack (of `held` items) the inventory holds: the counts of its
/// matching stacks, which include the held one, and never fewer than the held stack alone.
pub fn total_count(held: u32, matching: impl IntoIterator<Item = u32>) -> u64 {
    let sum: u64 = matching.into_iter().map(u64::from).sum();
    sum.max(u64::from(held))
}

/// Roman numerals as the game shows enchantment levels, I to X; plain numbers beyond.
pub fn roman(level: u32) -> String {
    if !(1..=10).contains(&level) {
        return level.to_string();
    }
    let mut left = level;
    let mut out = String::new();
    for (value, numeral) in [(10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")] {
        while left >= value {
            out.push_str(numeral);
            left -= value;
        }
    }
    out
}

/// An enchantment as the game names it: `Sharpness V`, or just `Mending` for an enchantment
/// that has one level only. `numeral` is the level's text.
pub fn enchantment_text(name: &str, level: u32, max_level: Option<u32>, numeral: &str) -> String {
    if level == 1 && max_level == Some(1) {
        name.to_string()
    } else {
        format!("{name} {numeral}")
    }
}

/// The translation key of enchantment `id`'s name: `minecraft:sharpness` is
/// `enchantment.minecraft.sharpness`.
pub fn enchantment_key(id: &str) -> String {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("enchantment.{namespace}.{}", path.replace('/', "."))
}

/// A name for enchantment `id` when no language names it: its path in words (`fire_aspect` is
/// `Fire Aspect`).
pub fn fallback_name(id: &str) -> String {
    let path = id.rsplit([':', '/']).next().unwrap_or(id);
    path.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect::<String>()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A whole number held in a tag of any numeric kind; none for another kind or a fraction that
/// is not finite.
pub fn number(tag: &Tag) -> Option<i64> {
    match tag {
        Tag::Byte(n) => Some(i64::from(*n)),
        Tag::Short(n) => Some(i64::from(*n)),
        Tag::Int(n) => Some(i64::from(*n)),
        Tag::Long(n) => Some(*n),
        Tag::Float(bits) => whole(f64::from(f32::from_bits(*bits))),
        Tag::Double(bits) => whole(f64::from_bits(*bits)),
        _ => None,
    }
}

fn whole(value: f64) -> Option<i64> {
    // `as` saturates at the ends of i64.
    value.is_finite().then_some(value as i64)
}

/// The enchantments an enchantments component holds (`{"minecraft:sharpness": 5}`), in the
/// component's order; entries that are no positive level are left out.
pub fn enchantment_levels(component: &Tag) -> Vec<(String, u32)> {
    let Tag::Compound(entries) = component else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|(id, level)| {
            let level = u32::try_from(number(level)?).ok().filter(|l| *l > 0)?;
            Some((id.to_string_lossy(), level))
        })
        .collect()
}

/// One enchantment of the held stack, with its most levels when the game knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Enchantment {
    pub id: String,
    pub level: u32,
    pub max_level: Option<u32>,
}

/// `levels` in the order of the game's enchantment registry (`registry`: each id and its most
/// levels), the ones it doesn't list last in their own order.
pub fn sort_enchantments(
    levels: Vec<(String, u32)>,
    registry: &[(String, u32)],
) -> Vec<Enchantment> {
    let mut out: Vec<(usize, Enchantment)> = levels
        .into_iter()
        .map(|(id, level)| {
            let known = registry.iter().position(|(known, _)| *known == id);
            let enchantment = Enchantment {
                max_level: known.map(|i| registry[i].1),
                id,
                level,
            };
            (known.unwrap_or(usize::MAX), enchantment)
        })
        .collect();
    // A stable sort keeps the unknown ones in their own order.
    out.sort_by_key(|(position, _)| *position);
    out.into_iter()
        .map(|(_, enchantment)| enchantment)
        .collect()
}

/// The enchantment line: the first [`MAX_ENCHANTMENTS`] names, `, ...` after them when there
/// are more. None without enchantments.
pub fn enchantment_line(names: &[String]) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let mut line = names[..names.len().min(MAX_ENCHANTMENTS)].join(", ");
    if names.len() > MAX_ENCHANTMENTS {
        line.push_str(", ");
        line.push_str(ELLIPSIS);
    }
    Some(line)
}

/// `text` cut to fit `max` GUI pixels as `width` measures it, ending in `...` when it was cut
/// (the cut never leaves a comma or a space before the dots). Empty when not even the dots fit.
/// It measures a handful of times, not once a character: the cut point is found by halving.
pub fn shorten(text: &str, max: f32, width: impl Fn(&str) -> f32) -> String {
    if text.is_empty() || width(text) <= max {
        return text.to_string();
    }
    let cut = |end: usize| {
        let kept = text[..end].trim_end_matches(|c: char| c == ',' || c.is_whitespace());
        format!("{kept}{ELLIPSIS}")
    };
    // Where each character starts: keeping the text up to one of these is a cut.
    let ends: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    if width(&cut(0)) > max {
        return String::new();
    }
    // `low` fits; the longest cut that fits lies in low..=high.
    let (mut low, mut high) = (0, ends.len() - 1);
    while low < high {
        let mid = (low + high).div_ceil(2);
        if width(&cut(ends[mid])) <= max {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    cut(ends[low])
}

/// The top of each of `count` lines, top line first, on a GUI `gui_height` pixels high: stacked
/// upwards from [`GAP`] above the game's own text over the hotbar.
pub fn line_tops(gui_height: f32, count: usize) -> Vec<f32> {
    let bottom = gui_height - CLEAR_ABOVE_BOTTOM - GAP;
    (0..count)
        .map(|i| bottom - (count - i) as f32 * LINE)
        .collect()
}

/// Where a line `text_width` wide starts so that it is centred on a GUI `gui_width` wide.
pub fn centre_x(gui_width: f32, text_width: f32) -> f32 {
    ((gui_width - text_width) / 2.0).floor()
}

/// The value of component `id` on the held item, its own or its item's default.
fn component(host: &dyn ClientHost, resource: &ItemResource, id: &str) -> Option<Tag> {
    resource.value(host, &ComponentId::new(id)?)
}

/// A component's value as a count: none unless it is a whole number from 0 up.
fn component_count(host: &dyn ClientHost, resource: &ItemResource, id: &str) -> Option<u32> {
    u32::try_from(number(&component(host, resource, id)?)?).ok()
}

/// The held stack's enchantments as the game names them, up to one more than the line shows
/// (so it knows to end with `...`). An enchanted book's stored enchantments count too.
fn enchantment_names(host: &dyn ClientHost, resource: &ItemResource) -> Vec<String> {
    let levels = ["minecraft:enchantments", "minecraft:stored_enchantments"]
        .into_iter()
        .filter_map(|id| component(host, resource, id))
        .map(|tag| enchantment_levels(&tag))
        .find(|levels| !levels.is_empty())
        .unwrap_or_default();
    if levels.is_empty() {
        return Vec::new();
    }
    let registry: Vec<(String, u32)> = host
        .enchantments()
        .into_iter()
        .map(|info| (info.id, info.max_level))
        .collect();
    sort_enchantments(levels, &registry)
        .into_iter()
        .take(MAX_ENCHANTMENTS + 1)
        .map(|e| {
            let name = host
                .translate(&enchantment_key(&e.id))
                .unwrap_or_else(|| fallback_name(&e.id));
            let numeral = host
                .translate(&format!("enchantment.level.{}", e.level))
                .unwrap_or_else(|| roman(e.level));
            enchantment_text(&name, e.level, e.max_level, &numeral)
        })
        .collect()
}

/// The lines about the local player's held stack: none outside a world, for a spectator (who
/// holds nothing on screen) and for an empty hand.
fn read(host: &dyn ClientHost) -> Vec<Line> {
    let Some(player) = host.local_player() else {
        return Vec::new();
    };
    if player.is_spectator(host) == Some(true) {
        return Vec::new();
    }
    let Some(stack) = host
        .living(player.entity())
        .and_then(|living| living.hand_item(host, Hand::Main))
    else {
        return Vec::new();
    };
    let show = Show {
        durability: host.setting_bool("durability").unwrap_or(true),
        count: host.setting_bool("count").unwrap_or(true),
        enchantments: host.setting_bool("enchantments").unwrap_or(true),
    };
    let resource = ItemResource::of(&stack);
    let mut held = Held {
        count: stack.count(),
        total: u64::from(stack.count()),
        ..Held::default()
    };
    if show.durability && resource.damageable(host) {
        let max = component_count(host, &resource, "minecraft:max_damage")
            .or_else(|| host.max_damage(stack.item()));
        let damage =
            component_count(host, &resource, "minecraft:damage").unwrap_or_else(|| stack.damage());
        held.durability = max.map(|max| (damage, max));
    }
    if show.count {
        let inventory = player.inventory(host).unwrap_or_default();
        let matching = inventory
            .iter()
            .filter(|other| stack.is_stackable_with(other))
            .map(ItemStack::count);
        held.total = total_count(stack.count(), matching);
    }
    if show.enchantments {
        held.enchantments = enchantment_names(host, &resource);
    }
    let labels = Labels {
        durability: host
            .translate("held_item_details.durability")
            .unwrap_or_else(|| Labels::default().durability),
        in_inventory: host
            .translate("held_item_details.in_inventory")
            .unwrap_or_else(|| Labels::default().in_inventory),
    };
    lines(&held, show, &labels)
}

#[derive(Default)]
struct HeldItemInfo {
    /// The lines the last client tick read.
    lines: Mutex<Vec<Line>>,
}

impl HeldItemInfo {
    fn cache(&self) -> MutexGuard<'_, Vec<Line>> {
        self.lines.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Whether the player left the mod and its lines on.
fn shown(host: &dyn ClientHost) -> bool {
    host.on(ID) && host.setting_bool("shown").unwrap_or(true)
}

#[mod_api::horizon_mod]
impl Hooks for HeldItemInfo {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// Reads the held stack once a client tick.
    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        let view: &dyn ClientHost = &*host;
        // The local player first: the reads after it are a world's.
        let in_world = view.on(ID) && view.local_player().is_some();
        let lines = if in_world && view.setting_bool("shown").unwrap_or(true) {
            read(view)
        } else {
            Vec::new()
        };
        *self.cache() = lines;
    }

    /// The lines, centred above the hotbar, each cut to the screen's width.
    fn render_gui_post(&self, host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
        if !shown(host) {
            return;
        }
        let lines = self.cache().clone();
        if lines.is_empty() {
            return;
        }
        let draw = &mut *event.draw;
        let [width, height] = draw.size();
        let room = width - 2.0 * SIDE;
        // Every measurement before the first drawing.
        let fitted: Vec<(String, f32, u32)> = lines
            .iter()
            .map(|line| {
                let text = shorten(&line.text, room, |s| draw.text_width(s));
                let text_width = draw.text_width(&text);
                (text, text_width, line.argb)
            })
            .collect();
        for ((text, text_width, argb), y) in fitted.iter().zip(line_tops(height, fitted.len())) {
            if !text.is_empty() {
                draw.text(text, [centre_x(width, *text_width), y], *argb, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six GUI pixels a character, as a stand-in for the game's font.
    fn mono(text: &str) -> f32 {
        text.chars().count() as f32 * 6.0
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    const ALL: Show = Show {
        durability: true,
        count: true,
        enchantments: true,
    };

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
    }

    #[test]
    fn durability_line_shows_what_is_left() {
        let line = durability_line("Durability", 127, 250).unwrap();
        assert_eq!(line.text, "Durability 123 / 250");
        assert_eq!(line.argb, WHITE);
        assert_eq!(
            durability_line("Durability", 0, 1561).unwrap().text,
            "Durability 1561 / 1561"
        );
        // Damage past the maximum reads as nothing left.
        let broken = durability_line("Durability", 300, 250).unwrap();
        assert_eq!(broken.text, "Durability 0 / 250");
        assert_eq!(broken.argb, RED);
        assert_eq!(durability_line("Durability", 0, 0), None);
    }

    #[test]
    fn durability_turns_yellow_under_a_quarter_and_red_under_a_tenth() {
        // An iron tool: 250 uses.
        assert_eq!(durability_color(250, 250), WHITE);
        assert_eq!(durability_color(63, 250), WHITE);
        assert_eq!(durability_color(62, 250), YELLOW, "a quarter is 62.5");
        assert_eq!(
            durability_color(25, 250),
            YELLOW,
            "exactly a tenth is not under it"
        );
        assert_eq!(durability_color(24, 250), RED);
        assert_eq!(durability_color(0, 250), RED);
        // Exactly a quarter stays white.
        assert_eq!(durability_color(25, 100), WHITE);
        assert_eq!(durability_color(24, 100), YELLOW);
        assert_eq!(durability_color(10, 100), YELLOW);
        assert_eq!(durability_color(9, 100), RED);
        // No overflow at the top of the range.
        assert_eq!(durability_color(u32::MAX, u32::MAX), WHITE);
        assert_eq!(
            durability_line("Durability", 230, 250).unwrap().argb,
            RED,
            "20 left of 250"
        );
    }

    #[test]
    fn roman_numerals_one_to_ten_then_plain_numbers() {
        let expected = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
        for (level, numeral) in (1..=10).zip(expected) {
            assert_eq!(roman(level), numeral, "{level}");
        }
        assert_eq!(roman(11), "11");
        assert_eq!(roman(255), "255");
        assert_eq!(roman(0), "0");
    }

    #[test]
    fn single_level_enchantments_show_no_numeral() {
        assert_eq!(
            enchantment_text("Sharpness", 5, Some(5), "V"),
            "Sharpness V"
        );
        assert_eq!(enchantment_text("Mending", 1, Some(1), "I"), "Mending");
        // A level past the most, or an enchantment the game doesn't list, keeps its level.
        assert_eq!(enchantment_text("Mending", 2, Some(1), "II"), "Mending II");
        assert_eq!(enchantment_text("Strange", 1, None, "I"), "Strange I");
        assert_eq!(
            enchantment_text("Unbreaking", 1, Some(3), "I"),
            "Unbreaking I"
        );
    }

    #[test]
    fn enchantment_names_come_from_translation_keys() {
        assert_eq!(
            enchantment_key("minecraft:sharpness"),
            "enchantment.minecraft.sharpness"
        );
        assert_eq!(
            enchantment_key("mymod:deep/frost"),
            "enchantment.mymod.deep.frost"
        );
        assert_eq!(enchantment_key("mending"), "enchantment.minecraft.mending");
        assert_eq!(fallback_name("minecraft:fire_aspect"), "Fire Aspect");
        assert_eq!(fallback_name("mymod:deep/frost_bite"), "Frost Bite");
        assert_eq!(fallback_name("looting"), "Looting");
    }

    #[test]
    fn levels_are_read_from_the_component() {
        let component = Tag::compound([
            ("minecraft:sharpness", Tag::Int(5)),
            ("minecraft:unbreaking", Tag::Short(3)),
            ("minecraft:mending", Tag::Byte(1)),
            ("minecraft:looting", Tag::Long(2)),
            ("minecraft:smite", Tag::Double(4.0f64.to_bits())),
            ("minecraft:knockback", Tag::Int(0)),
            ("minecraft:thorns", Tag::Int(-2)),
            ("minecraft:odd", Tag::String("x".into())),
        ]);
        let mut levels = enchantment_levels(&component);
        levels.sort();
        assert_eq!(
            levels,
            [
                ("minecraft:looting".to_string(), 2),
                ("minecraft:mending".to_string(), 1),
                ("minecraft:sharpness".to_string(), 5),
                ("minecraft:smite".to_string(), 4),
                ("minecraft:unbreaking".to_string(), 3),
            ]
        );
        assert!(enchantment_levels(&Tag::Int(3)).is_empty());
        assert_eq!(number(&Tag::Float(f32::NAN.to_bits())), None);
        assert_eq!(number(&Tag::Float(2.5f32.to_bits())), Some(2));
    }

    #[test]
    fn enchantments_follow_the_registry() {
        let registry = [
            ("minecraft:protection".to_string(), 4),
            ("minecraft:sharpness".to_string(), 5),
            ("minecraft:unbreaking".to_string(), 3),
            ("minecraft:mending".to_string(), 1),
        ];
        let levels = vec![
            ("minecraft:mending".to_string(), 1),
            ("zz:unknown".to_string(), 2),
            ("minecraft:unbreaking".to_string(), 3),
            ("aa:unknown".to_string(), 1),
            ("minecraft:sharpness".to_string(), 5),
        ];
        let sorted = sort_enchantments(levels, &registry);
        let ids: Vec<&str> = sorted.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "minecraft:sharpness",
                "minecraft:unbreaking",
                "minecraft:mending",
                "zz:unknown",
                "aa:unknown"
            ]
        );
        assert_eq!(sorted[2].max_level, Some(1));
        assert_eq!(sorted[3].max_level, None);
    }

    #[test]
    fn up_to_six_enchantments_on_one_line() {
        assert_eq!(enchantment_line(&[]), None);
        assert_eq!(
            enchantment_line(&names(&["Sharpness V", "Mending"])).unwrap(),
            "Sharpness V, Mending"
        );
        let six = names(&["A I", "B II", "C III", "D IV", "E V", "F"]);
        assert_eq!(
            enchantment_line(&six).unwrap(),
            "A I, B II, C III, D IV, E V, F"
        );
        let seven = names(&["A I", "B II", "C III", "D IV", "E V", "F", "G"]);
        assert_eq!(
            enchantment_line(&seven).unwrap(),
            "A I, B II, C III, D IV, E V, F, ..."
        );
    }

    #[test]
    fn count_line_names_what_the_inventory_holds() {
        assert_eq!(
            count_line(37, 214, "in inventory").unwrap(),
            "x37 (214 in inventory)"
        );
        assert_eq!(count_line(37, 37, "in inventory").unwrap(), "x37");
        assert_eq!(count_line(1, 1, "in inventory"), None);
        assert_eq!(
            count_line(1, 3, "in inventory").unwrap(),
            "x1 (3 in inventory)"
        );
    }

    #[test]
    fn total_count_adds_up_the_matching_stacks() {
        // The held 37 and two more stacks of the same.
        assert_eq!(total_count(37, [37, 64, 64, 49]), 214);
        assert_eq!(total_count(37, [37]), 37);
        // Never fewer than the held stack, even when the inventory read finds nothing.
        assert_eq!(total_count(12, []), 12);
        // No overflow with many full stacks of a large count.
        assert_eq!(
            total_count(1, [u32::MAX, u32::MAX]),
            2 * u64::from(u32::MAX)
        );
    }

    #[test]
    fn a_line_that_fits_stays_whole() {
        assert_eq!(
            shorten("Durability 123 / 250", 200.0, mono),
            "Durability 123 / 250"
        );
        assert_eq!(shorten("", 0.0, mono), "");
        // Exactly as wide as the room.
        assert_eq!(shorten("abcde", 30.0, mono), "abcde");
    }

    #[test]
    fn a_long_line_is_cut_with_dots() {
        let text = "Sharpness V, Unbreaking III, Mending";
        // 20 characters of room: 17 kept and the dots.
        let cut = shorten(text, 120.0, mono);
        assert_eq!(cut, "Sharpness V, Unbr...");
        assert!(mono(&cut) <= 120.0);
        // The cut never leaves a comma or a space before the dots.
        assert_eq!(shorten(text, 90.0, mono), "Sharpness V...");
        assert_eq!(shorten(text, 84.0, mono), "Sharpness V...");
        // Only the dots fit, then not even them.
        assert_eq!(shorten(text, 20.0, mono), "...");
        assert_eq!(shorten(text, 17.0, mono), "");
        assert_eq!(shorten(text, -5.0, mono), "");
    }

    #[test]
    fn cuts_fall_between_characters() {
        // Characters of two and three bytes are never split.
        let text = "Schärfe V, Haltbarkeit III, Glück";
        let cut = shorten(text, 60.0, mono);
        assert_eq!(cut, "Schärfe...");
        let cut = shorten("鋭さ V、耐久力 III", 30.0, mono);
        assert_eq!(cut, "鋭さ...");
    }

    #[test]
    fn lines_stack_up_from_above_the_name_and_the_action_bar() {
        // A GUI 240 high: the item name starts at 181, the action bar at 168.
        assert_eq!(line_tops(240.0, 1), [156.0]);
        assert_eq!(line_tops(240.0, 3), [136.0, 146.0, 156.0]);
        assert!(line_tops(240.0, 0).is_empty());
        // The lowest line ends above the action bar's row.
        let lowest = line_tops(240.0, 2)[1];
        assert!(lowest + LINE <= 240.0 - CLEAR_ABOVE_BOTTOM);
        assert_eq!(centre_x(320.0, 100.0), 110.0);
        assert_eq!(centre_x(321.0, 100.0), 110.0);
    }

    #[test]
    fn lines_follow_the_settings() {
        let sword = Held {
            durability: Some((127, 250)),
            count: 1,
            total: 1,
            enchantments: names(&["Sharpness V", "Mending"]),
        };
        let labels = Labels::default();
        let all = lines(&sword, ALL, &labels);
        assert_eq!(
            all,
            [
                Line {
                    text: "Sharpness V, Mending".into(),
                    argb: GREY
                },
                Line {
                    text: "Durability 123 / 250".into(),
                    argb: WHITE
                },
            ]
        );
        let no_enchantments = Show {
            enchantments: false,
            ..ALL
        };
        assert_eq!(lines(&sword, no_enchantments, &labels).len(), 1);
        let no_durability = Show {
            durability: false,
            ..ALL
        };
        assert_eq!(
            lines(&sword, no_durability, &labels)[0].text,
            "Sharpness V, Mending"
        );

        let arrows = Held {
            count: 37,
            total: 214,
            ..Held::default()
        };
        assert_eq!(
            lines(&arrows, ALL, &labels),
            [Line {
                text: "x37 (214 in inventory)".into(),
                argb: WHITE
            }]
        );
        let no_count = Show {
            count: false,
            ..ALL
        };
        assert!(lines(&arrows, no_count, &labels).is_empty());
        assert!(lines(&Held::default(), ALL, &labels).is_empty());
    }
}
