//! Toggle Sprint: press a key once to keep sprinting (V by default) or sneaking (C), instead of
//! holding the game's sprint or sneak key. While a toggle is on, the mod holds the game's own
//! `key.sprint` or `key.sneak` mapping down, so the game moves the player exactly as it does for a
//! held key, with all of its checks and packets. A screen lets go of what the mod holds (so
//! sneaking stops in an inventory) and the mod holds it again once the screen closes; dying,
//! leaving the world or switching the mod off turns both toggles off. A small line in a corner
//! says what is on, as the classic ToggleSprint HUD did: `[Sprinting (Toggled)]`,
//! `[Sneaking (Key Held)]`, `[Flying]`, `[Gliding]`...
use std::sync::{Mutex, MutexGuard};

use mod_api::{
    ClientHost, ClientTickPostEvent, ComputeFovModifierEvent, Corner, Hooks, KeyConflictContext,
    KeyRegistration, Registrar, RegistrationContext, RenderGuiPostEvent, SettingSpec,
};

/// The mod's id: its settings folder, its keys' owner and its translation keys.
pub const ID: &str = "toggle_sprint";
/// The WebAssembly mods' panel, as Coordinates HUD draws it: translucent black, white text.
pub const BACKGROUND: u32 = 0x7800_0000;
pub const INK: u32 = 0xFFFF_FFFF;
/// A line's height and the text's inset, in GUI pixels.
pub const LINE: f32 = 10.0;
pub const PADDING: f32 = 2.0;
/// The corners in the order the corner setting lists them.
pub const CORNERS: [Corner; 4] = Corner::ALL;
/// The most clicks of one key taken in a tick. A key's auto-repeat adds about one a tick, so this
/// is never reached by a player; it only bounds the loop.
pub const MAX_CLICKS: u32 = 32;

/// The settings on Toggle Sprint's page in Options > Mods.
pub fn settings() -> [SettingSpec; 2] {
    [
        SettingSpec::toggle("shown", "toggle_sprint.setting.shown", true)
            .tooltip("toggle_sprint.setting.shown.tooltip"),
        SettingSpec::choice(
            "corner",
            "toggle_sprint.setting.corner",
            CORNERS.map(|c| format!("toggle_sprint.corner.{}", c.name())),
            0,
        )
        .tooltip("toggle_sprint.setting.corner.tooltip"),
    ]
}

/// The corner the corner setting picks (`setting_choice`), top left when it reads nothing.
pub fn corner_of(choice: Option<u32>) -> Corner {
    CORNERS[choice.map_or(0, |i| i as usize).min(CORNERS.len() - 1)]
}

/// The two toggles, in the order the mod registers their keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sprint,
    Sneak,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Sprint, Kind::Sneak];

    /// Its place in the per-kind arrays.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The id of the mod's own key for it (its line in `horizon-mods.txt`).
    pub fn key_id(self) -> &'static str {
        match self {
            Kind::Sprint => "toggle_sprint",
            Kind::Sneak => "toggle_sneak",
        }
    }

    /// The translation key of the key's name on Options > Controls.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Sprint => "key.toggle_sprint.toggle_sprint",
            Kind::Sneak => "key.toggle_sprint.toggle_sneak",
        }
    }

    /// The key it is bound to until the player changes it.
    pub fn default_key(self) -> &'static str {
        match self {
            Kind::Sprint => "key.keyboard.v",
            Kind::Sneak => "key.keyboard.c",
        }
    }

    /// The game's own mapping the toggle holds down.
    pub fn vanilla(self) -> &'static str {
        match self {
            Kind::Sprint => "key.sprint",
            Kind::Sneak => "key.sneak",
        }
    }
}

/// The mod's two keys, listed under its name on Options > Controls. They act only while playing.
pub fn key_registrations() -> [KeyRegistration; 2] {
    Kind::ALL.map(|kind| {
        KeyRegistration::new(kind.key_id(), kind.default_key())
            .label(kind.label())
            .context(KeyConflictContext::InGame)
    })
}

/// One of the mod's keys since the last tick: held now (`is_down`), and the clicks it got
/// (`consume_click`: a press, and each auto-repeat while the key stays down).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyRead {
    pub down: bool,
    pub clicks: u32,
}

/// The game's own sprint or sneak mapping: held now (by its key, the game's toggle or this mod),
/// and whether the game's own "Sprint" or "Sneak: Toggle" option is on, which makes a press of
/// the mapping flip it and a release do nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vanilla {
    pub down: bool,
    pub toggle_option: bool,
}

/// What the toggles may do this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The player is in a world, alive, with the mod on. `playing` is false while a screen or an
    /// overlay covers the world: the toggles stay on but hold nothing until it closes.
    Live { playing: bool },
    /// The player died, or the mod was switched off: both toggles turn off and let go.
    Stop,
    /// No player in a world: both toggles turn off, and there is nothing to let go.
    Gone,
}

/// What a tick reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub mode: Mode,
    /// The mod's keys, by [`Kind::index`].
    pub keys: [KeyRead; 2],
    /// The game's sprint and sneak mappings, by [`Kind::index`].
    pub vanilla: [Vanilla; 2],
}

/// What holds a mapping down, for the status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    Toggled,
    KeyHeld,
}

/// What a tick does, by [`Kind::index`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// The `set_mapping_down(kind.vanilla(), down)` call to make, if any.
    pub calls: [Option<bool>; 2],
    /// What holds the mapping down once the calls are made.
    pub held: [Option<Held>; 2],
}

/// The call that holds a game mapping down: a press, only while playing and only when it is up
/// (a press of a held mapping would flip it up again with the game's toggle option on).
pub fn hold(vanilla: Vanilla, playing: bool) -> Option<bool> {
    (playing && !vanilla.down).then_some(true)
}

/// The call that lets a game mapping go, once, when a toggle turns off: none when it is up
/// already, a release normally, and a press with the game's toggle option on, where a release does
/// nothing and a press flips it up.
pub fn release(vanilla: Vanilla) -> Option<bool> {
    vanilla.down.then_some(vanilla.toggle_option)
}

/// What holds a game mapping when the mod's toggle is off: the game's own toggle (its option on),
/// or the player's key.
pub fn held_by_player(vanilla: Vanilla) -> Option<Held> {
    vanilla.down.then_some(if vanilla.toggle_option {
        Held::Toggled
    } else {
        Held::KeyHeld
    })
}

/// The two toggles, and whether each key was down at the last tick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Toggles {
    on: [bool; 2],
    was_down: [bool; 2],
}

impl Toggles {
    /// Whether the toggle of `kind` is on.
    pub fn on(&self, kind: Kind) -> bool {
        self.on[kind.index()]
    }

    /// One client tick. A key flips its toggle when it goes down: a click while it was up at the
    /// last tick (a tap between two ticks too), so its auto-repeat while held flips nothing. A
    /// toggle that is on holds its game mapping down while playing; one that turns off lets it go
    /// once. The player's own key is never touched while the toggle is off.
    pub fn step(&mut self, input: &Input) -> Step {
        let mut step = Step::default();
        for kind in Kind::ALL {
            let i = kind.index();
            let key = input.keys[i];
            let vanilla = input.vanilla[i];
            let pressed = !self.was_down[i] && (key.down || key.clicks > 0);
            self.was_down[i] = key.down;
            let was_on = self.on[i];
            match input.mode {
                // Behind a screen the game gives the keys no press; none counts there either.
                Mode::Live { playing } => {
                    if pressed && playing {
                        self.on[i] = !was_on;
                    }
                }
                Mode::Stop | Mode::Gone => self.on[i] = false,
            }
            (step.calls[i], step.held[i]) = if self.on[i] {
                let playing = input.mode == Mode::Live { playing: true };
                (hold(vanilla, playing), Some(Held::Toggled))
            } else if was_on {
                let call = match input.mode {
                    Mode::Gone => None,
                    Mode::Live { .. } | Mode::Stop => release(vanilla),
                };
                (call, None)
            } else {
                (None, held_by_player(vanilla))
            };
        }
        step
    }
}

/// What the player is doing besides sprinting and sneaking, as a client can read it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Movement {
    /// Flying (`Abilities.flying`): creative or spectator flight.
    pub flying: bool,
    /// Gliding on elytra (`LivingEntity.isFallFlying`).
    pub gliding: bool,
}

/// What the status line says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Flying,
    Gliding,
    Sneaking(Held),
    Sprinting(Held),
}

impl Status {
    /// The line, as the classic ToggleSprint HUD wrote it.
    pub fn text(self) -> &'static str {
        match self {
            Status::Flying => "[Flying]",
            Status::Gliding => "[Gliding]",
            Status::Sneaking(Held::Toggled) => "[Sneaking (Toggled)]",
            Status::Sneaking(Held::KeyHeld) => "[Sneaking (Key Held)]",
            Status::Sprinting(Held::Toggled) => "[Sprinting (Toggled)]",
            Status::Sprinting(Held::KeyHeld) => "[Sprinting (Key Held)]",
        }
    }
}

/// The status line for a moment: flying first, then gliding, then sneaking (which stops a
/// sprint), then sprinting; none when nothing is on.
pub fn status(movement: Movement, held: [Option<Held>; 2]) -> Option<Status> {
    if movement.flying {
        Some(Status::Flying)
    } else if movement.gliding {
        Some(Status::Gliding)
    } else {
        held[Kind::Sneak.index()]
            .map(Status::Sneaking)
            .or(held[Kind::Sprint.index()].map(Status::Sprinting))
    }
}

/// The panel around a line `width` GUI pixels wide: `[w, h]`.
pub fn size(width: f32) -> [f32; 2] {
    [width + 2.0 * PADDING, LINE + PADDING]
}

#[derive(Default)]
struct Shared {
    toggles: Toggles,
    /// The local player flies, as the last field-of-view update read it.
    flying: bool,
    /// The status line, worked out each client tick.
    status: Option<Status>,
    /// The width of the line last measured, and the status it was measured for.
    measured: Option<(Status, f32)>,
}

#[derive(Default)]
struct ToggleSprint {
    // Never held across a host call: a host call can fire another of this mod's hooks.
    shared: Mutex<Shared>,
}

impl ToggleSprint {
    fn shared(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Takes the clicks the mod's key for `kind` got since the last tick, at most [`MAX_CLICKS`].
fn take_clicks(host: &mut dyn ClientHost, kind: Kind) -> u32 {
    let mut clicks = 0;
    while clicks < MAX_CLICKS && host.consume_click(ID, kind.key_id()) {
        clicks += 1;
    }
    clicks
}

/// Whether the game's mapping `name` is held, by its key, the game's toggle or a mod.
fn vanilla_down(host: &dyn ClientHost, name: &str) -> bool {
    matches!(host.mapping(name), Ok(Some(mapping)) if mapping.down)
}

#[mod_api::horizon_mod]
impl Hooks for ToggleSprint {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for key in key_registrations() {
            r.register_key_mapping(key);
        }
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// The keys, the toggles and the holds, once a client tick; the status line is worked out
    /// here too, so a frame only draws it.
    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        // Taken every tick, the mod on or off, so no old press flips a toggle later.
        let clicks = Kind::ALL.map(|kind| take_clicks(host, kind));
        let view: &dyn ClientHost = &*host;
        let player = view.local_player();
        let living = player.and_then(|player| view.living(player.entity()));
        let dead = living
            .and_then(|living| living.health(view))
            .is_some_and(|health| health <= 0.0);
        let gliding = living
            .and_then(|living| living.is_gliding(view))
            .unwrap_or(false);
        let mode = if player.is_none() {
            Mode::Gone
        } else if dead || !view.on(ID) {
            Mode::Stop
        } else {
            Mode::Live {
                playing: view.playing(),
            }
        };
        let options = view.input_options();
        let toggle_options = [options.toggle_sprint, options.toggle_crouch];
        let mut input = Input {
            mode,
            keys: [KeyRead::default(); 2],
            vanilla: [Vanilla::default(); 2],
        };
        for kind in Kind::ALL {
            let i = kind.index();
            input.keys[i] = KeyRead {
                down: view.is_down(ID, kind.key_id()),
                clicks: clicks[i],
            };
            input.vanilla[i] = Vanilla {
                down: player.is_some() && vanilla_down(view, kind.vanilla()),
                toggle_option: toggle_options[i],
            };
        }
        let calls = {
            let mut shared = self.shared();
            let step = shared.toggles.step(&input);
            if mode == Mode::Gone {
                shared.flying = false;
            }
            let movement = Movement {
                flying: shared.flying,
                gliding,
            };
            shared.status = match mode {
                Mode::Live { .. } => status(movement, step.held),
                Mode::Stop | Mode::Gone => None,
            };
            step.calls
        };
        for kind in Kind::ALL {
            if let Some(down) = calls[kind.index()] {
                // Refused only with no player in a world, where there is nothing to hold.
                let _ = host.set_mapping_down(kind.vanilla(), down);
            }
        }
    }

    /// Whether the player flies, from the field-of-view inputs the game reads each client tick
    /// with a player: the flight a double jump starts shows here at once, where the abilities the
    /// server last sent would not.
    fn compute_fov_modifier(&self, _: &dyn ClientHost, event: &mut ComputeFovModifierEvent) {
        self.shared().flying = event.player.flying;
    }

    /// The status line in the corner picked, below what the corner holds already; none while
    /// F3's debug screen takes that corner, or when nothing is on.
    fn render_gui_post(&self, host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
        if !host.on(ID) || !host.setting_bool("shown").unwrap_or(true) {
            return;
        }
        let corner = corner_of(host.setting_choice("corner"));
        if !host.native_corner_shown(corner) || host.local_player().is_none() {
            return;
        }
        let (status, measured) = {
            let shared = self.shared();
            (shared.status, shared.measured)
        };
        let Some(status) = status else {
            return;
        };
        let text = status.text();
        let draw = &mut *event.draw;
        let width = match measured {
            Some((shown, width)) if shown == status => width,
            _ => {
                let width = draw.text_width(text);
                self.shared().measured = Some((status, width));
                width
            }
        };
        let [w, h] = size(width);
        let (x, y) = event.corners.place(corner, w, h);
        draw.fill([x, y, w, h], BACKGROUND);
        draw.text(text, [x + PADDING, y + PADDING], INK, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::SettingKind;

    const UP: KeyRead = KeyRead {
        down: false,
        clicks: 0,
    };
    /// The key went down since the last tick.
    const PRESS: KeyRead = KeyRead {
        down: true,
        clicks: 1,
    };
    /// The key stays down, its auto-repeat clicking.
    const REPEAT: KeyRead = KeyRead {
        down: true,
        clicks: 1,
    };
    /// The key went down and up again between two ticks.
    const TAP: KeyRead = KeyRead {
        down: false,
        clicks: 1,
    };
    const FREE: Vanilla = Vanilla {
        down: false,
        toggle_option: false,
    };
    const HELD: Vanilla = Vanilla {
        down: true,
        toggle_option: false,
    };
    const FLIP_UP: Vanilla = Vanilla {
        down: false,
        toggle_option: true,
    };
    const FLIP_DOWN: Vanilla = Vanilla {
        down: true,
        toggle_option: true,
    };
    const PLAYING: Mode = Mode::Live { playing: true };
    const SCREEN: Mode = Mode::Live { playing: false };

    /// A tick with the sprint key `key` and the game's sprint mapping `vanilla`; sneak untouched.
    fn sprint(mode: Mode, key: KeyRead, vanilla: Vanilla) -> Input {
        Input {
            mode,
            keys: [key, UP],
            vanilla: [vanilla, FREE],
        }
    }

    /// A tick with the sneak key `key` and the game's sneak mapping `vanilla`; sprint untouched.
    fn sneak(mode: Mode, key: KeyRead, vanilla: Vanilla) -> Input {
        Input {
            mode,
            keys: [UP, key],
            vanilla: [FREE, vanilla],
        }
    }

    fn calls(sprint: Option<bool>, sneak: Option<bool>) -> [Option<bool>; 2] {
        [sprint, sneak]
    }

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
        assert_eq!(corner_of(None), Corner::TopLeft);
        assert_eq!(corner_of(Some(0)), Corner::TopLeft);
        assert_eq!(corner_of(Some(3)), Corner::BottomRight);
        assert_eq!(corner_of(Some(99)), Corner::BottomRight);
    }

    /// The registrar refuses startup for an empty id or one with another character than
    /// `A-Z a-z 0-9 _ . -`, and for an id the mod registered twice.
    #[test]
    fn the_keys_are_v_and_c_with_ids_the_registrar_takes() {
        let keys = key_registrations();
        assert_eq!(keys[0].id, "toggle_sprint");
        assert_eq!(keys[0].key, "key.keyboard.v");
        assert_eq!(keys[0].label, "key.toggle_sprint.toggle_sprint");
        assert_eq!(keys[1].id, "toggle_sneak");
        assert_eq!(keys[1].key, "key.keyboard.c");
        assert_eq!(keys[1].label, "key.toggle_sprint.toggle_sneak");
        assert_ne!(keys[0].id, keys[1].id);
        for key in &keys {
            assert!(!key.id.is_empty());
            assert!(
                key.id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
                "{}",
                key.id
            );
            assert_eq!(key.context, KeyConflictContext::InGame);
            assert_eq!(key.category, "");
        }
        assert_eq!(Kind::Sprint.vanilla(), "key.sprint");
        assert_eq!(Kind::Sneak.vanilla(), "key.sneak");
    }

    #[test]
    fn every_translation_key_is_in_the_language_file() {
        let lang = include_str!("../assets/toggle_sprint/lang/en_us.json");
        let mut keys: Vec<String> = Kind::ALL.map(|k| k.label().to_owned()).to_vec();
        for setting in settings() {
            keys.push(setting.label.clone());
            keys.extend(setting.tooltip.clone());
            if let SettingKind::Choice { options, .. } = &setting.kind {
                keys.extend(options.iter().cloned());
            }
        }
        assert_eq!(keys.len(), 10);
        for key in keys {
            assert!(lang.contains(&format!("\"{key}\":")), "{key}");
        }
    }

    #[test]
    fn a_press_turns_a_toggle_on_and_holds_the_mapping() {
        let mut toggles = Toggles::default();
        let step = toggles.step(&sprint(PLAYING, PRESS, FREE));
        assert!(toggles.on(Kind::Sprint));
        assert!(!toggles.on(Kind::Sneak));
        assert_eq!(step.calls, calls(Some(true), None));
        assert_eq!(step.held, [Some(Held::Toggled), None]);
        // Held down now: no call while it stays down, a press again if something let it go.
        let step = toggles.step(&sprint(PLAYING, UP, HELD));
        assert_eq!(step.calls, calls(None, None));
        assert_eq!(step.held, [Some(Held::Toggled), None]);
        let step = toggles.step(&sprint(PLAYING, UP, FREE));
        assert_eq!(step.calls, calls(Some(true), None));
    }

    #[test]
    fn a_second_press_turns_it_off_and_lets_go_once() {
        let mut toggles = Toggles::default();
        toggles.step(&sneak(PLAYING, PRESS, FREE));
        toggles.step(&sneak(PLAYING, UP, HELD));
        let step = toggles.step(&sneak(PLAYING, PRESS, HELD));
        assert!(!toggles.on(Kind::Sneak));
        assert_eq!(step.calls, calls(None, Some(false)));
        assert_eq!(step.held, [None, None]);
        // Let go once: the player's own key is left alone afterwards.
        let step = toggles.step(&sneak(PLAYING, UP, HELD));
        assert_eq!(step.calls, calls(None, None));
        assert_eq!(step.held, [None, Some(Held::KeyHeld)]);
        let step = toggles.step(&sneak(PLAYING, UP, FREE));
        assert_eq!(step, Step::default());
    }

    /// Something else let the mapping go (a screen that just opened): turning the toggle off
    /// then has nothing to let go.
    #[test]
    fn a_toggle_already_let_go_needs_no_release() {
        let mut toggles = Toggles::default();
        toggles.step(&sprint(PLAYING, PRESS, FREE));
        toggles.step(&sprint(PLAYING, UP, FREE));
        let step = toggles.step(&sprint(PLAYING, PRESS, FREE));
        assert!(!toggles.on(Kind::Sprint));
        assert_eq!(step.calls, calls(None, None));
        assert_eq!(step.held, [None, None]);
    }

    #[test]
    fn holding_the_key_flips_it_once() {
        let mut toggles = Toggles::default();
        toggles.step(&sprint(PLAYING, PRESS, FREE));
        for _ in 0..40 {
            toggles.step(&sprint(PLAYING, REPEAT, HELD));
            assert!(toggles.on(Kind::Sprint));
        }
        toggles.step(&sprint(PLAYING, UP, HELD));
        assert!(toggles.on(Kind::Sprint));
        toggles.step(&sprint(PLAYING, PRESS, HELD));
        assert!(!toggles.on(Kind::Sprint));
    }

    #[test]
    fn a_tap_between_two_ticks_counts() {
        let mut toggles = Toggles::default();
        toggles.step(&sprint(PLAYING, TAP, FREE));
        assert!(toggles.on(Kind::Sprint));
        let step = toggles.step(&sprint(PLAYING, TAP, HELD));
        assert!(!toggles.on(Kind::Sprint));
        assert_eq!(step.calls, calls(Some(false), None));
        // Two clicks in one tick are still one press.
        let step = toggles.step(&sprint(
            PLAYING,
            KeyRead {
                down: true,
                clicks: 2,
            },
            FREE,
        ));
        assert!(toggles.on(Kind::Sprint));
        assert_eq!(step.calls, calls(Some(true), None));
    }

    #[test]
    fn the_two_toggles_are_apart() {
        let mut toggles = Toggles::default();
        let step = toggles.step(&Input {
            mode: PLAYING,
            keys: [PRESS, PRESS],
            vanilla: [FREE, FREE],
        });
        assert!(toggles.on(Kind::Sprint) && toggles.on(Kind::Sneak));
        assert_eq!(step.calls, calls(Some(true), Some(true)));
        let step = toggles.step(&Input {
            mode: PLAYING,
            keys: [UP, UP],
            vanilla: [HELD, HELD],
        });
        assert_eq!(step.calls, calls(None, None));
        let step = toggles.step(&Input {
            mode: PLAYING,
            keys: [UP, TAP],
            vanilla: [HELD, HELD],
        });
        assert!(toggles.on(Kind::Sprint));
        assert!(!toggles.on(Kind::Sneak));
        assert_eq!(step.calls, calls(None, Some(false)));
    }

    /// Opening a screen lets go of every mapping (the game does it); the toggle stays on and holds
    /// nothing until the screen closes, so sneaking stops in an inventory.
    #[test]
    fn a_screen_pauses_the_hold_and_closing_it_holds_again() {
        let mut toggles = Toggles::default();
        toggles.step(&sneak(PLAYING, PRESS, FREE));
        toggles.step(&sneak(PLAYING, UP, HELD));
        for _ in 0..5 {
            let step = toggles.step(&sneak(SCREEN, UP, FREE));
            assert!(toggles.on(Kind::Sneak));
            assert_eq!(step.calls, calls(None, None));
            assert_eq!(step.held, [None, Some(Held::Toggled)]);
        }
        let step = toggles.step(&sneak(PLAYING, UP, FREE));
        assert_eq!(step.calls, calls(None, Some(true)));
        // A click behind a screen flips nothing (the game gives none there anyway).
        let step = toggles.step(&sneak(SCREEN, TAP, FREE));
        assert!(toggles.on(Kind::Sneak));
        assert_eq!(step.calls, calls(None, None));
        let mut off = Toggles::default();
        off.step(&sprint(SCREEN, TAP, FREE));
        assert!(!off.on(Kind::Sprint));
    }

    /// With the game's own toggle option on, a press flips the mapping and a release does
    /// nothing: hold only when it is up, and let go with a press when it is down.
    #[test]
    fn with_the_games_toggle_option_the_mapping_is_flipped() {
        let mut toggles = Toggles::default();
        let step = toggles.step(&sprint(PLAYING, PRESS, FLIP_UP));
        assert_eq!(step.calls, calls(Some(true), None));
        let step = toggles.step(&sprint(PLAYING, REPEAT, FLIP_DOWN));
        assert_eq!(
            step.calls,
            calls(None, None),
            "a press now would flip it up"
        );
        toggles.step(&sprint(PLAYING, UP, FLIP_DOWN));
        let step = toggles.step(&sprint(PLAYING, PRESS, FLIP_DOWN));
        assert!(!toggles.on(Kind::Sprint));
        assert_eq!(step.calls, calls(Some(true), None), "a press flips it up");
        assert_eq!(release(FLIP_UP), None);
        assert_eq!(release(FLIP_DOWN), Some(true));
        assert_eq!(release(HELD), Some(false));
        assert_eq!(release(FREE), None);
        assert_eq!(hold(FLIP_UP, true), Some(true));
        assert_eq!(hold(FLIP_DOWN, true), None);
        assert_eq!(hold(FREE, false), None);
    }

    #[test]
    fn dying_turns_both_off_and_lets_go() {
        let mut toggles = Toggles::default();
        toggles.step(&Input {
            mode: PLAYING,
            keys: [PRESS, PRESS],
            vanilla: [FREE, FREE],
        });
        let step = toggles.step(&Input {
            mode: Mode::Stop,
            keys: [UP, UP],
            vanilla: [HELD, FLIP_DOWN],
        });
        assert!(!toggles.on(Kind::Sprint) && !toggles.on(Kind::Sneak));
        assert_eq!(step.calls, calls(Some(false), Some(true)));
        // Dead: presses count for nothing, and there is nothing more to let go.
        for _ in 0..3 {
            let step = toggles.step(&Input {
                mode: Mode::Stop,
                keys: [TAP, TAP],
                vanilla: [FREE, FREE],
            });
            assert!(!toggles.on(Kind::Sprint) && !toggles.on(Kind::Sneak));
            assert_eq!(step.calls, calls(None, None));
        }
        // Respawned, both stay off until pressed again.
        let step = toggles.step(&Input {
            mode: PLAYING,
            keys: [UP, UP],
            vanilla: [FREE, FREE],
        });
        assert_eq!(step, Step::default());
        toggles.step(&sprint(PLAYING, PRESS, FREE));
        assert!(toggles.on(Kind::Sprint));
    }

    /// The mod switched off in Options > Mods acts as a death: both off, both let go.
    #[test]
    fn switching_the_mod_off_lets_go() {
        let mut toggles = Toggles::default();
        toggles.step(&sprint(PLAYING, PRESS, FREE));
        let step = toggles.step(&sprint(Mode::Stop, UP, HELD));
        assert!(!toggles.on(Kind::Sprint));
        assert_eq!(step.calls, calls(Some(false), None));
    }

    #[test]
    fn leaving_the_world_turns_both_off_with_nothing_to_let_go() {
        let mut toggles = Toggles::default();
        toggles.step(&Input {
            mode: PLAYING,
            keys: [PRESS, PRESS],
            vanilla: [FREE, FREE],
        });
        let step = toggles.step(&Input {
            mode: Mode::Gone,
            keys: [UP, UP],
            vanilla: [HELD, HELD],
        });
        assert!(!toggles.on(Kind::Sprint) && !toggles.on(Kind::Sneak));
        assert_eq!(step.calls, calls(None, None));
        // Joining a world again, both are off.
        let step = toggles.step(&Input {
            mode: PLAYING,
            keys: [UP, UP],
            vanilla: [FREE, FREE],
        });
        assert_eq!(step, Step::default());
    }

    /// With its toggle off the mod never calls the game's mapping: holding the sprint or sneak
    /// key works as the game made it.
    #[test]
    fn the_players_own_keys_are_left_alone() {
        let mut toggles = Toggles::default();
        for vanilla in [HELD, FREE, FLIP_DOWN, FLIP_UP] {
            for mode in [PLAYING, SCREEN, Mode::Stop, Mode::Gone] {
                let step = toggles.step(&sprint(mode, UP, vanilla));
                assert_eq!(step.calls, calls(None, None));
            }
        }
        assert_eq!(
            toggles.step(&sprint(PLAYING, UP, HELD)).held,
            [Some(Held::KeyHeld), None]
        );
        assert_eq!(
            toggles.step(&sprint(PLAYING, UP, FLIP_DOWN)).held,
            [Some(Held::Toggled), None]
        );
    }

    #[test]
    fn the_status_line_for_each_combination() {
        let still = Movement::default();
        let flying = Movement {
            flying: true,
            gliding: false,
        };
        let gliding = Movement {
            flying: false,
            gliding: true,
        };
        let toggled = Some(Held::Toggled);
        let key = Some(Held::KeyHeld);
        // [sprint, sneak]
        assert_eq!(status(still, [None, None]), None);
        assert_eq!(
            status(still, [toggled, None]),
            Some(Status::Sprinting(Held::Toggled))
        );
        assert_eq!(
            status(still, [key, None]),
            Some(Status::Sprinting(Held::KeyHeld))
        );
        assert_eq!(
            status(still, [None, toggled]),
            Some(Status::Sneaking(Held::Toggled))
        );
        assert_eq!(
            status(still, [None, key]),
            Some(Status::Sneaking(Held::KeyHeld))
        );
        // Sneaking stops a sprint, so it comes first.
        assert_eq!(
            status(still, [toggled, key]),
            Some(Status::Sneaking(Held::KeyHeld))
        );
        assert_eq!(
            status(still, [key, toggled]),
            Some(Status::Sneaking(Held::Toggled))
        );
        // Flying and gliding come before both.
        for held in [
            [None, None],
            [toggled, None],
            [None, key],
            [toggled, toggled],
        ] {
            assert_eq!(status(flying, held), Some(Status::Flying));
            assert_eq!(status(gliding, held), Some(Status::Gliding));
        }
        let both = Movement {
            flying: true,
            gliding: true,
        };
        assert_eq!(status(both, [None, None]), Some(Status::Flying));
    }

    #[test]
    fn the_lines_read_as_the_classic_hud() {
        assert_eq!(
            Status::Sprinting(Held::Toggled).text(),
            "[Sprinting (Toggled)]"
        );
        assert_eq!(
            Status::Sprinting(Held::KeyHeld).text(),
            "[Sprinting (Key Held)]"
        );
        assert_eq!(
            Status::Sneaking(Held::Toggled).text(),
            "[Sneaking (Toggled)]"
        );
        assert_eq!(
            Status::Sneaking(Held::KeyHeld).text(),
            "[Sneaking (Key Held)]"
        );
        assert_eq!(Status::Flying.text(), "[Flying]");
        assert_eq!(Status::Gliding.text(), "[Gliding]");
        assert_eq!(size(60.0), [64.0, 12.0]);
    }
}
