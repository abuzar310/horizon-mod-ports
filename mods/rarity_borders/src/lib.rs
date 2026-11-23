//! Rarity Borders: a thin coloured outline round each inventory and container slot whose item is
//! uncommon (yellow), rare (aqua) or epic (light purple), the colour the item's name has, with a
//! brighter bottom edge. The outline sits on the slot's own one-pixel frame: the item, its count,
//! the hover highlight and the tooltip all draw over it. Common items (white) and the hotbar are
//! optional.
//!
//! The rarity is the game's own (`ItemResource::rarity`, vanilla's `ItemStack.getRarity`): the
//! stack's `minecraft:rarity` component, else its item's default, else common, raised one step
//! (common and uncommon to rare, rare to epic) for an item with enchantments.
use std::sync::{Mutex, MutexGuard};

use mod_api::{
    ClientHost, DrawContext, GuiLayerPostEvent, Hooks, Host, ItemRarity, ItemResource, ItemStack,
    MenuSlotKind, Registrar, RegistrationContext, ScreenRenderBackgroundEvent, SettingSpec,
};

/// The mod's id: its settings folder and translation keys.
pub const ID: &str = "rarity_borders";
/// The id the screen hooks give every container, station and mod window, and the player's own
/// inventory (`ScreenView::id`): they all draw on the inventory screen.
pub const INVENTORY_SCREEN: &str = "inventory";
/// The vanilla HUD layer the hotbar borders are drawn on top of (`gui_layer_post`).
pub const HOTBAR_LAYER: &str = "minecraft:hotbar";
/// The hotbar's slots.
pub const HOTBAR_SLOTS: usize = 9;
/// The player's own inventory menu (`InventoryMenu`): 46 slots, the hotbar at 36 to 44.
pub const INVENTORY_MENU_SLOTS: usize = 46;
/// The inventory menu's first hotbar slot.
pub const INVENTORY_MENU_HOTBAR: usize = 36;
/// An item icon's side, in GUI pixels.
pub const ITEM: f32 = 16.0;

/// The rarity colours, `0xRRGGBB`: the colours vanilla gives an item's name (white, yellow, aqua
/// and light purple).
pub const COMMON_RGB: u32 = 0xFF_FFFF;
pub const UNCOMMON_RGB: u32 = 0xFF_FF55;
pub const RARE_RGB: u32 = 0x55_FFFF;
pub const EPIC_RGB: u32 = 0xFF_55FF;
/// How bright the top and side edges are beside the bottom one, out of 255: three quarters.
pub const SIDE_SHADE: u32 = 0xBF;
/// The alpha of an opaque `0xAARRGGBB` colour.
pub const OPAQUE: u32 = 0xFF00_0000;

/// How long a rarity worked out for an item is kept, in milliseconds on the game's clock, so a
/// data pack reload or another server shows within this.
pub const FORGET_MS: u64 = 10_000;
/// The most items whose rarity is kept at once: more than the 90 slots of a large chest's screen
/// and the hotbar's 9 together, so a full screen of different items is asked about once.
pub const REMEMBERED: usize = 256;

/// The settings on Rarity Borders' page in Options > Mods.
pub fn settings() -> [SettingSpec; 3] {
    [
        SettingSpec::toggle("shown", "rarity_borders.setting.shown", true)
            .tooltip("rarity_borders.setting.shown.tooltip"),
        SettingSpec::toggle("common", "rarity_borders.setting.common", false)
            .tooltip("rarity_borders.setting.common.tooltip"),
        SettingSpec::toggle("hotbar", "rarity_borders.setting.hotbar", true)
            .tooltip("rarity_borders.setting.hotbar.tooltip"),
    ]
}

/// An item's rarity, as the game ranks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
}

impl Rarity {
    /// Every rarity, lowest first.
    pub const ALL: [Self; 4] = [Self::Common, Self::Uncommon, Self::Rare, Self::Epic];

    /// The name the `minecraft:rarity` component is written with.
    pub fn name(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Uncommon => "uncommon",
            Self::Rare => "rare",
            Self::Epic => "epic",
        }
    }

    /// A rarity from the text of a `minecraft:rarity` component: `uncommon`, also quoted as SNBT
    /// writes it (`"uncommon"`), with its namespace or in another case. Anything else, an empty
    /// text included, is common.
    pub fn parse(text: &str) -> Self {
        let text = text.trim().trim_matches('"');
        let text = text.strip_prefix("minecraft:").unwrap_or(text);
        Self::ALL
            .into_iter()
            .find(|rarity| rarity.name().eq_ignore_ascii_case(text))
            .unwrap_or(Self::Common)
    }

    /// The rarity the game worked out for a stack, by its name: a rarity a later game adds reads
    /// as common.
    pub fn from_item(rarity: ItemRarity) -> Self {
        Self::parse(rarity.as_str())
    }

    /// The colour of the rarity, `0xRRGGBB`.
    pub fn rgb(self) -> u32 {
        match self {
            Self::Common => COMMON_RGB,
            Self::Uncommon => UNCOMMON_RGB,
            Self::Rare => RARE_RGB,
            Self::Epic => EPIC_RGB,
        }
    }
}

/// The colour of the border a slot holding an item of `rarity` gets, `0xRRGGBB`; none for a
/// common item unless `common` (the setting) is on.
pub fn border_rgb(rarity: Rarity, common: bool) -> Option<u32> {
    (rarity != Rarity::Common || common).then(|| rarity.rgb())
}

/// The border round the item box `[x, y, w, h]` (GUI pixels): the one-pixel ring just outside it,
/// where the slot's own frame is, as four rectangles that don't overlap, in the order top, left,
/// right, bottom. The top and bottom span the corners.
pub fn edges([x, y, w, h]: [f32; 4]) -> [[f32; 4]; 4] {
    [
        [x - 1.0, y - 1.0, w + 2.0, 1.0],
        [x - 1.0, y, 1.0, h],
        [x + w, y, 1.0, h],
        [x - 1.0, y + h, w + 2.0, 1.0],
    ]
}

/// `rgb` (`0xRRGGBB`) with each channel scaled by `by` out of 255.
pub fn shade(rgb: u32, by: u32) -> u32 {
    let channel = |shift: u32| ((((rgb >> shift) & 0xFF) * by / 0xFF) & 0xFF) << shift;
    channel(16) | channel(8) | channel(0)
}

/// The colours of the four [`edges`], opaque `0xAARRGGBB`: the top and sides a little darker,
/// the bottom the rarity's own colour.
pub fn edge_colors(rgb: u32) -> [u32; 4] {
    let side = OPAQUE | shade(rgb, SIDE_SHADE);
    let bottom = OPAQUE | (rgb & 0xFF_FFFF);
    [side, side, side, bottom]
}

/// Where the hotbar draws slot `slot`'s item (0 to 8) on a GUI `size` (width, height) big, as
/// the game's `Hud.extractItemHotbar` does (Horizon's `hud_layout::hotbar`):
/// `width / 2 - 90 + slot * 20 + 2`, `height - 16 - 3`, in whole pixels.
pub fn hotbar_box(size: [f32; 2], slot: usize) -> [f32; 4] {
    let [width, height] = size.map(|v| v as i32);
    let x = width / 2 - 90 + 20 * slot as i32 + 2;
    let y = height - 16 - 3;
    [x as f32, y as f32, ITEM, ITEM]
}

/// For each hotbar slot (player inventory 0 to 8), the menu slot that shows it, by the open
/// screen's slot roles; none where the menu doesn't show it.
pub fn hotbar_slots(kinds: &[MenuSlotKind]) -> [Option<usize>; HOTBAR_SLOTS] {
    let mut slots = [None; HOTBAR_SLOTS];
    for (index, kind) in kinds.iter().enumerate() {
        if let MenuSlotKind::Player(n) = *kind
            && let Some(slot) = slots.get_mut(n as usize)
            && slot.is_none()
        {
            *slot = Some(index);
        }
    }
    slots
}

/// The hotbar's slots in the player's own inventory menu: 36 to 44.
pub fn inventory_hotbar_slots() -> [Option<usize>; HOTBAR_SLOTS] {
    std::array::from_fn(|slot| Some(INVENTORY_MENU_HOTBAR + slot))
}

/// Rarities already worked out, by item and components (`same` says whether two keys are the
/// same item, whatever their counts), so a frame asks the game only about items it hasn't seen.
/// Everything is forgotten every [`FORGET_MS`], when the clock goes back, and when
/// [`REMEMBERED`] items are known; the buffer, reserved once, never grows.
#[derive(Debug)]
pub struct Remembered<K> {
    known: Vec<(K, Rarity)>,
    since: Option<u64>,
}

impl<K> Default for Remembered<K> {
    fn default() -> Self {
        Self {
            known: Vec::with_capacity(REMEMBERED),
            since: None,
        }
    }
}

impl<K: Clone> Remembered<K> {
    /// The rarity of `key` at `now` (the game's clock, in milliseconds): the one kept, else
    /// `work_out`'s answer, kept.
    pub fn get(
        &mut self,
        key: &K,
        same: impl Fn(&K, &K) -> bool,
        now: u64,
        work_out: impl FnOnce() -> Rarity,
    ) -> Rarity {
        if self
            .since
            .is_none_or(|since| now < since || now - since >= FORGET_MS)
        {
            self.known.clear();
            self.since = Some(now);
        }
        if let Some(&(_, rarity)) = self.known.iter().find(|(known, _)| same(known, key)) {
            return rarity;
        }
        let rarity = work_out();
        if self.known.len() >= REMEMBERED {
            self.known.clear();
        }
        self.known.push((key.clone(), rarity));
        rarity
    }
}

/// The game's rarity for `stack` (`ItemResource::rarity`, vanilla's `ItemStack.getRarity`): its
/// `minecraft:rarity` component, else its item's default, raised a step for enchantments.
fn stack_rarity(host: &dyn Host, stack: &ItemStack) -> Rarity {
    Rarity::from_item(ItemResource::of(stack).rarity(host))
}

/// Draws the border of colour `rgb` round the item box `area`.
fn draw_border(draw: &mut (impl DrawContext + ?Sized), area: [f32; 4], rgb: u32) {
    for (rect, argb) in edges(area).into_iter().zip(edge_colors(rgb)) {
        draw.fill(rect, argb);
    }
}

#[derive(Default)]
struct RarityBorders {
    known: Mutex<Remembered<ItemStack>>,
}

impl RarityBorders {
    fn known(&self) -> MutexGuard<'_, Remembered<ItemStack>> {
        self.known.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[mod_api::horizon_mod]
impl Hooks for RarityBorders {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// On an inventory, container, station or mod window, after its panel and slot frames and
    /// before its items: a border on the frame of each slot holding a rare enough item, so the
    /// item, its count and the tooltip draw over it. Not on the creative inventory or with the
    /// recipe book covering the inventory: the game gives no slot boxes there.
    fn screen_render_background(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenRenderBackgroundEvent<'_>,
    ) {
        // The title's own screens fire this too, with no world: nothing else is asked there.
        if event.screen.id != INVENTORY_SCREEN
            || !host.on(ID)
            || !host.setting_bool("shown").unwrap_or(true)
        {
            return;
        }
        let Some(menu) = host
            .local_player()
            .and_then(|player| player.container_menu(host))
        else {
            return;
        };
        let Some(boxes) = menu.slot_boxes(host) else {
            return;
        };
        let common = host.setting_bool("common").unwrap_or(false);
        let now = host.millis();
        let draw = &mut *event.draw;
        let mut known = self.known();
        for (slot, area) in boxes.into_iter().enumerate() {
            let Some(area) = area else {
                continue;
            };
            let Some(Some(stack)) = menu.slot(host, slot) else {
                continue;
            };
            let rarity = known.get(&stack, ItemStack::is_stackable_with, now, || {
                stack_rarity(host, &stack)
            });
            if let Some(rgb) = border_rgb(rarity, common) {
                draw_border(&mut *draw, area, rgb);
            }
        }
    }

    /// Right after the vanilla hotbar is drawn: the same borders round its nine items. F1 hides
    /// the layer, so this is not heard then; nothing is drawn in spectator mode, where the hotbar
    /// is the teleport menu.
    fn gui_layer_post(&self, host: &dyn ClientHost, event: &mut GuiLayerPostEvent<'_>) {
        if event.layer != HOTBAR_LAYER
            || !host.on(ID)
            || !host.setting_bool("shown").unwrap_or(true)
            || !host.setting_bool("hotbar").unwrap_or(true)
        {
            return;
        }
        let Some(player) = host.local_player() else {
            return;
        };
        if player.is_spectator(host) != Some(false) {
            return;
        }
        let Some(menu) = player.container_menu(host) else {
            return;
        };
        // With an inventory screen open its slot roles say where the hotbar is; while playing the
        // menu is the player's own inventory menu.
        let slots = match menu.slot_kinds(host) {
            Some(kinds) => hotbar_slots(&kinds),
            None if menu.slot_count(host) == Some(INVENTORY_MENU_SLOTS) => inventory_hotbar_slots(),
            None => return,
        };
        let common = host.setting_bool("common").unwrap_or(false);
        let now = host.millis();
        let draw = &mut *event.draw;
        let size = draw.size();
        let mut known = self.known();
        for (index, slot) in slots.into_iter().enumerate() {
            let Some(slot) = slot else {
                continue;
            };
            let Some(Some(stack)) = menu.slot(host, slot) else {
                continue;
            };
            let rarity = known.get(&stack, ItemStack::is_stackable_with, now, || {
                stack_rarity(host, &stack)
            });
            if let Some(rgb) = border_rgb(rarity, common) {
                draw_border(&mut *draw, hotbar_box(size, index), rgb);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
    }

    #[test]
    fn rarity_parses_from_component_text() {
        assert_eq!(Rarity::parse("common"), Rarity::Common);
        assert_eq!(Rarity::parse("uncommon"), Rarity::Uncommon);
        assert_eq!(Rarity::parse("rare"), Rarity::Rare);
        assert_eq!(Rarity::parse("epic"), Rarity::Epic);
        // As SNBT quotes it, with its namespace, in another case, with spaces round it.
        assert_eq!(Rarity::parse("\"uncommon\""), Rarity::Uncommon);
        assert_eq!(Rarity::parse("minecraft:rare"), Rarity::Rare);
        assert_eq!(Rarity::parse("  EPIC "), Rarity::Epic);
        assert_eq!(Rarity::parse("\"minecraft:Epic\""), Rarity::Epic);
        // Anything else is common.
        assert_eq!(Rarity::parse(""), Rarity::Common);
        assert_eq!(Rarity::parse("legendary"), Rarity::Common);
        assert_eq!(Rarity::parse("mymod:epic"), Rarity::Common);
        assert_eq!(Rarity::parse("rarer"), Rarity::Common);
    }

    #[test]
    fn each_rarity_reads_back_from_its_name() {
        for rarity in Rarity::ALL {
            assert_eq!(Rarity::parse(rarity.name()), rarity);
        }
        for (item, rarity) in [
            (ItemRarity::Common, Rarity::Common),
            (ItemRarity::Uncommon, Rarity::Uncommon),
            (ItemRarity::Rare, Rarity::Rare),
            (ItemRarity::Epic, Rarity::Epic),
        ] {
            assert_eq!(Rarity::from_item(item), rarity);
            assert_eq!(rarity.name(), item.as_str());
        }
    }

    /// The game's rule (`ItemRarity::of`, vanilla's `ItemStack.getRarity`), which the borders
    /// follow: enchantments raise common and uncommon to rare and rare to epic.
    #[test]
    fn enchantments_raise_the_rarity_as_vanilla_does() {
        let shown =
            |base: &str, enchanted: bool| Rarity::from_item(ItemRarity::of(base, enchanted));
        assert_eq!(shown("", false), Rarity::Common);
        assert_eq!(shown("common", false), Rarity::Common);
        assert_eq!(shown("uncommon", false), Rarity::Uncommon);
        assert_eq!(shown("rare", false), Rarity::Rare);
        assert_eq!(shown("epic", false), Rarity::Epic);
        assert_eq!(shown("", true), Rarity::Rare);
        assert_eq!(shown("common", true), Rarity::Rare);
        assert_eq!(shown("uncommon", true), Rarity::Rare);
        assert_eq!(shown("rare", true), Rarity::Epic);
        assert_eq!(shown("epic", true), Rarity::Epic);
    }

    #[test]
    fn each_rarity_has_its_name_colour() {
        assert_eq!(Rarity::Common.rgb(), 0xFFFFFF);
        assert_eq!(Rarity::Uncommon.rgb(), 0xFFFF55);
        assert_eq!(Rarity::Rare.rgb(), 0x55FFFF);
        assert_eq!(Rarity::Epic.rgb(), 0xFF55FF);
        // Common items get a border only when asked.
        assert_eq!(border_rgb(Rarity::Common, false), None);
        assert_eq!(border_rgb(Rarity::Common, true), Some(0xFFFFFF));
        for rarity in [Rarity::Uncommon, Rarity::Rare, Rarity::Epic] {
            assert_eq!(border_rgb(rarity, false), Some(rarity.rgb()));
            assert_eq!(border_rgb(rarity, true), Some(rarity.rgb()));
        }
    }

    #[test]
    fn the_border_is_the_ring_round_the_item() {
        assert_eq!(
            edges([10.0, 20.0, 16.0, 16.0]),
            [
                [9.0, 19.0, 18.0, 1.0],
                [9.0, 20.0, 1.0, 16.0],
                [26.0, 20.0, 1.0, 16.0],
                [9.0, 36.0, 18.0, 1.0],
            ]
        );
        // Every pixel of the 18 by 18 square round the item but the item's own, once each.
        let ring = edges([1.0, 1.0, 16.0, 16.0]);
        for row in 0..18usize {
            for column in 0..18usize {
                let (px, py) = (column as f32 + 0.5, row as f32 + 0.5);
                let hits = ring
                    .iter()
                    .filter(|[x, y, w, h]| (*x..x + w).contains(&px) && (*y..y + h).contains(&py))
                    .count();
                let inside = (1..17).contains(&row) && (1..17).contains(&column);
                assert_eq!(hits, usize::from(!inside), "pixel {column}, {row}");
            }
        }
    }

    #[test]
    fn the_bottom_edge_is_brighter() {
        assert_eq!(
            edge_colors(UNCOMMON_RGB),
            [0xFFBF_BF3F, 0xFFBF_BF3F, 0xFFBF_BF3F, 0xFFFF_FF55]
        );
        assert_eq!(edge_colors(COMMON_RGB)[0], 0xFFBF_BFBF);
        assert_eq!(edge_colors(EPIC_RGB)[3], 0xFFFF_55FF);
        // A colour with alpha in it still comes out opaque.
        assert_eq!(edge_colors(0x12_55_FF_FF)[3], 0xFF55_FFFF);
        assert_eq!(shade(0xFF_FFFF, 0xFF), 0xFF_FFFF);
        assert_eq!(shade(0xFF_FFFF, 0), 0);
        assert_eq!(shade(0x80_4020, 0x80), 0x40_2010);
    }

    #[test]
    fn hotbar_items_sit_where_vanilla_draws_them() {
        // 854 by 480 at GUI scale 2.
        assert_eq!(hotbar_box([427.0, 240.0], 0), [125.0, 221.0, 16.0, 16.0]);
        assert_eq!(hotbar_box([427.0, 240.0], 8), [285.0, 221.0, 16.0, 16.0]);
        // Slots are 20 pixels apart, and a fractional GUI size counts in whole pixels.
        assert_eq!(hotbar_box([427.9, 240.5], 4), [205.0, 221.0, 16.0, 16.0]);
    }

    #[test]
    fn the_hotbar_is_found_by_its_slot_roles() {
        use MenuSlotKind as K;
        // The player's own inventory menu: result, grid, armour head to feet, the main
        // inventory, the hotbar, the off hand.
        let mut inventory = vec![K::Result, K::CraftingGrid, K::CraftingGrid];
        inventory.extend([K::CraftingGrid, K::CraftingGrid]);
        inventory.extend((36..40u32).rev().map(K::Player));
        inventory.extend((9..36u32).map(K::Player));
        inventory.extend((0..9u32).map(K::Player));
        inventory.push(K::Player(40));
        assert_eq!(inventory.len(), INVENTORY_MENU_SLOTS);
        assert_eq!(hotbar_slots(&inventory), inventory_hotbar_slots());
        // A chest of three rows: its 27 slots, then the inventory, then the hotbar.
        let mut chest = vec![K::Storage; 27];
        chest.extend((9..36u32).map(K::Player));
        chest.extend((0..9u32).map(K::Player));
        let chest_hotbar: [Option<usize>; HOTBAR_SLOTS] =
            std::array::from_fn(|slot| Some(54 + slot));
        assert_eq!(hotbar_slots(&chest), chest_hotbar);
        // A menu that doesn't show the hotbar.
        assert_eq!(hotbar_slots(&[K::Other, K::Storage]), [None; HOTBAR_SLOTS]);
        assert_eq!(
            inventory_hotbar_slots(),
            [
                Some(36),
                Some(37),
                Some(38),
                Some(39),
                Some(40),
                Some(41),
                Some(42),
                Some(43),
                Some(44)
            ]
        );
    }

    /// Two keys are the same item when their first parts are, as two stacks of one item with
    /// other counts are.
    fn same(a: &(u32, u32), b: &(u32, u32)) -> bool {
        a.0 == b.0
    }

    #[test]
    fn a_rarity_is_worked_out_once_per_item() {
        let mut known = Remembered::default();
        let mut asked = 0;
        for count in 1..=10 {
            let rarity = known.get(&(7, count), same, 100, || {
                asked += 1;
                Rarity::Epic
            });
            assert_eq!(rarity, Rarity::Epic);
        }
        assert_eq!(
            asked, 1,
            "another count of the same item is not asked again"
        );
        let other = known.get(&(8, 1), same, 100, || Rarity::Uncommon);
        assert_eq!(other, Rarity::Uncommon);
        assert_eq!(
            known.get(&(7, 1), same, 100, || Rarity::Common),
            Rarity::Epic
        );
    }

    #[test]
    fn rarities_are_forgotten_after_a_while() {
        let mut known = Remembered::default();
        assert_eq!(
            known.get(&(1, 1), same, 1000, || Rarity::Rare),
            Rarity::Rare
        );
        let just_before = 1000 + FORGET_MS - 1;
        assert_eq!(
            known.get(&(1, 1), same, just_before, || Rarity::Common),
            Rarity::Rare
        );
        let after = 1000 + FORGET_MS;
        assert_eq!(
            known.get(&(1, 1), same, after, || Rarity::Epic),
            Rarity::Epic
        );
        // A clock that went back forgets too.
        assert_eq!(
            known.get(&(1, 1), same, 5, || Rarity::Uncommon),
            Rarity::Uncommon
        );
    }

    /// The game stops a mod whose hook runs past its time budget: the buffer reserved at the
    /// start never grows, however many items a frame shows.
    #[test]
    fn the_rarity_buffer_never_grows() {
        let mut known = Remembered::default();
        let reserved = known.known.capacity();
        assert!(reserved >= REMEMBERED);
        for item in 0..1000u32 {
            known.get(&(item, 1), same, 0, || Rarity::Rare);
            assert!(known.known.len() <= REMEMBERED);
        }
        assert_eq!(known.known.capacity(), reserved);
    }
}
