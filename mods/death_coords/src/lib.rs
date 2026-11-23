//! Death Coordinates: where you last died, for each world and each server. When the death screen
//! opens, the mod notes the block you died in, the dimension and the world's clock, keeps them in
//! its data folder (one small file for each single-player save or server address, so they last
//! across restarts) and puts a line in chat. `/lastdeath` says it again, with the in-game day and
//! how long ago it was.
use std::sync::{Mutex, MutexGuard};

use mod_api::{
    ClientHost, ClientTickPostEvent, Command, CommandEvent, ConfigPath, Flow, Hooks,
    RegisterClientCommandsEvent, Registrar, RegistrationContext, ScreenOpening, ScreenOpeningEvent,
    SettingSpec, Text,
};

/// The mod's id: its settings, data folder and translation keys.
pub const ID: &str = "death_coords";
/// The id screen hooks see for the death screen (`Screen::Dead` in the game's `Screen::id`).
pub const DEATH_SCREEN: &str = "dead";
/// The client command that says the last death again: `/lastdeath`.
pub const COMMAND: &str = "lastdeath";
/// The toggle that puts a line in chat at each death.
pub const ANNOUNCE: &str = "announce";
/// The first line of a death file: the format and its version.
pub const FILE_HEADER: &str = "death_coords 1";
/// The key when the game can't say which world or server this is.
pub const GLOBAL_KEY: &str = "global";
/// What `/lastdeath` says when no death is on record.
pub const NO_DEATH: &str = "No death on record for this world or server yet";
/// The world's clock: 20 ticks a second, 24000 a day.
pub const TICKS_PER_SECOND: i64 = 20;
pub const TICKS_PER_DAY: i64 = 24_000;
/// A death screen that opens this soon after you came into the world, at the very block of the
/// death on record, is that death shown again (you left while dead and came back), not a new
/// one: 5 seconds.
pub const REJOIN_TICKS: u32 = 100;
/// The longest key kept whole. A longer one keeps its first [`KEPT_OF_LONG_KEY`] characters and
/// ends with a hash of the whole key, so two long keys stay apart.
pub const MAX_KEY: usize = 100;
pub const KEPT_OF_LONG_KEY: usize = 80;

/// The setting on Death Coordinates' page in Options > Mods.
pub fn settings() -> [SettingSpec; 1] {
    [
        SettingSpec::toggle(ANNOUNCE, "death_coords.setting.announce", true)
            .tooltip("death_coords.setting.announce.tooltip"),
    ]
}

/// One death: where, and when on the world's clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Death {
    /// The block the player's feet were in.
    pub pos: [i32; 3],
    /// The dimension's id, as `minecraft:overworld`.
    pub dimension: String,
    /// The world's game time then (ticks since the world was made), when the game gave a clock.
    pub game_time: Option<i64>,
    /// The world's time of day then (24000 ticks a day, counting on), for the day number.
    pub day_time: Option<i64>,
}

/// The block a position is in: each coordinate rounded down, so -34.2 is block -35.
pub fn block_pos(pos: [f64; 3]) -> [i32; 3] {
    pos.map(|v| v.floor() as i32)
}

/// A dimension as the chat line names it: "the Overworld", "the Nether" and "the End" for the
/// three vanilla ones (an id without a namespace is Minecraft's), the id itself for any other.
pub fn dimension_name(id: &str) -> String {
    match id.strip_prefix("minecraft:").unwrap_or(id) {
        "overworld" => "the Overworld".to_owned(),
        "the_nether" => "the Nether".to_owned(),
        "the_end" => "the End".to_owned(),
        _ => id.to_owned(),
    }
}

/// The line in chat when you die: `You died at 120, 64, -35 in the Overworld`.
pub fn death_line(death: &Death) -> String {
    let [x, y, z] = death.pos;
    format!(
        "You died at {x}, {y}, {z} in {}",
        dimension_name(&death.dimension)
    )
}

/// The day a time of day falls on, counted as F3's "Day #" counts it (the first day is 0).
pub fn day_number(day_time: i64) -> i64 {
    day_time.div_euclid(TICKS_PER_DAY)
}

/// A span of world ticks, short: `45 s`, `12 min`, `3 h 5 min`. None for a negative span.
pub fn elapsed(ticks: i64) -> Option<String> {
    if ticks < 0 {
        return None;
    }
    let seconds = ticks / TICKS_PER_SECOND;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    Some(match (hours, minutes % 60) {
        (0, 0) => format!("{seconds} s"),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    })
}

/// What `/lastdeath` says: the death's line, then its day and how long ago when the clocks tell
/// (`now` is the world's game time now). A death later than now is from another world's clock,
/// so it gets no "ago".
pub fn last_death_line(death: Option<&Death>, now: Option<i64>) -> String {
    let Some(death) = death else {
        return NO_DEATH.to_owned();
    };
    let day = death.day_time.map(|t| format!("day {}", day_number(t)));
    let ago = death
        .game_time
        .zip(now)
        .and_then(|(then, now)| elapsed(now.checked_sub(then)?))
        .map(|span| format!("{span} ago"));
    let details: Vec<String> = day.into_iter().chain(ago).collect();
    if details.is_empty() {
        death_line(death)
    } else {
        format!("{} ({})", death_line(death), details.join(", "))
    }
}

/// A death as its file keeps it: the header, then `key=value` lines.
pub fn write_death(death: &Death) -> String {
    let [x, y, z] = death.pos;
    let mut text = format!(
        "{FILE_HEADER}\ndimension={}\npos={x},{y},{z}\n",
        death.dimension
    );
    if let Some(t) = death.game_time {
        text.push_str(&format!("game_time={t}\n"));
    }
    if let Some(t) = death.day_time {
        text.push_str(&format!("day_time={t}\n"));
    }
    text
}

/// A death file read back. None for a file of another format or version, or one without the
/// place (dimension and block); a time that doesn't read is left out, keys it doesn't know are
/// skipped, and Windows line ends and spaces around values are fine.
pub fn parse_death(text: &str) -> Option<Death> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if lines.next()? != FILE_HEADER {
        return None;
    }
    let mut pos = None;
    let mut dimension = None;
    let mut game_time: Option<i64> = None;
    let mut day_time: Option<i64> = None;
    for line in lines {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "dimension" if !value.is_empty() => dimension = Some(value.to_owned()),
            "pos" => pos = parse_pos(value),
            "game_time" => game_time = value.parse().ok(),
            "day_time" => day_time = value.parse().ok(),
            _ => {}
        }
    }
    Some(Death {
        pos: pos?,
        dimension: dimension?,
        game_time,
        day_time,
    })
}

/// `120,64,-35`: exactly three whole numbers.
fn parse_pos(value: &str) -> Option<[i32; 3]> {
    let mut parts = value.split(',').map(|part| part.trim().parse::<i32>());
    let pos = [
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    ];
    parts.next().is_none().then_some(pos)
}

/// The key the last death is kept under, safe as a file name on every system: the single-player
/// save's folder (`world-<folder>`, which a world's rename doesn't change), else the server's
/// address (`server-<address>`, lower case, without the default port 25565), else [`GLOBAL_KEY`].
pub fn world_key(save_folder: Option<&str>, address: Option<&str>) -> String {
    let save_folder = save_folder.filter(|folder| !folder.is_empty());
    let address = address.map(str::trim).filter(|address| !address.is_empty());
    let key = match (save_folder, address) {
        (Some(folder), _) => format!("world-{}", escape(folder)),
        (None, Some(address)) => {
            let lower = address.to_ascii_lowercase();
            let server = lower.strip_suffix(":25565").unwrap_or(&lower);
            format!("server-{}", escape(server))
        }
        (None, None) => return GLOBAL_KEY.to_owned(),
    };
    capped(key)
}

/// Every byte but ASCII letters, digits, `_`, `-` and `.` as `%XX`, so no key holds a separator,
/// a reserved character or a space, and two names never give the same key.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A key of at most [`MAX_KEY`] characters (keys are ASCII, so it cuts between characters).
fn capped(key: String) -> String {
    if key.len() <= MAX_KEY {
        return key;
    }
    format!(
        "{}-{:016x}",
        &key[..KEPT_OF_LONG_KEY],
        fnv1a(key.as_bytes())
    )
}

/// The 64-bit FNV-1a hash.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The file in the mod's data folder that keeps the last death for `key`.
pub fn death_path(key: &str) -> String {
    format!("deaths/{key}.txt")
}

/// Whether a command typed in chat (without its slash) is `/lastdeath`.
pub fn own_command(line: &str) -> bool {
    line.split_whitespace().next() == Some(COMMAND)
}

/// Whether a death screen opening `ticks_in_world` ticks after you came into the world, at
/// `death`, is the death on record (`previous`) shown again rather than a new one.
pub fn reopened(previous: &Death, death: &Death, ticks_in_world: u32) -> bool {
    ticks_in_world < REJOIN_TICKS
        && previous.pos == death.pos
        && previous.dimension == death.dimension
}

/// The world or server the player is in, as a key (`client_session().client_server()`).
fn key_of(host: &dyn ClientHost) -> String {
    let server = host.client_session().client_server();
    let server = server.as_ref();
    world_key(
        server.and_then(|s| s.save_folder.as_deref()),
        server.and_then(|s| s.address.as_deref()),
    )
}

/// The death on record for `key`, if its file is there and reads.
fn read_death(host: &dyn ClientHost, key: &str) -> Option<Death> {
    let path = ConfigPath::new(&death_path(key)).ok()?;
    let bytes = host.data_files().read(&path).ok()?;
    parse_death(std::str::from_utf8(&bytes).ok()?)
}

/// Keeps `death` as the one on record for `key`.
fn save_death(host: &dyn ClientHost, key: &str, death: &Death) {
    if let Ok(path) = ConfigPath::new(&death_path(key)) {
        // A failed write only loses the record across a restart; the chat line still comes.
        let _ = host
            .data_files()
            .write(&path, write_death(death).as_bytes());
    }
}

#[derive(Default)]
struct State {
    /// A death the chat hasn't been told of yet: `screen_opening` can't write to chat, the next
    /// client tick does.
    unannounced: Option<Death>,
    /// Client ticks since the local player came into the current world.
    ticks_in_world: u32,
}

#[derive(Default)]
struct DeathCoords {
    state: Mutex<State>,
}

impl DeathCoords {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[mod_api::horizon_mod]
impl Hooks for DeathCoords {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// The death screen is opening: note where, keep it, and leave the line for the next tick.
    /// Never stops a screen from opening.
    fn screen_opening(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenOpeningEvent<'_>,
    ) -> Option<ScreenOpening> {
        if event.new.id != DEATH_SCREEN || !host.on(ID) {
            return None;
        }
        // No player yet (a world loading with its player already dead): nothing to note.
        let Some(pos) = host
            .local_player()
            .and_then(|player| player.entity().position(host))
        else {
            return None;
        };
        let clock = host.clock();
        let death = Death {
            pos: block_pos(pos),
            dimension: host.dimension().to_owned(),
            game_time: clock.map(|c| c.game_time),
            day_time: clock.map(|c| c.day_time),
        };
        let key = key_of(host);
        let previous = read_death(host, &key);
        let ticks_in_world = self.state().ticks_in_world;
        let death = match previous {
            // Back on a server while still dead: the death on record keeps its time.
            Some(previous) if reopened(&previous, &death, ticks_in_world) => previous,
            _ => {
                save_death(host, &key, &death);
                death
            }
        };
        self.state().unannounced = Some(death);
        None
    }

    /// Counts the ticks in this world, and puts a new death's line in chat.
    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        let in_world = host.local_player().is_some();
        let unannounced = {
            let mut state = self.state();
            if in_world {
                state.ticks_in_world = state.ticks_in_world.saturating_add(1);
                state.unannounced.take()
            } else {
                state.ticks_in_world = 0;
                state.unannounced = None;
                None
            }
        };
        let Some(death) = unannounced else {
            return;
        };
        if host.on(ID) && host.setting_bool(ANNOUNCE).unwrap_or(true) {
            let _ = host.chat_line(Text::literal(death_line(&death)));
        }
    }

    /// `/lastdeath` in the chat box's completions, as the game's own commands are.
    fn register_client_commands(
        &self,
        host: &mut dyn ClientHost,
        event: &mut RegisterClientCommandsEvent<'_>,
    ) {
        if host.on(ID) {
            event.commands.push(Command::new(COMMAND));
        }
    }

    /// `/lastdeath`: the death on record for this world or server, or that there is none.
    fn command(&self, host: &mut dyn ClientHost, event: &mut CommandEvent<'_>) -> Flow {
        if !own_command(event.command) || !host.on(ID) {
            return Flow::Continue;
        }
        let line = {
            let view: &dyn ClientHost = &*host;
            let death = read_death(view, &key_of(view));
            last_death_line(death.as_ref(), view.clock().map(|c| c.game_time))
        };
        let _ = host.chat_line(Text::literal(line));
        Flow::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn death(
        pos: [i32; 3],
        dimension: &str,
        game_time: Option<i64>,
        day_time: Option<i64>,
    ) -> Death {
        Death {
            pos,
            dimension: dimension.to_owned(),
            game_time,
            day_time,
        }
    }

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
        assert_eq!(settings()[0].key, ANNOUNCE);
    }

    #[test]
    fn the_chat_line_says_where() {
        let overworld = death([120, 64, -35], "minecraft:overworld", Some(1), Some(1));
        assert_eq!(
            death_line(&overworld),
            "You died at 120, 64, -35 in the Overworld"
        );
        assert_eq!(
            death_line(&death([0, -60, 7], "minecraft:the_nether", None, None)),
            "You died at 0, -60, 7 in the Nether"
        );
        assert_eq!(
            death_line(&death([-1, 50, 1], "minecraft:the_end", None, None)),
            "You died at -1, 50, 1 in the End"
        );
        assert_eq!(
            death_line(&death([3, 70, 4], "mymod:mining", None, None)),
            "You died at 3, 70, 4 in mymod:mining"
        );
    }

    #[test]
    fn the_three_vanilla_dimensions_have_names() {
        assert_eq!(dimension_name("minecraft:overworld"), "the Overworld");
        assert_eq!(dimension_name("minecraft:the_nether"), "the Nether");
        assert_eq!(dimension_name("minecraft:the_end"), "the End");
        // An id without a namespace is Minecraft's.
        assert_eq!(dimension_name("overworld"), "the Overworld");
        // Another namespace keeps its id, even with a vanilla path.
        assert_eq!(dimension_name("mymod:the_end"), "mymod:the_end");
        assert_eq!(dimension_name("aether:the_aether"), "aether:the_aether");
        assert_eq!(dimension_name("minecraft:nether"), "minecraft:nether");
    }

    #[test]
    fn the_block_is_the_one_the_feet_are_in() {
        assert_eq!(block_pos([120.7, 64.0, -34.2]), [120, 64, -35]);
        assert_eq!(block_pos([-0.5, -64.9, 0.0]), [-1, -65, 0]);
        assert_eq!(
            block_pos([29_999_999.9, 319.99, -29_999_999.1]),
            [29_999_999, 319, -30_000_000]
        );
    }

    #[test]
    fn the_file_keeps_a_death() {
        let timed = death(
            [120, 64, -35],
            "minecraft:overworld",
            Some(123_456),
            Some(-30),
        );
        let text = write_death(&timed);
        assert_eq!(
            text,
            "death_coords 1\ndimension=minecraft:overworld\npos=120,64,-35\ngame_time=123456\nday_time=-30\n"
        );
        assert_eq!(parse_death(&text), Some(timed));
        let untimed = death([0, 0, 0], "mymod:mining", None, None);
        assert_eq!(
            write_death(&untimed),
            "death_coords 1\ndimension=mymod:mining\npos=0,0,0\n"
        );
        assert_eq!(parse_death(&write_death(&untimed)), Some(untimed));
    }

    #[test]
    fn the_file_reader_is_forgiving_but_not_credulous() {
        // Windows line ends, blank lines, spaces and keys of a later version are fine.
        let text = "death_coords 1\r\n\r\n pos = 1, 2 ,3 \r\ndimension=minecraft:the_end\r\ncause=lava\r\n";
        assert_eq!(
            parse_death(text),
            Some(death([1, 2, 3], "minecraft:the_end", None, None))
        );
        // A time that doesn't read is left out, not the death.
        assert_eq!(
            parse_death("death_coords 1\npos=1,2,3\ndimension=a:b\ngame_time=soon\nday_time=7\n"),
            Some(death([1, 2, 3], "a:b", None, Some(7)))
        );
        // Not this file, another version, or a death without its place.
        for bad in [
            "",
            "\n\n",
            "pos=1,2,3\ndimension=minecraft:overworld\n",
            "death_coords 2\npos=1,2,3\ndimension=minecraft:overworld\n",
            "death_coords 1\ndimension=minecraft:overworld\n",
            "death_coords 1\npos=1,2,3\n",
            "death_coords 1\npos=1,2,3\ndimension=\n",
            "death_coords 1\npos=1,2\ndimension=minecraft:overworld\n",
            "death_coords 1\npos=1,2,3,4\ndimension=minecraft:overworld\n",
            "death_coords 1\npos=1,x,3\ndimension=minecraft:overworld\n",
            "death_coords 1\npos=1,2,9999999999\ndimension=minecraft:overworld\n",
        ] {
            assert_eq!(parse_death(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_world_is_known_by_its_save_folder_and_a_server_by_its_address() {
        assert_eq!(world_key(Some("New World"), None), "world-New%20World");
        // The save folder wins over any address.
        assert_eq!(
            world_key(Some("New World"), Some("192.168.1.5:51234")),
            "world-New%20World"
        );
        assert_eq!(
            world_key(None, Some("Play.Example.com")),
            "server-play.example.com"
        );
        // The default port is the same server as no port; another port is another server.
        assert_eq!(
            world_key(None, Some("play.example.com:25565")),
            "server-play.example.com"
        );
        assert_eq!(
            world_key(None, Some("play.example.com:25566")),
            "server-play.example.com%3A25566"
        );
        assert_eq!(world_key(None, None), GLOBAL_KEY);
        assert_eq!(world_key(Some(""), Some("  ")), GLOBAL_KEY);
        assert_eq!(world_key(Some("Мир"), None), "world-%D0%9C%D0%B8%D1%80");
        // `%` itself is escaped, so no two names share a key.
        assert_eq!(world_key(Some("100%"), None), "world-100%25");
        assert_ne!(world_key(Some("a b"), None), world_key(Some("a%20b"), None));
        assert_eq!(death_path("world-x"), "deaths/world-x.txt");
    }

    #[test]
    fn every_key_is_a_file_name_the_game_takes() {
        let long = "x".repeat(300);
        for (folder, address) in [
            (Some("CON"), None),
            (Some("nul.txt"), None),
            (Some("ends with a dot."), None),
            (Some("ends with a space "), None),
            (Some(".."), None),
            (Some("a/b\\c:d*e?f\"g<h>i|j"), None),
            (Some("tab\there"), None),
            (Some(long.as_str()), None),
            (None, Some("Friend's world (ab-****)")),
            (None, Some("[2001:db8::1]:25565")),
            (None, Some("COM1")),
            (None, None),
        ] {
            let key = world_key(folder, address);
            assert!(key.len() <= MAX_KEY, "{key}");
            assert!(ConfigPath::new(&death_path(&key)).is_ok(), "{key}");
        }
    }

    #[test]
    fn a_long_key_is_cut_but_stays_its_own() {
        let a = world_key(Some("a".repeat(200).as_str()), None);
        let b = world_key(Some(format!("{}b", "a".repeat(199)).as_str()), None);
        assert_eq!(a.len(), KEPT_OF_LONG_KEY + 17);
        assert!(a.starts_with("world-aaaa"), "{a}");
        assert_ne!(a, b);
        // FNV-1a's published 64-bit test vectors.
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn lastdeath_adds_the_day_and_how_long_ago() {
        let timed = death(
            [120, 64, -35],
            "minecraft:overworld",
            Some(1_000),
            Some(30_000),
        );
        assert_eq!(
            last_death_line(Some(&timed), Some(1_000 + 20 * 60 * 12)),
            "You died at 120, 64, -35 in the Overworld (day 1, 12 min ago)"
        );
        // No clock now, or a clock from before the death (another world's): no "ago".
        assert_eq!(
            last_death_line(Some(&timed), None),
            "You died at 120, 64, -35 in the Overworld (day 1)"
        );
        assert_eq!(
            last_death_line(Some(&timed), Some(999)),
            "You died at 120, 64, -35 in the Overworld (day 1)"
        );
        let untimed = death([1, 2, 3], "minecraft:the_nether", None, None);
        assert_eq!(
            last_death_line(Some(&untimed), Some(5_000)),
            "You died at 1, 2, 3 in the Nether"
        );
        let no_day = death([1, 2, 3], "minecraft:the_end", Some(0), None);
        assert_eq!(
            last_death_line(Some(&no_day), Some(900)),
            "You died at 1, 2, 3 in the End (45 s ago)"
        );
        assert_eq!(last_death_line(None, Some(5)), NO_DEATH);
    }

    #[test]
    fn how_long_ago_in_seconds_minutes_and_hours() {
        assert_eq!(elapsed(-1), None);
        assert_eq!(elapsed(0).as_deref(), Some("0 s"));
        assert_eq!(elapsed(19).as_deref(), Some("0 s"));
        assert_eq!(elapsed(20 * 59).as_deref(), Some("59 s"));
        assert_eq!(elapsed(20 * 60).as_deref(), Some("1 min"));
        assert_eq!(elapsed(20 * 3599).as_deref(), Some("59 min"));
        assert_eq!(elapsed(20 * 3600).as_deref(), Some("1 h"));
        assert_eq!(
            elapsed(20 * (3 * 3600 + 5 * 60 + 30)).as_deref(),
            Some("3 h 5 min")
        );
        // Days as F3 counts them, from 0; a time set below zero is a day before the first.
        assert_eq!(day_number(0), 0);
        assert_eq!(day_number(23_999), 0);
        assert_eq!(day_number(24_000), 1);
        assert_eq!(day_number(-1), -1);
    }

    #[test]
    fn only_its_own_command_is_taken() {
        assert!(own_command("lastdeath"));
        assert!(own_command("lastdeath please"));
        assert!(own_command("  lastdeath"));
        assert!(!own_command("lastdeaths"));
        assert!(!own_command("last death"));
        assert!(!own_command("tp 0 0 0"));
        assert!(!own_command(""));
    }

    #[test]
    fn a_death_shown_again_on_rejoining_is_not_a_new_one() {
        let earlier = death([5, 60, 5], "minecraft:overworld", Some(100), Some(100));
        let again = death(
            [5, 60, 5],
            "minecraft:overworld",
            Some(90_000),
            Some(90_000),
        );
        assert!(reopened(&earlier, &again, 0));
        assert!(reopened(&earlier, &again, REJOIN_TICKS - 1));
        // Long after coming in, the same block is a new death there.
        assert!(!reopened(&earlier, &again, REJOIN_TICKS));
        let next_block = death([5, 60, 6], "minecraft:overworld", None, None);
        assert!(!reopened(&earlier, &next_block, 0));
        let other_dimension = death([5, 60, 5], "minecraft:the_nether", None, None);
        assert!(!reopened(&earlier, &other_dimension, 0));
    }
}
