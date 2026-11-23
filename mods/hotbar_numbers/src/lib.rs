//! Hotbar Numbers: in each of the nine hotbar slots, the key that selects it (`key.hotbar.1` to
//! `key.hotbar.9`, as Controls binds them), in a corner of the slot in the game's normal font
//! with a shadow, drawn right after the vanilla hotbar layer. A long key name is shortened to fit
//! the slot. Nothing is drawn in spectator mode (the hotbar is the teleport menu there) or with
//! the HUD hidden.
use std::sync::Mutex;

use mod_api::text::TextContent;
use mod_api::{
    ClientHost, Corner, GuiLayerPostEvent, Hooks, HudMode, Registrar, RegistrationContext,
    SettingSpec, Text,
};

/// The mod's id: its settings folder and translation keys.
pub const ID: &str = "hotbar_numbers";
/// The vanilla HUD layer the labels are drawn on top of (`gui_layer_post`).
pub const HOTBAR_LAYER: &str = "minecraft:hotbar";
/// The hotbar's slots.
pub const SLOTS: usize = 9;
/// The widest a label may be, in GUI pixels: the inside of a slot.
pub const MAX_WIDTH: f32 = 18.0;
/// How often the key names are read again, in milliseconds on the game's clock: a rebound key
/// or a new language shows within this.
pub const REFRESH_MS: u64 = 500;
/// The translation key of the name an unbound key has.
pub const UNBOUND: &str = "key.keyboard.unknown";
/// The labels' default colour, `0xRRGGBB`: white.
pub const DEFAULT_COLOR: u32 = 0xFF_FFFF;
/// The corners of a slot in the order the corner setting lists them.
pub const CORNERS: [Corner; 2] = [Corner::TopLeft, Corner::BottomRight];
/// How deep a key name's component is walked; a key name is one or two nodes.
const MAX_DEPTH: u32 = 8;

/// The settings on Hotbar Numbers' page in Options > Mods.
pub fn settings() -> [SettingSpec; 4] {
    [
        SettingSpec::toggle("shown", "hotbar_numbers.setting.shown", true)
            .tooltip("hotbar_numbers.setting.shown.tooltip"),
        SettingSpec::toggle("only_rebound", "hotbar_numbers.setting.only_rebound", false)
            .tooltip("hotbar_numbers.setting.only_rebound.tooltip"),
        SettingSpec::choice(
            "corner",
            "hotbar_numbers.setting.corner",
            CORNERS.map(|c| format!("hotbar_numbers.corner.{}", c.name())),
            0,
        )
        .tooltip("hotbar_numbers.setting.corner.tooltip"),
        SettingSpec::color("color", "hotbar_numbers.setting.color", DEFAULT_COLOR)
            .tooltip("hotbar_numbers.setting.color.tooltip"),
    ]
}

/// The vanilla mapping that selects hotbar slot `slot` (0 to 8): `key.hotbar.1` to `key.hotbar.9`.
pub fn mapping_name(slot: usize) -> String {
    format!("key.hotbar.{}", slot + 1)
}

/// The name of the key slot `slot` has by default: its number, `1` to `9`. The game names the
/// number row's keys by their digits on every keyboard layout.
pub fn default_name(slot: usize) -> String {
    (slot + 1).to_string()
}

/// A text component as plain text: literal text as it is, a translation in the game's language
/// (else its fallback, else its key) with its arguments put in, a key binding by its translated
/// name, then its children in order.
pub fn plain(text: &Text, translate: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    write_plain(text, translate, &mut out, 0);
    out
}

fn write_plain(
    text: &Text,
    translate: &dyn Fn(&str) -> Option<String>,
    out: &mut String,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    match &text.content {
        TextContent::Literal(s) => out.push_str(s),
        TextContent::Translatable {
            key,
            fallback,
            arguments,
        } => {
            let template = translate(key)
                .or_else(|| fallback.clone())
                .unwrap_or_else(|| key.clone());
            let arguments: Vec<String> = arguments
                .iter()
                .map(|argument| {
                    let mut s = String::new();
                    write_plain(argument, translate, &mut s, depth + 1);
                    s
                })
                .collect();
            out.push_str(&fill_template(&template, &arguments));
        }
        TextContent::Keybind(name) => {
            out.push_str(&translate(name).unwrap_or_else(|| name.clone()));
        }
        _ => {}
    }
    for child in &text.children {
        write_plain(child, translate, out, depth + 1);
    }
}

/// A translation's template with its arguments put in, as the game formats one: `%s` takes the
/// next argument, `%2$s` the second, `%%` is a percent sign. A missing argument leaves nothing;
/// any other `%` stays as it is.
pub fn fill_template(template: &str, arguments: &[String]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut next = 0;
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some('s') => {
                chars.next();
                if let Some(argument) = arguments.get(next) {
                    out.push_str(argument);
                }
                next += 1;
            }
            Some(d) if d.is_ascii_digit() => {
                // `%<n>$s`: argument n, counting from 1.
                let mut ahead = chars.clone();
                let mut n: usize = 0;
                while let Some(digit) = ahead.peek().copied().and_then(|d| d.to_digit(10)) {
                    n = n.saturating_mul(10).saturating_add(digit as usize);
                    ahead.next();
                }
                if n >= 1 && ahead.next() == Some('$') && ahead.next() == Some('s') {
                    chars = ahead;
                    if let Some(argument) = arguments.get(n - 1) {
                        out.push_str(argument);
                    }
                } else {
                    out.push('%');
                }
            }
            _ => out.push('%'),
        }
    }
    out
}

/// The name of the key bound to slot `slot`, from what the game `answered` for its mapping:
/// none for an unbound key (`unbound` is that name in the game's language) or an empty answer;
/// the slot's own number when the game answered the mapping's own name (`action`, as Controls
/// lists it) for a binding it could not resolve, since the default binding is all it can be.
pub fn key_name(
    slot: usize,
    answered: &str,
    unbound: Option<&str>,
    action: Option<&str>,
) -> Option<String> {
    let answered = answered.trim();
    if answered.is_empty() || unbound.map(str::trim) == Some(answered) {
        return None;
    }
    if action.map(str::trim) == Some(answered) {
        return Some(default_name(slot));
    }
    Some(answered.to_owned())
}

/// Whether slot `slot` shows its key `name`: always, or with `only_rebound` only when the key
/// is not the slot's own number.
pub fn shows(slot: usize, name: &str, only_rebound: bool) -> bool {
    !only_rebound || name != default_name(slot)
}

/// A key name short enough for a slot, `width` measuring text in GUI pixels: the name as it is
/// when it fits in `max`; else the first letter of each word but the last, then the last word
/// (`Left Shift` is `LShift`, `Keypad 1` is `K1`), with characters cut from its end until it
/// fits (`LSh`, or the first three letters of a long single word). Never less than one character.
pub fn short_label(name: &str, max: f32, width: &dyn Fn(&str) -> f32) -> String {
    let name = name.trim();
    if name.is_empty() || width(name) <= max {
        return name.to_owned();
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    let compact: String = match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => rest
            .iter()
            .filter_map(|word| word.chars().next())
            .chain(last.chars())
            .collect(),
        _ => name.to_owned(),
    };
    let mut chars: Vec<char> = compact.chars().collect();
    loop {
        let label: String = chars.iter().collect();
        if chars.len() <= 1 || width(label.as_str()) <= max {
            return label;
        }
        chars.pop();
    }
}

/// Where vanilla draws the item of hotbar slot `slot` (0 to 8) on a GUI `size` wide and high, in
/// GUI pixels: the hotbar is 182 by 22, centred at the bottom (half the width rounded down, as
/// the game divides whole pixels), and slot `i`'s 16 pixel item starts 3 pixels in plus 20 a
/// slot, 19 pixels up from the bottom.
pub fn slot_origin(size: [i32; 2], slot: usize) -> [i32; 2] {
    let [width, height] = size;
    [width / 2 - 91 + 3 + 20 * slot as i32, height - 19]
}

/// Where a label `width` GUI pixels wide is drawn in the slot whose item starts at `origin`: one
/// pixel up and left of the item's top left corner, or at its bottom right where vanilla draws a
/// stack's count (`x + 17 - width`, `y + 9`).
pub fn label_at(origin: [i32; 2], corner: Corner, width: f32) -> [f32; 2] {
    let [x, y] = origin.map(|v| v as f32);
    match corner {
        Corner::BottomRight => [x + 17.0 - width, y + 9.0],
        _ => [x - 1.0, y - 1.0],
    }
}

/// The colour `DrawContext::text` takes, opaque ARGB, for a colour setting's `0xRRGGBB`.
pub fn argb(rgb: u32) -> u32 {
    0xFF00_0000 | (rgb & 0xFF_FFFF)
}

/// Whether key names read at `read` (the game's clock, in milliseconds) are read again at `now`:
/// never read, the clock went back, or [`REFRESH_MS`] passed.
pub fn due(read: Option<u64>, now: u64) -> bool {
    match read {
        None => true,
        Some(at) => now < at || now - at >= REFRESH_MS,
    }
}

/// One slot's key: its name, the label drawn and the label's width in GUI pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub name: String,
    pub short: String,
    pub width: f32,
}

#[derive(Default)]
struct State {
    labels: [Option<Label>; SLOTS],
    /// When the names were last read, on the game's clock.
    read: Option<u64>,
}

#[derive(Default)]
struct HotbarNumbers {
    // Never held across a host call: a host call can fire another of this mod's hooks.
    state: Mutex<State>,
}

/// What the game calls the key bound to vanilla mapping `mapping`, in plain text. `mapping`
/// answers only some vanilla mappings (not the hotbar's at this SDK); a key binding component
/// resolves any of them through the player's current bindings, as chat shows one.
fn answered_name(host: &dyn ClientHost, mapping: &str) -> Option<String> {
    match host.mapping(mapping) {
        Ok(Some(m)) => Some(plain(&m.label, &|key| host.translate(key))),
        _ => host
            .text_string(&Text::new(TextContent::Keybind(mapping.to_owned())))
            .ok(),
    }
}

impl HotbarNumbers {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The nine slots' labels, read again from the game every [`REFRESH_MS`]; `width` measures
    /// text as it is drawn.
    fn labels(&self, host: &dyn ClientHost, width: &dyn Fn(&str) -> f32) -> [Option<Label>; SLOTS] {
        let now = host.millis();
        {
            let state = self.state();
            if !due(state.read, now) {
                return state.labels.clone();
            }
        }
        let unbound = host.translate(UNBOUND);
        let labels: [Option<Label>; SLOTS] = std::array::from_fn(|slot| {
            let mapping = mapping_name(slot);
            let answered = answered_name(host, &mapping)?;
            let action = host.translate(&mapping);
            let name = key_name(slot, &answered, unbound.as_deref(), action.as_deref())?;
            let short = short_label(&name, MAX_WIDTH, width);
            Some(Label {
                width: width(short.as_str()),
                name,
                short,
            })
        });
        let mut state = self.state();
        state.labels = labels.clone();
        state.read = Some(now);
        labels
    }
}

#[mod_api::horizon_mod]
impl Hooks for HotbarNumbers {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// Right after the vanilla hotbar is drawn: each slot's key in the corner picked. F1 hides
    /// the layer, so this is not heard then.
    fn gui_layer_post(&self, host: &dyn ClientHost, event: &mut GuiLayerPostEvent<'_>) {
        if event.layer != HOTBAR_LAYER
            || !host.on(ID)
            || !host.setting_bool("shown").unwrap_or(true)
            || matches!(host.hud_mode(), HudMode::NoHud | HudMode::AllHidden)
        {
            return;
        }
        // A spectator's hotbar is the teleport menu, drawn in this layer too.
        let Some(player) = host.local_player() else {
            return;
        };
        if player.is_spectator(host) != Some(false) {
            return;
        }
        let only_rebound = host.setting_bool("only_rebound").unwrap_or(false);
        let corner = host
            .setting_choice("corner")
            .map_or(0, |i| i as usize)
            .min(CORNERS.len() - 1);
        let corner = CORNERS[corner];
        let color = argb(host.setting_color("color").unwrap_or(DEFAULT_COLOR));
        let draw = &mut *event.draw;
        let [w, h] = draw.size();
        let size = [w as i32, h as i32];
        let labels = self.labels(host, &|text| draw.text_width(text));
        for (slot, label) in labels.iter().enumerate() {
            let Some(label) = label else {
                continue;
            };
            if !shows(slot, &label.name, only_rebound) {
                continue;
            }
            let at = label_at(slot_origin(size, slot), corner, label.width);
            draw.text(&label.short, at, color, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default font's width for most characters: 5 pixels and a 1 pixel gap.
    fn six(text: &str) -> f32 {
        text.chars().count() as f32 * 6.0
    }

    fn lang(key: &str) -> Option<String> {
        match key {
            "key.keyboard.left.shift" => Some("Left Shift".into()),
            "key.mouse" => Some("Button %1$s".into()),
            "key.keyboard.unknown" => Some("Not Bound".into()),
            "test.two" => Some("%2$s then %1$s".into()),
            "test.seq" => Some("%s and %s".into()),
            _ => None,
        }
    }

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
        assert_eq!(CORNERS[0], Corner::TopLeft, "the default corner");
    }

    #[test]
    fn each_slot_has_its_mapping_and_number() {
        assert_eq!(mapping_name(0), "key.hotbar.1");
        assert_eq!(mapping_name(8), "key.hotbar.9");
        assert_eq!(default_name(0), "1");
        assert_eq!(default_name(8), "9");
    }

    #[test]
    fn the_slots_are_where_vanilla_draws_the_items() {
        // A 320 by 240 GUI: the hotbar spans 69..251, the first item at 72, 20 pixels a slot.
        assert_eq!(slot_origin([320, 240], 0), [72, 221]);
        assert_eq!(slot_origin([320, 240], 1), [92, 221]);
        assert_eq!(slot_origin([320, 240], 8), [232, 221]);
        // The last item ends 3 pixels inside the hotbar's right edge.
        assert_eq!(slot_origin([320, 240], 8)[0] + 16, 320 / 2 + 91 - 3);
        // An odd width rounds the centre down, as the game does: 213, not 213.5.
        assert_eq!(slot_origin([427, 240], 0), [125, 221]);
    }

    #[test]
    fn labels_sit_in_the_corner_picked() {
        let origin = [72, 221];
        assert_eq!(label_at(origin, Corner::TopLeft, 6.0), [71.0, 220.0]);
        // Bottom right ends where a stack's count ends.
        assert_eq!(label_at(origin, Corner::BottomRight, 6.0), [83.0, 230.0]);
        // The widest label still starts inside the slot (which begins 2 pixels left of the item).
        let [x, _] = label_at(origin, Corner::BottomRight, MAX_WIDTH);
        assert!(x >= origin[0] as f32 - 2.0, "{x}");
    }

    #[test]
    fn long_key_names_are_shortened_to_fit_a_slot() {
        assert_eq!(short_label("1", MAX_WIDTH, &six), "1");
        assert_eq!(short_label("Tab", MAX_WIDTH, &six), "Tab");
        assert_eq!(short_label("Left Shift", MAX_WIDTH, &six), "LSh");
        assert_eq!(short_label("Right Control", MAX_WIDTH, &six), "RCo");
        assert_eq!(short_label("Keypad 1", MAX_WIDTH, &six), "K1");
        assert_eq!(short_label("Button 4", MAX_WIDTH, &six), "B4");
        // One long word: its first three letters.
        assert_eq!(short_label("Backspace", MAX_WIDTH, &six), "Bac");
        assert_eq!(short_label("  Escape ", MAX_WIDTH, &six), "Esc");
        assert_eq!(short_label("", MAX_WIDTH, &six), "");
        for name in ["Left Shift", "Right Control", "Backspace", "Ñandú Öl"] {
            assert!(
                six(&short_label(name, MAX_WIDTH, &six)) <= MAX_WIDTH,
                "{name}"
            );
        }
        // Narrow letters keep more of the name; a character too wide alone is still drawn.
        let narrow = |s: &str| -> f32 { s.chars().map(|c| if c == 'l' { 2.0 } else { 6.0 }).sum() };
        assert_eq!(short_label("Fallback", MAX_WIDTH, &narrow), "Fall");
        assert_eq!(short_label("WW", 4.0, &six), "W");
    }

    #[test]
    fn unbound_unknown_and_default_keys() {
        let unbound = Some("Not Bound");
        let action = Some("Hotbar Slot 3");
        assert_eq!(key_name(2, "3", unbound, action), Some("3".into()));
        assert_eq!(key_name(2, " R ", unbound, action), Some("R".into()));
        assert_eq!(key_name(2, "Not Bound", unbound, action), None);
        assert_eq!(key_name(2, "", unbound, action), None);
        // A binding the game could not resolve reads as the mapping's own name: the default.
        assert_eq!(
            key_name(2, "Hotbar Slot 3", unbound, action),
            Some("3".into())
        );
        assert_eq!(key_name(2, "3", None, None), Some("3".into()));
    }

    #[test]
    fn only_rebound_keys_hide_the_usual_numbers() {
        assert!(shows(0, "1", false));
        assert!(!shows(0, "1", true));
        assert!(shows(0, "2", true), "slot 1 on the 2 key is rebound");
        assert!(shows(4, "Left Shift", true));
        assert!(!shows(8, "9", true));
    }

    #[test]
    fn key_names_read_as_plain_text() {
        assert_eq!(plain(&Text::literal("R"), &lang), "R");
        assert_eq!(
            plain(&Text::translate("key.keyboard.left.shift", vec![]), &lang),
            "Left Shift"
        );
        // Mouse buttons name their number through a positional argument.
        assert_eq!(
            plain(
                &Text::translate("key.mouse", vec![Text::literal("4")]),
                &lang
            ),
            "Button 4"
        );
        assert_eq!(
            plain(
                &Text::new(TextContent::Keybind("key.keyboard.unknown".into())),
                &lang
            ),
            "Not Bound"
        );
        let missing = Text::new(TextContent::Translatable {
            key: "key.keyboard.world.1".into(),
            fallback: Some("World %s".into()),
            arguments: vec![Text::literal("1")],
        });
        assert_eq!(plain(&missing, &lang), "World 1");
        assert_eq!(
            plain(&Text::translate("key.keyboard.f13", vec![]), &lang),
            "key.keyboard.f13"
        );
        let joined = Text::literal("Ctrl").append(Text::literal(" + Q"));
        assert_eq!(plain(&joined, &lang), "Ctrl + Q");
    }

    #[test]
    fn translation_templates_take_their_arguments() {
        let args = ["a".to_string(), "b".to_string()];
        assert_eq!(fill_template("%s and %s", &args), "a and b");
        assert_eq!(fill_template("%2$s then %1$s", &args), "b then a");
        assert_eq!(fill_template("100%%", &args), "100%");
        assert_eq!(fill_template("%s %s %s", &args), "a b ");
        assert_eq!(fill_template("%3$s|%0$s|%x|%", &args), "|%0$s|%x|%");
        assert_eq!(fill_template("%99999999999999999999999$s!", &args), "!");
        assert_eq!(
            plain(
                &Text::translate("test.two", vec![Text::literal("x"), Text::literal("y")]),
                &lang
            ),
            "y then x"
        );
    }

    #[test]
    fn names_are_read_again_twice_a_second() {
        assert!(due(None, 0));
        assert!(!due(Some(1000), 1000));
        assert!(!due(Some(1000), 1000 + REFRESH_MS - 1));
        assert!(due(Some(1000), 1000 + REFRESH_MS));
        assert!(due(Some(1000), 999), "the clock went back");
    }

    #[test]
    fn the_colour_is_opaque() {
        assert_eq!(argb(DEFAULT_COLOR), 0xFFFF_FFFF);
        assert_eq!(argb(0x12_3456), 0xFF12_3456);
        assert_eq!(argb(0x7F12_3456), 0xFF12_3456);
    }
}
