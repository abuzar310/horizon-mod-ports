//! Speedrun Clock: a real-time (RTA) speedrun timer in a corner of the screen, `MM:SS.mmm`, and
//! `HH:MM:SS.mmm` once an hour has passed. It starts on the player's first move after a world
//! loads, keeps counting through deaths, and stops when the end credits open. Leaving the world
//! resets it for the next run. Finished runs and the personal best are kept in the mod's data
//! folder; a finished run that beats the best shows in green. `/speedrun` starts, stops and resets
//! it by hand and tells the best.
use std::sync::{Mutex, MutexGuard};

use mod_api::{
    ClientHost, ClientPlayerLoggingInEvent, ClientPlayerLoggingOutEvent, ClientTickPostEvent,
    Command, CommandEvent, ConfigFileError, ConfigPath, Corner, Flow, Hooks,
    RegisterClientCommandsEvent, Registrar, RegistrationContext, RenderGuiPostEvent, ScreenOpening,
    ScreenOpeningEvent, SettingSpec, Text,
};

/// The mod's id: its settings folder, data folder and translation keys.
pub const ID: &str = "speedrun_clock";
/// The WebAssembly mods' panel, as Coordinates HUD draws it: translucent black, white text.
pub const BACKGROUND: u32 = 0x7800_0000;
pub const INK: u32 = 0xFFFF_FFFF;
/// The timer while it waits for the first move, and the best's line: grey.
pub const GREY: u32 = 0xFFAA_AAAA;
/// A finished run that beat the personal best: the game's green.
pub const RECORD: u32 = 0xFF55_FF55;
/// A line's height and the text's inset, in GUI pixels.
pub const LINE: f32 = 10.0;
pub const PADDING: f32 = 2.0;
/// The corners in the order the corner setting lists them.
pub const CORNERS: [Corner; 4] = Corner::ALL;
/// The corner setting's default: top right.
pub const DEFAULT_CORNER: usize = 1;

/// The id the game gives the end poem and credits screen (`Screen::Win => "win"` in
/// horizon-client's `game.rs`), opened when the player leaves the End.
pub const CREDITS_SCREEN: &str = "win";
/// Options > Credits & Attribution opens the same screen over the options screen
/// (horizon-client's `game/interface/options.rs`): that is no finish.
pub const OPTIONS_SCREEN: &str = "options";

/// A move this far from where the player appeared, in blocks, across the ground or upwards,
/// starts the run. Settling down onto the ground does not.
pub const START_MOVE: f64 = 0.01;
/// Farther than this from where the player appeared, in one tick, is the game placing the player
/// (the spawn point arriving, a teleport), not a first step: the timer waits from there instead.
/// A first tick of walking, sprinting or a sprint-jump from standing covers well under half a
/// block, and a jump rises 0.42.
pub const SNAP: f64 = 1.0;

/// The personal best in the mod's data folder (`horizon-mod-data/speedrun_clock/`): one line.
pub const BEST_FILE: &str = "best.txt";
/// Every finished run, one line each, oldest first.
pub const RUNS_FILE: &str = "runs.txt";

/// The client command's usages, each completed in the chat box as the game's own.
pub const USAGES: [&str; 5] = [
    "speedrun",
    "speedrun start",
    "speedrun stop",
    "speedrun reset",
    "speedrun best",
];

/// The settings on Speedrun Clock's page in Options > Mods.
pub fn settings() -> [SettingSpec; 3] {
    [
        SettingSpec::toggle("shown", "speedrun_clock.setting.shown", true)
            .tooltip("speedrun_clock.setting.shown.tooltip"),
        SettingSpec::choice(
            "corner",
            "speedrun_clock.setting.corner",
            CORNERS.map(|c| format!("speedrun_clock.corner.{}", c.name())),
            DEFAULT_CORNER,
        )
        .tooltip("speedrun_clock.setting.corner.tooltip"),
        SettingSpec::toggle("show_best", "speedrun_clock.setting.show_best", true)
            .tooltip("speedrun_clock.setting.show_best.tooltip"),
    ]
}

/// `ms` milliseconds as the timer shows them: `MM:SS.mmm`, with the hours in front
/// (`HH:MM:SS.mmm`) once an hour has passed. Milliseconds are cut, never rounded up, so a time
/// never shows the next second, minute or hour before it is reached.
pub fn format_time(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = ms / 60_000 % 60;
    let seconds = ms / 1000 % 60;
    let millis = ms % 1000;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("{minutes:02}:{seconds:02}.{millis:03}")
    }
}

/// The milliseconds from `since` to `now` on the game's clock; zero if the clock reads earlier.
pub fn elapsed(since: u64, now: u64) -> u64 {
    now.saturating_sub(since)
}

/// Whether a finished run of `time` beats the personal best `best`. The first run ever beats
/// nothing and so sets the best; a tie is not a new best.
pub fn beats(time: u64, best: Option<u64>) -> bool {
    best.is_none_or(|best| time < best)
}

/// How the player moved since they appeared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// Not at all, or only settling down onto the ground.
    Still,
    /// A first step, a jump: the run starts.
    Moved,
    /// Put somewhere else by the game, too far for one step: wait again from there.
    Snapped,
}

/// How the player at `now` moved from `spawn`, where they appeared.
pub fn movement(spawn: [f64; 3], now: [f64; 3]) -> Move {
    let across = (now[0] - spawn[0]).hypot(now[2] - spawn[2]);
    let up = now[1] - spawn[1];
    if across > SNAP || up.abs() > SNAP {
        Move::Snapped
    } else if across > START_MOVE || up > START_MOVE {
        Move::Moved
    } else {
        Move::Still
    }
}

/// How a stopped run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// `/speedrun stop`: not a finish, so it is neither kept nor compared with the best.
    ByHand,
    /// The end credits opened; `record` when it beat the best kept before it.
    Finished { record: bool },
}

/// The run, as a state machine. Times are the game's clock (`ClientHost::millis`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Run {
    /// No world, or just reset: waiting to see where the player is.
    #[default]
    Idle,
    /// In a world, where the player appeared: their first move starts the run.
    Armed { spawn: [f64; 3] },
    /// Counting since `since`, through deaths and changes of dimension.
    Running { since: u64 },
    /// Stopped at `time` milliseconds, until a reset or a new start.
    Stopped { time: u64, end: End },
}

impl Run {
    /// One client tick: where the local player is at `now`, None outside a world. An idle timer
    /// learns where the player appeared; an armed one starts on their first move.
    pub fn observe(&mut self, now: u64, position: Option<[f64; 3]>) {
        match (*self, position) {
            (Run::Idle, Some(pos)) => *self = Run::Armed { spawn: pos },
            (Run::Armed { .. }, None) => *self = Run::Idle,
            (Run::Armed { spawn }, Some(pos)) => match movement(spawn, pos) {
                Move::Still => {}
                Move::Moved => *self = Run::Running { since: now },
                Move::Snapped => *self = Run::Armed { spawn: pos },
            },
            _ => {}
        }
    }

    /// `/speedrun start`: a new run from zero at `now`. False, changing nothing, while one runs.
    pub fn start(&mut self, now: u64) -> bool {
        if self.running() {
            return false;
        }
        *self = Run::Running { since: now };
        true
    }

    /// `/speedrun stop`: the time it stopped at, None when it wasn't running.
    pub fn stop(&mut self, now: u64) -> Option<u64> {
        let Run::Running { since } = *self else {
            return None;
        };
        let time = elapsed(since, now);
        *self = Run::Stopped {
            time,
            end: End::ByHand,
        };
        Some(time)
    }

    /// The end credits opened at `now`: the run's time, and whether it beat `best`, the
    /// personal best before it. None, changing nothing, when no run was counting.
    pub fn finish(&mut self, now: u64, best: Option<u64>) -> Option<(u64, bool)> {
        let Run::Running { since } = *self else {
            return None;
        };
        let time = elapsed(since, now);
        let record = beats(time, best);
        *self = Run::Stopped {
            time,
            end: End::Finished { record },
        };
        Some((time, record))
    }

    /// Leaving the world, or `/speedrun reset`: ready for the next run.
    pub fn reset(&mut self) {
        *self = Run::Idle;
    }

    /// Whether the timer is counting.
    pub fn running(&self) -> bool {
        matches!(self, Run::Running { .. })
    }

    /// Whether the run is waiting for the first move.
    pub fn waiting(&self) -> bool {
        matches!(self, Run::Idle | Run::Armed { .. })
    }

    /// Whether the run finished and beat the best: the timer shows green.
    pub fn record(&self) -> bool {
        matches!(
            self,
            Run::Stopped {
                end: End::Finished { record: true },
                ..
            }
        )
    }

    /// The time the timer shows at `now`, in milliseconds.
    pub fn time(&self, now: u64) -> u64 {
        match *self {
            Run::Idle | Run::Armed { .. } => 0,
            Run::Running { since } => elapsed(since, now),
            Run::Stopped { time, .. } => time,
        }
    }
}

/// Whether a change of screen from `current` to `new` (their ids) is the end credits of a run:
/// the credits screen, not opened from Options.
pub fn ends_run(current: &str, new: &str) -> bool {
    new == CREDITS_SCREEN && current != OPTIONS_SCREEN
}

/// A time as the data files keep it: the milliseconds, then the time as the timer shows it, for
/// a person reading the file: `754321 12:34.321`.
pub fn time_line(ms: u64) -> String {
    format!("{ms} {}\n", format_time(ms))
}

/// The milliseconds a data file's line starts with. None for a blank line, one that starts with
/// anything else, and zero (no run takes no time).
pub fn parse_time(line: &str) -> Option<u64> {
    let first = line.split_whitespace().next()?;
    first.parse::<u64>().ok().filter(|ms| *ms > 0)
}

/// The best in `best.txt`'s text: its first line with a time.
pub fn parse_best(text: &str) -> Option<u64> {
    text.lines().find_map(parse_time)
}

/// The runs in `runs.txt`'s text, oldest first; lines without a time are skipped.
pub fn parse_runs(text: &str) -> Vec<u64> {
    text.lines().filter_map(parse_time).collect()
}

/// What reading `best.txt` says the best is: None when there is no file yet. An error the file
/// system gave stays an error, so the best is never written over a file that couldn't be read.
pub fn stored_best(read: Result<Vec<u8>, ConfigFileError>) -> Result<Option<u64>, ConfigFileError> {
    match read {
        Ok(bytes) => Ok(parse_best(&String::from_utf8_lossy(&bytes))),
        Err(ConfigFileError::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

/// `runs.txt` with a run of `ms` added at the end, from what reading it gave. None when it
/// couldn't be read, so the runs kept are never written over.
pub fn with_run(read: Result<Vec<u8>, ConfigFileError>, ms: u64) -> Option<String> {
    let mut text = match read {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(ConfigFileError::NotFound) => String::new(),
        Err(_) => return None,
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&time_line(ms));
    Some(text)
}

/// What `/speedrun` was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// `/speedrun`: the time and the best.
    Status,
    Start,
    Stop,
    Reset,
    Best,
    /// Anything else after `/speedrun`: how to use it.
    Help,
}

/// The action a client command (without its slash) asks for; None when it isn't `/speedrun`.
pub fn parse_command(command: &str) -> Option<Action> {
    let mut words = command.split_whitespace();
    if words.next()? != "speedrun" {
        return None;
    }
    let action = match words.next() {
        None => Action::Status,
        Some("start") => Action::Start,
        Some("stop") => Action::Stop,
        Some("reset") => Action::Reset,
        Some("best") => Action::Best,
        Some(_) => Action::Help,
    };
    Some(if words.next().is_some() {
        Action::Help
    } else {
        action
    })
}

/// The personal best, as chat tells it.
pub fn best_line(best: Option<u64>) -> String {
    match best {
        Some(ms) => format!("Personal best: {}", format_time(ms)),
        None => "No personal best yet: finish a run to set one".to_owned(),
    }
}

/// `/speedrun best`'s line: the best, and how many finished runs are kept (None when the runs
/// file couldn't be read).
pub fn best_message(best: Option<u64>, runs: Option<usize>) -> String {
    match runs {
        Some(1) => format!("{}. 1 finished run kept.", best_line(best)),
        Some(n) => format!("{}. {n} finished runs kept.", best_line(best)),
        None => format!("{}.", best_line(best)),
    }
}

/// The chat line once a run finished at `time`, `best` being the best before it.
pub fn finish_line(time: u64, record: bool, best: Option<u64>) -> String {
    match (record, best) {
        (true, Some(old)) => format!(
            "Run finished: {}, a new personal best (was {}).",
            format_time(time),
            format_time(old)
        ),
        (true, None) => format!(
            "Run finished: {}, your first personal best.",
            format_time(time)
        ),
        (false, _) => format!("Run finished: {}. {}.", format_time(time), best_line(best)),
    }
}

/// Carries out `/speedrun`, `start`, `stop`, `reset` and help on `run` at `now`, `best` being the
/// personal best; returns the chat line. `best` has its own line, [`best_message`].
pub fn act(run: &mut Run, best: Option<u64>, action: Action, now: u64) -> String {
    match action {
        Action::Status => {
            let state = match *run {
                Run::Idle | Run::Armed { .. } => "waiting for your first move",
                Run::Running { .. } => "running",
                Run::Stopped {
                    end: End::ByHand, ..
                } => "stopped",
                Run::Stopped {
                    end: End::Finished { .. },
                    ..
                } => "finished",
            };
            format!(
                "Speedrun: {} ({state}). {}.",
                format_time(run.time(now)),
                best_line(best)
            )
        }
        Action::Start => {
            if run.start(now) {
                "Speedrun timer started.".to_owned()
            } else {
                "The speedrun timer is already running.".to_owned()
            }
        }
        Action::Stop => match run.stop(now) {
            Some(time) => format!(
                "Speedrun timer stopped at {}. A run stopped by hand is not kept.",
                format_time(time)
            ),
            None => "The speedrun timer isn't running.".to_owned(),
        },
        Action::Reset => {
            run.reset();
            "Speedrun timer reset: it starts on your next move.".to_owned()
        }
        Action::Best => best_line(best),
        Action::Help => "Usage: /speedrun [start|stop|reset|best]".to_owned(),
    }
}

/// The panel's lines and their colours at `now`: the time (grey while waiting for the first
/// move, green for a finished run that beat the best), then the best when `show_best`.
pub fn panel(run: &Run, now: u64, best: Option<u64>, show_best: bool) -> Vec<(String, u32)> {
    let colour = if run.waiting() {
        GREY
    } else if run.record() {
        RECORD
    } else {
        INK
    };
    let mut lines = vec![(format_time(run.time(now)), colour)];
    if show_best {
        let text = match best {
            Some(ms) => format!("Best {}", format_time(ms)),
            None => "Best --:--.---".to_owned(),
        };
        lines.push((text, GREY));
    }
    lines
}

/// The panel around `lines` lines whose widest is `width` GUI pixels: `[w, h]`.
pub fn size(width: f32, lines: usize) -> [f32; 2] {
    [width + 2.0 * PADDING, LINE * lines as f32 + PADDING]
}

/// One of the mod's data files, as it reads.
fn read(host: &dyn ClientHost, name: &str) -> Result<Vec<u8>, ConfigFileError> {
    host.data_files().read(&ConfigPath::new(name)?)
}

/// Replaces one of the mod's data files.
fn write(host: &dyn ClientHost, name: &str, text: &str) -> Result<(), ConfigFileError> {
    host.data_files()
        .write(&ConfigPath::new(name)?, text.as_bytes())
}

#[derive(Default)]
struct State {
    run: Run,
    /// The personal best, read from `best.txt` once a world is open.
    best: Option<u64>,
    best_read: bool,
    /// A line for chat on the next tick: the credits' hook can't write to chat.
    news: Option<String>,
}

#[derive(Default)]
struct SpeedrunTimer {
    // Never held across a host call: a host call can fire another of this mod's hooks.
    state: Mutex<State>,
}

impl SpeedrunTimer {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Leaving a world, or joining one: ready for the next run.
    fn reset(&self) {
        let mut state = self.state();
        state.run.reset();
        state.news = None;
    }
}

#[mod_api::horizon_mod]
impl Hooks for SpeedrunTimer {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// `/speedrun` and its words in the chat box's completions, as the game's own commands.
    fn register_client_commands(
        &self,
        _: &mut dyn ClientHost,
        event: &mut RegisterClientCommandsEvent<'_>,
    ) {
        event.commands.extend(USAGES.into_iter().map(Command::new));
    }

    /// `/speedrun`: the time, or start, stop, reset, or the best; the answer goes to chat.
    fn command(&self, host: &mut dyn ClientHost, event: &mut CommandEvent<'_>) -> Flow {
        let Some(action) = parse_command(event.command) else {
            return Flow::Continue;
        };
        if !host.on(ID) {
            return Flow::Continue;
        }
        let now = host.millis();
        let line = if action == Action::Best {
            // From the files, so an edit or a deletion since the world opened shows.
            let view: &dyn ClientHost = &*host;
            let stored = stored_best(read(view, BEST_FILE));
            let runs = match read(view, RUNS_FILE) {
                Ok(bytes) => Some(parse_runs(&String::from_utf8_lossy(&bytes)).len()),
                Err(ConfigFileError::NotFound) => Some(0),
                Err(_) => None,
            };
            let mut state = self.state();
            if let Ok(best) = stored {
                state.best = best;
            }
            best_message(state.best, runs)
        } else {
            let mut state = self.state();
            let best = state.best;
            act(&mut state.run, best, action, now)
        };
        let _ = host.chat_line(Text::literal(line));
        Flow::Cancel
    }

    /// Each tick in a world: the best is read once, the run sees where the player is, and the
    /// line about a finished run goes to chat.
    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        if !host.on(ID) {
            return;
        }
        let news = self.state().news.take();
        if let Some(line) = news {
            let _ = host.chat_line(Text::literal(line));
        }
        let view: &dyn ClientHost = &*host;
        let position = view
            .local_player()
            .and_then(|player| player.entity().position(view));
        if position.is_some() && !self.state().best_read {
            let stored = stored_best(read(view, BEST_FILE));
            let mut state = self.state();
            state.best_read = true;
            if let Ok(best) = stored {
                state.best = best;
            }
        }
        let now = view.millis();
        self.state().run.observe(now, position);
    }

    /// The end credits opening stop the run; it is kept, and it becomes the best when it beat it.
    /// The screen always opens: this only listens.
    fn screen_opening(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenOpeningEvent<'_>,
    ) -> Option<ScreenOpening> {
        if !host.on(ID) || !ends_run(event.current.id, event.new.id) {
            return None;
        }
        let now = host.millis();
        if !self.state().run.running() {
            return None;
        }
        // The best from its file, so an edit or a deletion since the world opened counts.
        let stored = stored_best(read(host, BEST_FILE));
        let best = match &stored {
            Ok(best) => *best,
            Err(_) => self.state().best,
        };
        let finished = self.state().run.finish(now, best);
        let (time, record) = finished?;
        if record && stored.is_ok() {
            let _ = write(host, BEST_FILE, &time_line(time));
        }
        if let Some(runs) = with_run(read(host, RUNS_FILE), time) {
            let _ = write(host, RUNS_FILE, &runs);
        }
        let mut state = self.state();
        state.best = if record { Some(time) } else { best };
        state.news = Some(finish_line(time, record, best));
        None
    }

    /// Joining a world: a run left over from the last one, if leaving it went unheard, is gone.
    fn client_player_logging_in(&self, _: &mut dyn ClientHost, _: &mut ClientPlayerLoggingInEvent) {
        // Not gated on `host.on(ID)`: forgetting a run is no action in the game.
        self.reset();
    }

    /// Leaving the world resets the timer, ready for the next run.
    fn client_player_logging_out(
        &self,
        _: &mut dyn ClientHost,
        _: &mut ClientPlayerLoggingOutEvent,
    ) {
        // Not gated on `host.on(ID)`: a run turned off mid-way must not carry into the next world.
        self.reset();
    }

    /// The panel in the corner picked, below what the corner holds already; none while F3's
    /// debug screen takes that corner.
    fn render_gui_post(&self, host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
        if !host.on(ID) || !host.setting_bool("shown").unwrap_or(true) {
            return;
        }
        let corner = host
            .setting_choice("corner")
            .map_or(DEFAULT_CORNER, |i| i as usize)
            .min(CORNERS.len() - 1);
        let corner = CORNERS[corner];
        if !host.native_corner_shown(corner) || host.local_player().is_none() {
            return;
        }
        let show_best = host.setting_bool("show_best").unwrap_or(true);
        let now = host.millis();
        let lines = {
            let state = self.state();
            panel(&state.run, now, state.best, show_best)
        };
        let draw = &mut *event.draw;
        let widest = lines
            .iter()
            .map(|(text, _)| draw.text_width(text))
            .fold(0.0, f32::max);
        let [w, h] = size(widest, lines.len());
        let (x, y) = event.corners.place(corner, w, h);
        draw.fill([x, y, w, h], BACKGROUND);
        for (i, (text, colour)) in lines.iter().enumerate() {
            draw.text(
                text,
                [x + PADDING, y + PADDING + LINE * i as f32],
                *colour,
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPAWN: [f64; 3] = [8.5, 64.0, -3.5];

    /// A run that started at 1000 ms on the game's clock.
    fn running() -> Run {
        let mut run = Run::default();
        run.observe(900, Some(SPAWN));
        run.observe(1000, Some([8.6, 64.0, -3.5]));
        assert_eq!(run, Run::Running { since: 1000 });
        run
    }

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
        assert_eq!(CORNERS[DEFAULT_CORNER], Corner::TopRight);
    }

    #[test]
    fn the_time_starts_at_zero() {
        assert_eq!(format_time(0), "00:00.000");
        assert_eq!(format_time(7), "00:00.007");
        assert_eq!(format_time(1_234), "00:01.234");
    }

    #[test]
    fn minutes_and_seconds_under_an_hour() {
        assert_eq!(format_time(59_999), "00:59.999");
        assert_eq!(format_time(60_000), "01:00.000");
        assert_eq!(format_time(754_321), "12:34.321");
        assert_eq!(format_time(3_599_999), "59:59.999");
    }

    #[test]
    fn the_hours_show_once_an_hour_has_passed() {
        assert_eq!(format_time(3_600_000), "01:00:00.000");
        assert_eq!(format_time(3_600_001), "01:00:00.001");
        assert_eq!(format_time(5_025_678), "01:23:45.678");
        assert_eq!(format_time(36_000_000), "10:00:00.000");
        assert_eq!(format_time(360_000_000), "100:00:00.000");
    }

    #[test]
    fn milliseconds_are_cut_never_rounded_up() {
        // A time one millisecond short of the next second, minute or hour still shows the one
        // before: the timer never claims a time not yet reached.
        assert_eq!(format_time(999), "00:00.999");
        assert_eq!(format_time(1_999), "00:01.999");
        assert_eq!(format_time(59_999), "00:59.999");
        assert_eq!(format_time(3_599_999), "59:59.999");
        assert_eq!(format_time(7_199_999), "01:59:59.999");
    }

    #[test]
    fn a_clock_reading_earlier_gives_zero_not_a_wrap() {
        assert_eq!(elapsed(1000, 4500), 3500);
        assert_eq!(elapsed(4500, 1000), 0);
        let run = running();
        assert_eq!(run.time(500), 0);
    }

    #[test]
    fn idle_until_the_player_is_in_a_world() {
        let mut run = Run::default();
        assert_eq!(run, Run::Idle);
        run.observe(0, None);
        assert_eq!(run, Run::Idle);
        run.observe(50, Some(SPAWN));
        assert_eq!(run, Run::Armed { spawn: SPAWN });
        assert!(run.waiting());
        assert_eq!(run.time(10_000), 0);
    }

    #[test]
    fn standing_still_or_settling_down_does_not_start_it() {
        let mut run = Run::default();
        run.observe(0, Some(SPAWN));
        run.observe(50, Some(SPAWN));
        run.observe(100, Some([8.5, 63.8, -3.5]));
        run.observe(150, Some([8.505, 63.8, -3.495]));
        assert_eq!(run, Run::Armed { spawn: SPAWN });
    }

    #[test]
    fn the_first_step_starts_it_at_that_tick() {
        let mut run = Run::default();
        run.observe(0, Some(SPAWN));
        run.observe(50, Some(SPAWN));
        run.observe(100, Some([8.5, 64.0, -3.6]));
        assert_eq!(run, Run::Running { since: 100 });
        assert_eq!(run.time(1_100), 1_000);
    }

    #[test]
    fn a_jump_in_place_starts_it() {
        let mut run = Run::default();
        run.observe(0, Some(SPAWN));
        run.observe(50, Some([8.5, 64.42, -3.5]));
        assert_eq!(run, Run::Running { since: 50 });
    }

    #[test]
    fn being_placed_by_the_game_waits_again_from_there() {
        let mut run = Run::default();
        // Placed at the origin before the spawn point arrived, then put at spawn.
        run.observe(0, Some([0.0, 0.0, 0.0]));
        run.observe(50, Some(SPAWN));
        assert_eq!(run, Run::Armed { spawn: SPAWN });
        run.observe(100, Some([8.6, 64.0, -3.5]));
        assert_eq!(run, Run::Running { since: 100 });
        // A correction of two blocks is the game too, not a step.
        let mut run = Run::default();
        run.observe(0, Some(SPAWN));
        run.observe(50, Some([10.5, 64.0, -3.5]));
        assert_eq!(
            run,
            Run::Armed {
                spawn: [10.5, 64.0, -3.5]
            }
        );
        // Spawned above the ground: the fall waits, the landing waits, the step starts it.
        let mut run = Run::default();
        run.observe(0, Some([8.5, 70.0, -3.5]));
        for (tick, y) in [69.9, 69.5, 68.8, 67.6, 66.1, 64.4, 64.0, 64.0]
            .into_iter()
            .enumerate()
        {
            run.observe(50 * (tick as u64 + 1), Some([8.5, y, -3.5]));
            assert!(run.waiting(), "falling to {y}");
        }
        run.observe(500, Some([8.62, 64.0, -3.5]));
        assert_eq!(run, Run::Running { since: 500 });
    }

    #[test]
    fn a_sprint_jump_from_standing_starts_it() {
        let mut run = Run::default();
        run.observe(0, Some(SPAWN));
        // Sprint-jumping's first tick: about a third of a block across, 0.42 up.
        run.observe(50, Some([8.83, 64.42, -3.5]));
        assert_eq!(run, Run::Running { since: 50 });
    }

    #[test]
    fn a_lost_player_before_the_first_move_waits_again() {
        let mut run = Run::default();
        run.observe(0, Some(SPAWN));
        run.observe(50, None);
        assert_eq!(run, Run::Idle);
        run.observe(100, Some([100.0, 70.0, 100.0]));
        assert_eq!(
            run,
            Run::Armed {
                spawn: [100.0, 70.0, 100.0]
            }
        );
    }

    #[test]
    fn it_keeps_counting_through_deaths_and_dimensions() {
        let mut run = running();
        // Dead (no position for a while), respawned far away, then through a portal.
        run.observe(5_000, None);
        run.observe(6_000, Some([0.5, 70.0, 0.5]));
        run.observe(7_000, Some([10_000.0, 40.0, -200.0]));
        assert_eq!(run, Run::Running { since: 1000 });
        assert_eq!(run.time(61_000), 60_000);
    }

    #[test]
    fn the_credits_stop_it_and_tell_whether_it_is_a_best() {
        let mut run = running();
        assert_eq!(run.finish(601_000, Some(700_000)), Some((600_000, true)));
        assert_eq!(
            run,
            Run::Stopped {
                time: 600_000,
                end: End::Finished { record: true }
            }
        );
        assert!(run.record());
        // Stopped: the time stays, ticks change nothing, and a second credits screen neither.
        assert_eq!(run.time(9_999_999), 600_000);
        run.observe(700_000, Some(SPAWN));
        assert_eq!(run.finish(800_000, None), None);
        assert_eq!(run.time(9_999_999), 600_000);
    }

    #[test]
    fn a_slower_run_or_a_tie_is_no_best() {
        let mut run = running();
        assert_eq!(run.finish(701_000, Some(700_000)), Some((700_000, false)));
        assert!(!run.record());
        let mut run = running();
        assert_eq!(run.finish(801_000, Some(700_000)), Some((800_000, false)));
        assert!(!run.record());
    }

    #[test]
    fn the_first_finished_run_is_the_first_best() {
        let mut run = running();
        assert_eq!(run.finish(61_000, None), Some((60_000, true)));
        assert!(run.record());
    }

    #[test]
    fn credits_before_any_run_change_nothing() {
        let mut run = Run::default();
        assert_eq!(run.finish(1000, None), None);
        run.observe(0, Some(SPAWN));
        assert_eq!(run.finish(1000, None), None);
        assert_eq!(run, Run::Armed { spawn: SPAWN });
    }

    #[test]
    fn leaving_the_world_resets_it_for_the_next_run() {
        for mut run in [
            running(),
            Run::Stopped {
                time: 5,
                end: End::ByHand,
            },
            Run::Armed { spawn: SPAWN },
        ] {
            run.reset();
            assert_eq!(run, Run::Idle);
            assert_eq!(run.time(123_456), 0);
            // The next world: where the player appears, then their first move.
            run.observe(200_000, Some([1.5, 80.0, 1.5]));
            run.observe(200_050, Some([1.7, 80.0, 1.5]));
            assert_eq!(run, Run::Running { since: 200_050 });
        }
    }

    #[test]
    fn by_hand_start_and_stop() {
        let mut run = Run::default();
        assert!(run.start(500));
        assert_eq!(run, Run::Running { since: 500 });
        assert!(!run.start(900), "already running");
        assert_eq!(run, Run::Running { since: 500 });
        assert_eq!(run.stop(2_500), Some(2_000));
        assert_eq!(
            run,
            Run::Stopped {
                time: 2_000,
                end: End::ByHand
            }
        );
        assert!(!run.record(), "a run stopped by hand is never a best");
        assert_eq!(run.stop(3_000), None);
        assert_eq!(
            run.finish(3_000, None),
            None,
            "and the credits can't keep it"
        );
        // Start again from zero.
        assert!(run.start(10_000));
        assert_eq!(run.time(10_250), 250);
    }

    #[test]
    fn only_the_credits_of_a_run_end_it() {
        assert!(ends_run("playing", "win"));
        assert!(ends_run("dead", "win"));
        assert!(!ends_run("options", "win"), "Credits & Attribution");
        assert!(!ends_run("playing", "dead"));
        assert!(!ends_run("win", "playing"));
        assert!(!ends_run("playing", "paused"));
    }

    #[test]
    fn a_best_is_beaten_only_by_a_faster_run() {
        assert!(beats(60_000, None));
        assert!(beats(59_999, Some(60_000)));
        assert!(!beats(60_000, Some(60_000)));
        assert!(!beats(60_001, Some(60_000)));
    }

    #[test]
    fn the_best_file_reads_back_what_it_wrote() {
        assert_eq!(time_line(754_321), "754321 12:34.321\n");
        assert_eq!(time_line(5_025_678), "5025678 01:23:45.678\n");
        for ms in [1, 999, 59_999, 754_321, 3_600_000, 360_000_000] {
            assert_eq!(parse_best(&time_line(ms)), Some(ms));
        }
    }

    #[test]
    fn the_best_file_skips_what_isnt_a_time() {
        assert_eq!(parse_best(""), None);
        assert_eq!(parse_best("\n\n"), None);
        assert_eq!(parse_best("fast\n"), None);
        assert_eq!(parse_best("12:34.321\n"), None);
        assert_eq!(parse_best("0\n"), None, "no run takes no time");
        assert_eq!(parse_best("-5\n"), None);
        assert_eq!(parse_best("754321\r\n"), Some(754_321));
        assert_eq!(parse_best("  754321  whatever\n"), Some(754_321));
        assert_eq!(parse_best("\r\nnot\n754321 12:34.321\n"), Some(754_321));
    }

    #[test]
    fn reading_the_best_tells_no_file_from_a_failed_read() {
        assert_eq!(
            stored_best(Ok(b"754321 12:34.321\n".to_vec())),
            Ok(Some(754_321))
        );
        assert_eq!(stored_best(Ok(Vec::new())), Ok(None));
        assert_eq!(stored_best(Err(ConfigFileError::NotFound)), Ok(None));
        assert_eq!(
            stored_best(Err(ConfigFileError::Io("denied".into()))),
            Err(ConfigFileError::Io("denied".into()))
        );
        // Not UTF-8: read as far as it makes sense, never a panic.
        assert_eq!(stored_best(Ok(vec![0xFF, b'\n', b'4', b'2'])), Ok(Some(42)));
    }

    #[test]
    fn runs_are_added_one_line_each() {
        let first = with_run(Err(ConfigFileError::NotFound), 754_321).unwrap();
        assert_eq!(first, "754321 12:34.321\n");
        let second = with_run(Ok(first.into_bytes()), 700_000).unwrap();
        assert_eq!(second, "754321 12:34.321\n700000 11:40.000\n");
        assert_eq!(parse_runs(&second), [754_321, 700_000]);
        // A file someone edited without a last newline, or with blank and other lines.
        let edited = with_run(Ok(b"754321\r\n\njunk\n5".to_vec()), 60_000).unwrap();
        assert_eq!(edited, "754321\r\n\njunk\n5\n60000 01:00.000\n");
        assert_eq!(parse_runs(&edited), [754_321, 5, 60_000]);
        assert_eq!(with_run(Ok(Vec::new()), 1).unwrap(), "1 00:00.001\n");
    }

    #[test]
    fn runs_that_couldnt_be_read_are_not_written_over() {
        assert_eq!(with_run(Err(ConfigFileError::Io("busy".into())), 1), None);
        assert_eq!(with_run(Err(ConfigFileError::NotAFile), 1), None);
    }

    #[test]
    fn the_data_files_have_names_the_game_takes() {
        assert!(ConfigPath::new(BEST_FILE).is_ok());
        assert!(ConfigPath::new(RUNS_FILE).is_ok());
    }

    #[test]
    fn speedrun_and_its_words_are_the_commands() {
        assert_eq!(parse_command("speedrun"), Some(Action::Status));
        assert_eq!(parse_command("speedrun start"), Some(Action::Start));
        assert_eq!(parse_command("speedrun stop"), Some(Action::Stop));
        assert_eq!(parse_command("speedrun reset"), Some(Action::Reset));
        assert_eq!(parse_command("speedrun best"), Some(Action::Best));
        assert_eq!(parse_command("speedrun  start "), Some(Action::Start));
        assert_eq!(parse_command("speedrun go"), Some(Action::Help));
        assert_eq!(parse_command("speedrun start now"), Some(Action::Help));
        assert_eq!(parse_command("speedrunx"), None);
        assert_eq!(parse_command("tp 0 0 0"), None);
        assert_eq!(parse_command(""), None);
        for usage in USAGES {
            assert_ne!(parse_command(usage), Some(Action::Help), "{usage}");
            assert_ne!(parse_command(usage), None, "{usage}");
        }
    }

    #[test]
    fn the_commands_answer_in_chat() {
        let mut run = Run::default();
        assert_eq!(
            act(&mut run, None, Action::Status, 0),
            "Speedrun: 00:00.000 (waiting for your first move). No personal best yet: finish a run to set one."
        );
        assert_eq!(
            act(&mut run, None, Action::Stop, 0),
            "The speedrun timer isn't running."
        );
        assert_eq!(
            act(&mut run, None, Action::Start, 1_000),
            "Speedrun timer started."
        );
        assert_eq!(
            act(&mut run, Some(600_000), Action::Status, 61_000),
            "Speedrun: 01:00.000 (running). Personal best: 10:00.000."
        );
        assert_eq!(
            act(&mut run, None, Action::Start, 2_000),
            "The speedrun timer is already running."
        );
        assert_eq!(
            act(&mut run, None, Action::Stop, 4_000),
            "Speedrun timer stopped at 00:03.000. A run stopped by hand is not kept."
        );
        assert_eq!(
            act(&mut run, None, Action::Reset, 5_000),
            "Speedrun timer reset: it starts on your next move."
        );
        assert_eq!(run, Run::Idle);
        assert_eq!(
            act(&mut run, None, Action::Help, 0),
            "Usage: /speedrun [start|stop|reset|best]"
        );
        assert_eq!(
            best_message(Some(754_321), Some(3)),
            "Personal best: 12:34.321. 3 finished runs kept."
        );
        assert_eq!(
            best_message(Some(754_321), Some(1)),
            "Personal best: 12:34.321. 1 finished run kept."
        );
        assert_eq!(
            best_message(None, None),
            "No personal best yet: finish a run to set one."
        );
    }

    #[test]
    fn a_finished_run_is_told_in_chat() {
        assert_eq!(
            finish_line(600_000, true, Some(700_000)),
            "Run finished: 10:00.000, a new personal best (was 11:40.000)."
        );
        assert_eq!(
            finish_line(600_000, true, None),
            "Run finished: 10:00.000, your first personal best."
        );
        assert_eq!(
            finish_line(800_000, false, Some(700_000)),
            "Run finished: 13:20.000. Personal best: 11:40.000."
        );
    }

    #[test]
    fn the_panel_shows_the_time_then_the_best() {
        let waiting = Run::Armed { spawn: SPAWN };
        assert_eq!(
            panel(&waiting, 5_000, None, true),
            [
                ("00:00.000".to_owned(), GREY),
                ("Best --:--.---".to_owned(), GREY)
            ]
        );
        let run = running();
        assert_eq!(
            panel(&run, 62_345, Some(754_321), true),
            [
                ("01:01.345".to_owned(), INK),
                ("Best 12:34.321".to_owned(), GREY)
            ]
        );
        assert_eq!(
            panel(&run, 62_345, Some(754_321), false),
            [("01:01.345".to_owned(), INK)]
        );
    }

    #[test]
    fn a_finished_run_that_beat_the_best_shows_green() {
        let mut run = running();
        run.finish(601_000, Some(754_321));
        assert_eq!(panel(&run, 9_999_999, Some(600_000), false)[0].1, RECORD);
        let mut run = running();
        run.finish(801_000, Some(754_321));
        assert_eq!(panel(&run, 9_999_999, Some(754_321), false)[0].1, INK);
        let mut run = running();
        run.stop(601_000);
        assert_eq!(panel(&run, 9_999_999, Some(754_321), false)[0].1, INK);
    }

    #[test]
    fn the_panel_fits_its_lines() {
        assert_eq!(size(60.0, 1), [64.0, 12.0]);
        assert_eq!(size(60.0, 2), [64.0, 22.0]);
    }
}
