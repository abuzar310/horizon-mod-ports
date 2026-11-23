//! X-Ray (single player): press X to see ores through the ground, in your own single-player world
//! only.
//!
//! While X-ray is on, the blocks the ground is mostly made of (stone, deepslate, tuff, dirt, grass,
//! gravel, sand, netherrack, basalt, end stone...) draw nothing, so what is left among them stands
//! out: ores, chests, spawners, water and lava, caves and buildings. It uses the game's own block
//! render visibility: a block part answering `block_render_visibility` is attached to those blocks
//! (by the game's tags where a tag holds just them, else by id), and switching X-ray on or off asks
//! the game to resolve the answers again (`ClientHost::refresh_render_visibility`), which remeshes
//! the loaded terrain once. Light, collision and the server are untouched. There is no full
//! brightness: the SDK gives a WebAssembly mod no gamma or night vision call, so caves stay as dark
//! as they are.
//!
//! Single player only. X-ray works only while this window owns the integrated server of a save
//! (`ClientServer::integrated_local` with a `save_folder`) and the Tab list holds the local player
//! alone. That lock is checked on every press of the key, on every client tick while X-ray is on,
//! and when a level loads; when it fails X-ray turns off and says once in chat that it works only
//! in single player. A world opened to LAN looks exactly like single player to the SDK while you
//! are alone in it, so X-ray turns off as soon as anyone else joins.
use std::sync::{Mutex, MutexGuard};

use mod_api::{
    BlockPart, BlockRenderVisibilityEvent, ClientHost, ClientLevelLoadEvent, ClientServer,
    ClientTickPostEvent, Hooks, KeyConflictContext, KeyRegistration, Registrar,
    RegistrationContext, SettingSpec, Text,
};

/// The mod's id: its settings folder, its key's owner and its translation keys.
pub const ID: &str = "xray_single_player";
/// The id of the mod's key (its line in `horizon-mods.txt`).
pub const KEY: &str = "toggle";
/// The key's name on Options > Controls.
pub const KEY_LABEL: &str = "key.xray_single_player.toggle";
/// The key it is bound to until the player changes it.
pub const DEFAULT_KEY: &str = "key.keyboard.x";
/// The most clicks of the key taken in a tick. Its auto-repeat adds about one a tick, so this is
/// never reached by a player; it only bounds the loop.
pub const MAX_CLICKS: u32 = 32;
/// The "remember state" setting's key.
pub const REMEMBER: &str = "remember";
/// The chat line said when the lock turns X-ray off or keeps it off.
pub const SINGLE_PLAYER_ONLY: &str = "xray_single_player.message.single_player_only";
/// That line in English, for a game whose languages lack the key.
pub const SINGLE_PLAYER_ONLY_FALLBACK: &str = "X-Ray works only in single player";

/// The game's block tags whose blocks X-ray hides: each holds only blocks the ground is made of.
/// The game reads them from its own tag files when a world opens, so the blocks are never copied
/// here. In the game's 26.3 data they hold stone, granite, diorite, andesite, tuff and deepslate;
/// netherrack, basalt and blackstone; dirt, coarse and rooted dirt; grass, podzol and mycelium;
/// mud and muddy mangrove roots; the two moss blocks; the two nyliums; and terracotta of every
/// colour. `minecraft:sand` is left out: it holds suspicious sand, which hides loot.
pub const HIDDEN_TAGS: [&str; 8] = [
    "minecraft:base_stone_overworld",
    "minecraft:base_stone_nether",
    "minecraft:dirt",
    "minecraft:grass_blocks",
    "minecraft:mud",
    "minecraft:moss_blocks",
    "minecraft:nylium",
    "minecraft:terracotta",
];

/// The blocks X-ray hides by id: filler that no tag above holds. Each is named once (the game
/// refuses the same part twice on one block) and none is in a tag above (a block a tag already
/// gives the part would only log a warning).
pub const HIDDEN_BLOCKS: [&str; 12] = [
    "minecraft:calcite",
    "minecraft:dripstone_block",
    "minecraft:smooth_basalt",
    "minecraft:gravel",
    "minecraft:sand",
    "minecraft:red_sand",
    "minecraft:sandstone",
    "minecraft:red_sandstone",
    "minecraft:clay",
    "minecraft:end_stone",
    "minecraft:soul_sand",
    "minecraft:soul_soil",
];

/// Whether a block or tag id may be hidden at all: never an ore (`*_ore`, ancient debris, a tag
/// of ores), a chest of any kind, a spawner of any kind, a vault, suspicious sand or gravel, or a
/// fluid (water, lava, a bubble column). Every id the mod attaches its part to passes this.
pub fn may_hide(id: &str) -> bool {
    let path = id.rsplit(':').next().unwrap_or(id);
    let kept = path.ends_with("_ore")
        || path.ends_with("_ores")
        || path == "ancient_debris"
        || path.contains("chest")
        || path.contains("spawner")
        || path.contains("vault")
        || path.contains("suspicious")
        || path.contains("water")
        || path.contains("lava")
        || path.contains("bubble_column");
    !kept
}

/// The tags the part is attached to.
pub fn hidden_tags() -> impl Iterator<Item = &'static str> {
    HIDDEN_TAGS.into_iter().filter(|id| may_hide(id))
}

/// The blocks the part is attached to by id.
pub fn hidden_blocks() -> impl Iterator<Item = &'static str> {
    HIDDEN_BLOCKS.into_iter().filter(|id| may_hide(id))
}

/// The mod's key, listed under its name on Options > Controls, where the player can rebind it. It
/// acts only while playing.
pub fn key_registration() -> KeyRegistration {
    KeyRegistration::new(KEY, DEFAULT_KEY)
        .label(KEY_LABEL)
        .context(KeyConflictContext::InGame)
}

/// The settings on the mod's page in Options > Mods.
pub fn settings() -> [SettingSpec; 1] {
    [
        SettingSpec::toggle(REMEMBER, "xray_single_player.setting.remember", false)
            .tooltip("xray_single_player.setting.remember.tooltip"),
    ]
}

/// The single-player lock's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lock {
    /// This window owns a save's integrated server, but the game has no Tab list to read: X-ray
    /// waits, silently.
    Waiting,
    /// A closed single-player world: the local player alone in this window's own save, or nobody
    /// listed yet (the first ticks after joining, before your own entry arrives; no one else can
    /// join a world before its owner).
    Open,
    /// Anything else: a server, a splitscreen guest, or a LAN world someone else joined.
    Closed,
}

/// The lock from what the client session says: whether this window owns the integrated server
/// (`integrated_local`), whether that server is a save's (`save_folder`), and how many players the
/// Tab list holds (`None` while it has not been sent).
pub fn lock(integrated_local: bool, has_save_folder: bool, players_listed: Option<usize>) -> Lock {
    if !integrated_local || !has_save_folder {
        return Lock::Closed;
    }
    match players_listed {
        None => Lock::Waiting,
        Some(0 | 1) => Lock::Open,
        Some(_) => Lock::Closed,
    }
}

/// What a step reads from the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    /// The client is in a logged-in world (`client_server()` answers).
    pub in_world: bool,
    /// The player left the mod on in Options > Mods.
    pub mod_on: bool,
    /// The "remember state" setting.
    pub remember: bool,
    /// The key went down this tick.
    pub pressed: bool,
}

/// What a step asks the game to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// Resolve the hidden blocks again (`refresh_render_visibility`).
    pub refresh: bool,
    /// Say in chat that X-ray works only in single player.
    pub warn: bool,
}

/// X-ray's state: whether it hides the filler now, and what the player asked for.
#[derive(Debug, Default)]
pub struct XRay {
    /// The part hides its blocks: what the game is asked to mesh with.
    active: bool,
    /// The player's last choice, for "remember state".
    wanted: bool,
    /// A world session is under way: its first step is done.
    session: bool,
    /// This session may still turn X-ray back on from `wanted` once the lock opens.
    restore: bool,
    /// The key was down at the last read.
    was_down: bool,
}

impl XRay {
    /// Whether the filler is hidden now.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Reads the key: true when it went down since the last read. `clicks` are the clicks it got
    /// since (a press, and each auto-repeat while it stays down), so holding the key flips X-ray
    /// once, not once a repeat, and a tap between two ticks still counts.
    pub fn key(&mut self, down: bool, clicks: u32) -> bool {
        let pressed = !self.was_down && (down || clicks > 0);
        self.was_down = down;
        pressed
    }

    /// One step: a client tick, or a level loading (with no key). `lock` is asked at most once,
    /// and only when the answer matters: a press, X-ray on, or a remembered state to restore.
    pub fn step(&mut self, input: Input, lock: impl FnOnce() -> Lock) -> Step {
        let mut step = Step::default();
        if !input.in_world {
            // Out of a world X-ray is always off, so the next join meshes with nothing hidden
            // before the lock is asked. There is no world to remesh.
            self.active = false;
            self.session = false;
            self.restore = false;
            return step;
        }
        if !self.session {
            self.session = true;
            self.restore = input.remember && self.wanted;
            // A table the last world left behind is resolved again; the game does nothing when
            // nothing changed. A mod switched off asks the game for nothing.
            step.refresh = input.mod_on;
        }
        if !input.mod_on {
            if self.active {
                self.active = false;
                step.refresh = true;
            }
            self.restore = false;
            return step;
        }
        if input.pressed {
            self.restore = false;
            if self.active {
                self.active = false;
                self.wanted = false;
                step.refresh = true;
            } else {
                match lock() {
                    Lock::Open => {
                        self.active = true;
                        self.wanted = true;
                        step.refresh = true;
                    }
                    Lock::Closed => step.warn = true,
                    // No Tab list to read yet: the press is let go, as if it came too early.
                    Lock::Waiting => {}
                }
            }
        } else if self.active {
            if lock() == Lock::Closed {
                // Someone joined, or the world is no longer this window's own. The choice is
                // kept for "remember state", but this session does not turn it back on.
                self.active = false;
                step.refresh = true;
                step.warn = true;
            }
        } else if self.restore {
            match lock() {
                Lock::Open => {
                    self.restore = false;
                    self.active = true;
                    step.refresh = true;
                }
                // Not single player: stays off, without a word, since the player pressed nothing.
                Lock::Closed => self.restore = false,
                Lock::Waiting => {}
            }
        }
        step
    }
}

/// What the part answers: whether X-ray hides its blocks now. The part is a value of its own that
/// the game asks while it resolves the hidden states, so it reads this, not the mod's state.
static HIDING: Mutex<bool> = Mutex::new(false);

fn hiding() -> bool {
    *HIDING.lock().unwrap_or_else(|e| e.into_inner())
}

fn set_hiding(on: bool) {
    *HIDING.lock().unwrap_or_else(|e| e.into_inner()) = on;
}

/// The part on the filler blocks: hides them while X-ray is on. It only ever sets `hidden`, so it
/// never clears another mod's answer.
struct Filler;

#[mod_api::horizon_part]
impl BlockPart for Filler {
    fn block_render_visibility(&self, _: &dyn ClientHost, event: &mut BlockRenderVisibilityEvent) {
        if hiding() {
            event.hidden = true;
        }
    }
}

/// The lock as the client session reads now. The Tab list is asked only of a window that owns a
/// save's integrated server.
fn read_lock(host: &dyn ClientHost, server: &ClientServer) -> Lock {
    let owned = server.integrated_local && server.save_folder.is_some();
    let players = if owned {
        host.client_session()
            .client_tab_list()
            .map(|list| list.entries.len())
    } else {
        None
    };
    lock(
        server.integrated_local,
        server.save_folder.is_some(),
        players,
    )
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
struct XRaySinglePlayer {
    state: Mutex<XRay>,
}

impl XRaySinglePlayer {
    fn state(&self) -> MutexGuard<'_, XRay> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One step with what the game says now, then the refresh and the chat line it asks for.
    /// `key` is the key's read this tick (`is_down`, clicks), none when a level loads.
    fn sync(&self, host: &mut dyn ClientHost, key: Option<(bool, u32)>) {
        let step = {
            let view: &dyn ClientHost = &*host;
            let server = view.client_session().client_server();
            let input = Input {
                in_world: server.is_some(),
                mod_on: view.on(ID),
                remember: view.setting_bool(REMEMBER).unwrap_or(false),
                pressed: false,
            };
            let mut state = self.state();
            let pressed = key.is_some_and(|(down, clicks)| state.key(down, clicks));
            let step = state.step(Input { pressed, ..input }, || {
                server
                    .as_ref()
                    .map_or(Lock::Closed, |server| read_lock(view, server))
            });
            set_hiding(state.active());
            step
            // The state's lock is let go here: the refresh asks the part, which reads only
            // `HIDING`, and no lock is held across it.
        };
        if step.refresh {
            host.refresh_render_visibility();
        }
        if step.warn {
            let line = host
                .translate(SINGLE_PLAYER_ONLY)
                .unwrap_or_else(|| SINGLE_PLAYER_ONLY_FALLBACK.to_owned());
            let _ = host.chat_line(Text::literal(line));
        }
    }
}

#[mod_api::horizon_mod]
impl Hooks for XRaySinglePlayer {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.register_key_mapping(key_registration());
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
        for tag in hidden_tags() {
            r.block_part_tag(tag, Filler);
        }
        for id in hidden_blocks() {
            let block = r.block_key(id);
            r.block_part(block, Filler);
        }
    }

    /// The key and the lock, once a client tick.
    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        let clicks = take_clicks(host);
        let down = host.is_down(ID, KEY);
        self.sync(host, Some((down, clicks)));
    }

    /// A level loaded (joining a world, or changing dimension): the lock is checked at once.
    fn client_level_load(&self, host: &mut dyn ClientHost, _: &mut ClientLevelLoadEvent) {
        self.sync(host, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In this player's own single-player world, alone.
    const ALONE: Lock = Lock::Open;

    fn tick(pressed: bool) -> Input {
        Input {
            in_world: true,
            mod_on: true,
            remember: false,
            pressed,
        }
    }

    fn remembering(pressed: bool) -> Input {
        Input {
            remember: true,
            ..tick(pressed)
        }
    }

    const TITLE: Input = Input {
        in_world: false,
        mod_on: true,
        remember: false,
        pressed: false,
    };

    const REFRESH: Step = Step {
        refresh: true,
        warn: false,
    };
    const NOTHING: Step = Step {
        refresh: false,
        warn: false,
    };
    const WARN: Step = Step {
        refresh: false,
        warn: true,
    };

    fn never() -> Lock {
        panic!("the lock was asked when its answer did not matter")
    }

    /// A state already one tick into a world, with X-ray off.
    fn joined() -> XRay {
        let mut xray = XRay::default();
        assert_eq!(xray.step(tick(false), never), REFRESH);
        xray
    }

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
        assert_eq!(settings()[0].key, REMEMBER);
    }

    #[test]
    fn the_key_is_x_while_playing() {
        let key = key_registration();
        assert_eq!(key.id, KEY);
        assert_eq!(key.key, "key.keyboard.x");
        assert_eq!(key.label, KEY_LABEL);
        assert_eq!(key.context, KeyConflictContext::InGame);
    }

    #[test]
    fn the_lock_opens_only_alone_in_your_own_save() {
        assert_eq!(lock(true, true, Some(1)), Lock::Open);
        // A LAN guest joined: the host's Tab list holds two.
        assert_eq!(lock(true, true, Some(2)), Lock::Closed);
        assert_eq!(lock(true, true, Some(8)), Lock::Closed);
        // A server, local address or not.
        assert_eq!(lock(false, false, Some(1)), Lock::Closed);
        assert_eq!(lock(false, false, None), Lock::Closed);
        // A splitscreen guest does not own the server and has no save folder.
        assert_eq!(lock(false, true, Some(1)), Lock::Closed);
        assert_eq!(lock(true, false, Some(1)), Lock::Closed);
        // The first ticks after joining your own save: nobody listed yet, so nobody else.
        assert_eq!(lock(true, true, Some(0)), Lock::Open);
        // No Tab list to read at all: wait.
        assert_eq!(lock(true, true, None), Lock::Waiting);
        assert_eq!(lock(false, false, Some(0)), Lock::Closed);
    }

    #[test]
    fn the_key_turns_it_on_and_off_in_single_player() {
        let mut xray = joined();
        assert!(!xray.active());
        assert_eq!(xray.step(tick(true), || ALONE), REFRESH);
        assert!(xray.active());
        // While on, each tick checks the lock: still alone, nothing to do.
        assert_eq!(xray.step(tick(false), || ALONE), NOTHING);
        assert!(xray.active());
        assert_eq!(xray.step(tick(true), never), REFRESH);
        assert!(!xray.active());
        assert_eq!(xray.step(tick(false), never), NOTHING);
    }

    #[test]
    fn a_server_refuses_with_one_line_a_press() {
        let mut xray = joined();
        assert_eq!(xray.step(tick(true), || Lock::Closed), WARN);
        assert!(!xray.active());
        // No press, no line: the lock is not even asked while X-ray is off.
        for _ in 0..100 {
            assert_eq!(xray.step(tick(false), never), NOTHING);
        }
        assert_eq!(xray.step(tick(true), || Lock::Closed), WARN);
        assert!(!xray.active());
    }

    #[test]
    fn a_press_while_still_joining_is_let_go() {
        let mut xray = joined();
        assert_eq!(xray.step(tick(true), || Lock::Waiting), NOTHING);
        assert!(!xray.active());
        assert_eq!(xray.step(tick(true), || ALONE), REFRESH);
        assert!(xray.active());
    }

    #[test]
    fn someone_joining_turns_it_off_with_one_line() {
        let mut xray = joined();
        xray.step(tick(true), || ALONE);
        assert_eq!(xray.step(tick(false), || Lock::Closed), REFRESH_AND_WARN);
        assert!(!xray.active());
        // Said once: off now, the lock is no longer asked.
        for _ in 0..100 {
            assert_eq!(xray.step(tick(false), never), NOTHING);
        }
        // The guest left: X-ray stays off until the key is pressed again.
        assert_eq!(xray.step(tick(true), || ALONE), REFRESH);
        assert!(xray.active());
    }

    const REFRESH_AND_WARN: Step = Step {
        refresh: true,
        warn: true,
    };

    #[test]
    fn no_tab_list_while_on_is_not_someone_joining() {
        let mut xray = joined();
        xray.step(tick(true), || ALONE);
        assert_eq!(xray.step(tick(false), || Lock::Waiting), NOTHING);
        assert!(xray.active());
    }

    #[test]
    fn leaving_the_world_turns_it_off_before_the_next_join() {
        let mut xray = joined();
        xray.step(tick(true), || ALONE);
        assert!(xray.active());
        // Out of a world: off at once, nothing to remesh, the lock not asked.
        assert_eq!(xray.step(TITLE, never), NOTHING);
        assert!(!xray.active());
        assert_eq!(xray.step(TITLE, never), NOTHING);
        // The next world starts with one refresh, which clears what the last one left, and off.
        assert_eq!(xray.step(tick(false), never), REFRESH);
        assert!(!xray.active());
    }

    #[test]
    fn without_remember_each_world_starts_off() {
        let mut xray = joined();
        xray.step(tick(true), || ALONE);
        xray.step(TITLE, never);
        assert_eq!(xray.step(tick(false), never), REFRESH);
        for _ in 0..10 {
            assert_eq!(xray.step(tick(false), never), NOTHING);
        }
        assert!(!xray.active());
    }

    #[test]
    fn remember_turns_it_back_on_in_the_next_single_player_world() {
        let mut xray = XRay::default();
        xray.step(remembering(false), never);
        xray.step(remembering(true), || ALONE);
        xray.step(TITLE, never);
        // Joining: no Tab list to read at first, so it waits without a word.
        assert_eq!(xray.step(remembering(false), || Lock::Waiting), REFRESH);
        assert!(!xray.active());
        assert_eq!(xray.step(remembering(false), || Lock::Waiting), NOTHING);
        assert_eq!(xray.step(remembering(false), || ALONE), REFRESH);
        assert!(xray.active());
        assert_eq!(xray.step(remembering(false), || ALONE), NOTHING);
    }

    #[test]
    fn remember_stays_off_on_a_server_without_a_word() {
        let mut xray = XRay::default();
        xray.step(remembering(false), never);
        xray.step(remembering(true), || ALONE);
        xray.step(TITLE, never);
        assert_eq!(xray.step(remembering(false), || Lock::Closed), REFRESH);
        assert!(!xray.active());
        for _ in 0..10 {
            assert_eq!(xray.step(remembering(false), never), NOTHING);
        }
        // Back in single player, the choice is still remembered.
        xray.step(TITLE, never);
        assert_eq!(xray.step(remembering(false), || ALONE), REFRESH);
        assert!(xray.active());
    }

    #[test]
    fn remember_keeps_an_off_choice_off() {
        let mut xray = XRay::default();
        xray.step(remembering(false), never);
        xray.step(remembering(true), || ALONE);
        xray.step(remembering(true), never);
        assert!(!xray.active());
        xray.step(TITLE, never);
        assert_eq!(xray.step(remembering(false), never), REFRESH);
        assert_eq!(xray.step(remembering(false), never), NOTHING);
        assert!(!xray.active());
    }

    #[test]
    fn switching_the_mod_off_turns_it_off_once() {
        let mut xray = joined();
        xray.step(tick(true), || ALONE);
        let off = Input {
            mod_on: false,
            ..tick(false)
        };
        assert_eq!(xray.step(off, never), REFRESH);
        assert!(!xray.active());
        assert_eq!(xray.step(off, never), NOTHING);
        // A press while the mod is off does nothing.
        let press = Input {
            pressed: true,
            ..off
        };
        assert_eq!(xray.step(press, never), NOTHING);
        assert!(!xray.active());
    }

    #[test]
    fn a_world_joined_with_the_mod_off_asks_nothing() {
        let off = Input {
            mod_on: false,
            ..remembering(false)
        };
        let mut xray = XRay::default();
        assert_eq!(xray.step(off, never), NOTHING);
        assert_eq!(
            xray.step(
                Input {
                    pressed: true,
                    ..off
                },
                never
            ),
            NOTHING
        );
        assert!(!xray.active());
        // Switched on again in the same world: the key works as usual.
        assert_eq!(xray.step(remembering(true), || ALONE), REFRESH);
        assert!(xray.active());
    }

    #[test]
    fn holding_the_key_flips_once_and_a_tap_counts() {
        let mut xray = XRay::default();
        assert!(xray.key(true, 1));
        // Held: its auto-repeat clicks, but it is one press.
        for _ in 0..20 {
            assert!(!xray.key(true, 1));
        }
        assert!(!xray.key(false, 0));
        // Down and up again between two ticks: one click, not down now.
        assert!(xray.key(false, 1));
        assert!(!xray.key(false, 0));
        assert!(xray.key(true, 0));
    }

    #[test]
    fn nothing_hidden_is_an_ore_a_chest_a_spawner_or_a_fluid() {
        for id in HIDDEN_TAGS.iter().chain(HIDDEN_BLOCKS.iter()) {
            assert!(may_hide(id), "{id}");
            let path = id.strip_prefix("minecraft:").expect("a vanilla id");
            for kept in ["ore", "debris", "chest", "spawner", "water", "lava"] {
                assert!(!path.contains(kept), "{id} looks like {kept}");
            }
        }
        assert_eq!(hidden_tags().count(), HIDDEN_TAGS.len());
        assert_eq!(hidden_blocks().count(), HIDDEN_BLOCKS.len());
    }

    #[test]
    fn may_hide_keeps_what_x_ray_is_for() {
        for id in [
            "minecraft:diamond_ore",
            "minecraft:deepslate_diamond_ore",
            "minecraft:nether_quartz_ore",
            "minecraft:nether_gold_ore",
            "minecraft:ancient_debris",
            "minecraft:coal_ores",
            "minecraft:chest",
            "minecraft:trapped_chest",
            "minecraft:ender_chest",
            "minecraft:spawner",
            "minecraft:trial_spawner",
            "minecraft:vault",
            "minecraft:suspicious_sand",
            "minecraft:suspicious_gravel",
            "minecraft:water",
            "minecraft:lava",
            "minecraft:bubble_column",
            "mymod:tin_ore",
        ] {
            assert!(!may_hide(id), "{id}");
        }
        for id in [
            "minecraft:stone",
            "minecraft:deepslate",
            "stone",
            "mymod:slate",
        ] {
            assert!(may_hide(id), "{id}");
        }
    }

    #[test]
    fn each_id_and_tag_is_named_once_and_namespaced() {
        let mut all: Vec<&str> = HIDDEN_TAGS
            .iter()
            .chain(HIDDEN_BLOCKS.iter())
            .copied()
            .collect();
        all.sort_unstable();
        let count = all.len();
        all.dedup();
        assert_eq!(all.len(), count, "an id or tag is named twice");
        for id in HIDDEN_TAGS.iter().chain(HIDDEN_BLOCKS.iter()) {
            assert!(!id.starts_with('#'), "{id}");
            let (namespace, path) = id.split_once(':').expect("namespaced");
            assert!(!namespace.is_empty() && !path.is_empty(), "{id}");
        }
    }

    #[test]
    fn the_part_hides_only_while_x_ray_is_on() {
        set_hiding(true);
        assert!(hiding());
        set_hiding(false);
        assert!(!hiding());
    }
}
