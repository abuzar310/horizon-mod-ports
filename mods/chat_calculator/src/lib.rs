//! Chat Calculator: a chat message that starts with `=` is worked out on this computer and never
//! sent. `=2*64+5` shows the line `2*64+5 = 133 (2 stacks + 5)` in your own chat only. It reads
//! numbers with decimals, `+ - * / % ^` (power, right associative), parentheses, unary minus and
//! Minecraft's units as words after a number: `st` (a stack, 64), `sb` (a shulker box full of
//! stacks, 1728) and `dc` (a double chest full of stacks, 3456).
//!
//! A message starting with `==`, or a lone `=`, is sent as the player typed it.
use mod_api::{
    ClientChatEvent, ClientHost, Flow, Hooks, Registrar, RegistrationContext, SettingSpec, Text,
};

/// The mod's id: its settings folder and translation keys.
pub const ID: &str = "chat_calculator";
/// The name error lines start with.
pub const NAME: &str = "Chat Calculator";
/// Items in a stack, a shulker box full of stacks and a double chest full of stacks.
pub const STACK: f64 = 64.0;
pub const SHULKER_BOX: f64 = 27.0 * STACK;
pub const DOUBLE_CHEST: f64 = 54.0 * STACK;
/// How deep parentheses, unary signs and powers may nest. Typed chat is at most 256 characters, so
/// this is never reached by a real sum, and it keeps a silly one from running out of stack.
pub const MAX_DEPTH: usize = 64;
/// From this size on an answer is written as `1.5e20`: past it a 64-bit float no longer holds
/// every whole number, and the full digits would be noise.
pub const SCIENTIFIC_FROM: f64 = 1e15;

/// The settings on Chat Calculator's page in Options > Mods.
pub fn settings() -> [SettingSpec; 1] {
    [
        SettingSpec::toggle("stacks", "chat_calculator.setting.stacks", true)
            .tooltip("chat_calculator.setting.stacks.tooltip"),
    ]
}

/// Why a sum has no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcError {
    /// Not a sum this calculator reads: `2**`, `(1+2`, `2xy`, nothing at all.
    Unreadable,
    /// `1/0`, `5%0` or `0^-1`.
    DivideByZero,
    /// A number or an answer past the largest a 64-bit float holds.
    TooBig,
    /// No real number answers it: `(-8)^0.5`.
    NoAnswer,
}

/// One piece of a sum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Token {
    Number(f64),
    /// One of `+ - * / % ^`.
    Op(char),
    Open,
    Close,
    /// A unit word after a number, as the items it stands for: `st` is 64.
    Unit(f64),
}

/// The items a unit word stands for, any case: `st`, `sb`, `dc`.
pub fn unit(word: &str) -> Option<f64> {
    match word.to_ascii_lowercase().as_str() {
        "st" => Some(STACK),
        "sb" => Some(SHULKER_BOX),
        "dc" => Some(DOUBLE_CHEST),
        _ => None,
    }
}

/// Splits a sum into its tokens. Numbers are digits with at most one `.` (`12`, `1.5`, `.5`);
/// words are units; spaces are skipped. Anything else can't be read.
pub fn tokenize(text: &str) -> Result<Vec<Token>, CalcError> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let token = match c {
            c if c.is_whitespace() => continue,
            '+' | '-' | '*' | '/' | '%' | '^' => Token::Op(c),
            '(' => Token::Open,
            ')' => Token::Close,
            c if c.is_ascii_digit() || c == '.' => {
                let mut end = start + 1;
                let mut dot = c == '.';
                while let Some(&(at, next)) = chars.peek() {
                    if next.is_ascii_digit() || (next == '.' && !dot) {
                        dot |= next == '.';
                        end = at + 1;
                        chars.next();
                    } else {
                        break;
                    }
                }
                let digits = &text[start..end];
                if digits == "." {
                    return Err(CalcError::Unreadable);
                }
                let value: f64 = digits.parse().map_err(|_| CalcError::Unreadable)?;
                Token::Number(checked(value)?)
            }
            c if c.is_ascii_alphabetic() => {
                let mut end = start + 1;
                while let Some(&(at, next)) = chars.peek() {
                    if next.is_ascii_alphabetic() {
                        end = at + 1;
                        chars.next();
                    } else {
                        break;
                    }
                }
                Token::Unit(unit(&text[start..end]).ok_or(CalcError::Unreadable)?)
            }
            _ => return Err(CalcError::Unreadable),
        };
        tokens.push(token);
    }
    Ok(tokens)
}

/// A finite number, or why it is not one.
fn checked(value: f64) -> Result<f64, CalcError> {
    if value.is_nan() {
        Err(CalcError::NoAnswer)
    } else if value.is_infinite() {
        Err(CalcError::TooBig)
    } else {
        Ok(value)
    }
}

/// A recursive descent over the tokens, one method a level, loosest first:
///
/// ```text
/// sum     := product (('+' | '-') product)*
/// product := signed (('*' | '/' | '%') signed)*
/// signed  := ('-' | '+') signed | power
/// power   := counted ('^' signed)?          right associative: 2^3^2 is 2^9
/// counted := primary unit?                  3st, 1.5sb, (1+1)dc
/// primary := number | '(' sum ')'
/// ```
///
/// A sign binds looser than `^`, as in maths: `-2^2` is -4, and `2^-3` is 0.125.
struct Parser<'t> {
    tokens: &'t [Token],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.at).copied()
    }

    fn eat_op(&mut self, ops: &[char]) -> Option<char> {
        match self.peek() {
            Some(Token::Op(op)) if ops.contains(&op) => {
                self.at += 1;
                Some(op)
            }
            _ => None,
        }
    }

    fn sum(&mut self) -> Result<f64, CalcError> {
        let mut value = self.product()?;
        while let Some(op) = self.eat_op(&['+', '-']) {
            let right = self.product()?;
            value = checked(if op == '+' {
                value + right
            } else {
                value - right
            })?;
        }
        Ok(value)
    }

    fn product(&mut self) -> Result<f64, CalcError> {
        let mut value = self.signed()?;
        while let Some(op) = self.eat_op(&['*', '/', '%']) {
            let right = self.signed()?;
            value = match op {
                '*' => checked(value * right)?,
                _ if right == 0.0 => return Err(CalcError::DivideByZero),
                '/' => checked(value / right)?,
                _ => checked(value % right)?,
            };
        }
        Ok(value)
    }

    /// Every way down the grammar (a bracket, a sign, a power) passes here, so the depth is
    /// counted once, here.
    fn signed(&mut self) -> Result<f64, CalcError> {
        if self.depth >= MAX_DEPTH {
            return Err(CalcError::Unreadable);
        }
        self.depth += 1;
        let value = match self.eat_op(&['-', '+']) {
            Some('-') => self.signed().map(|v| -v),
            Some(_) => self.signed(),
            None => self.power(),
        };
        self.depth -= 1;
        value
    }

    fn power(&mut self) -> Result<f64, CalcError> {
        let base = self.counted()?;
        if self.eat_op(&['^']).is_none() {
            return Ok(base);
        }
        let exponent = self.signed()?;
        if base == 0.0 && exponent < 0.0 {
            return Err(CalcError::DivideByZero);
        }
        checked(base.powf(exponent))
    }

    fn counted(&mut self) -> Result<f64, CalcError> {
        let value = self.primary()?;
        match self.peek() {
            Some(Token::Unit(items)) => {
                self.at += 1;
                checked(value * items)
            }
            _ => Ok(value),
        }
    }

    fn primary(&mut self) -> Result<f64, CalcError> {
        let token = self.peek().ok_or(CalcError::Unreadable)?;
        self.at += 1;
        match token {
            Token::Number(value) => Ok(value),
            Token::Open => {
                let value = self.sum()?;
                if self.peek() != Some(Token::Close) {
                    return Err(CalcError::Unreadable);
                }
                self.at += 1;
                Ok(value)
            }
            _ => Err(CalcError::Unreadable),
        }
    }
}

/// Works out a sum: `2*64+5` is 133, `3st+12` is 204.
pub fn evaluate(expression: &str) -> Result<f64, CalcError> {
    let tokens = tokenize(expression)?;
    let mut parser = Parser {
        tokens: tokens.as_slice(),
        at: 0,
        depth: 0,
    };
    let value = parser.sum()?;
    if parser.at != tokens.len() {
        return Err(CalcError::Unreadable);
    }
    Ok(value)
}

/// The sum in a chat message, when it is one for the calculator: the text after a leading `=`,
/// trimmed. `None` for a message to send as it is: one that doesn't start with `=`, starts with
/// `==`, or has nothing after the `=`.
pub fn expression(message: &str) -> Option<&str> {
    let rest = message.trim().strip_prefix('=')?;
    if rest.starts_with('=') {
        return None;
    }
    let rest = rest.trim();
    (!rest.is_empty()).then_some(rest)
}

/// `text` without the zeros a fixed-point number ends in, nor a `.` left bare: `2.500000` is
/// `2.5`, `64.000000` is `64`.
fn trim_zeros(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}

/// An answer as the chat shows it: up to 6 decimals, without trailing zeros (`0.333333`, `2.5`,
/// `133`), and from [`SCIENTIFIC_FROM`] on as `1.152922e18`. Never `-0`.
pub fn format_number(value: f64) -> String {
    if value.abs() >= SCIENTIFIC_FROM {
        // `{:.6e}` always writes an `e`: `1.152922e18`.
        let text = format!("{value:.6e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
        return format!("{}e{exponent}", trim_zeros(mantissa));
    }
    let text = format!("{value:.6}");
    match trim_zeros(&text) {
        "-0" => "0".to_owned(),
        trimmed => trimmed.to_owned(),
    }
}

/// The stacks an answer makes, for a whole number of items of 64 or more as the chat shows it
/// (`133`): `2 stacks + 5`, `1 stack`, `27 stacks`. `None` for anything else (`63`, `-64`,
/// `64.5`, `1e15`).
pub fn stacks_form(shown: &str) -> Option<String> {
    let items: u64 = shown.parse().ok()?;
    if items < 64 {
        return None;
    }
    let (stacks, rest) = (items / 64, items % 64);
    let word = if stacks == 1 { "stack" } else { "stacks" };
    Some(if rest == 0 {
        format!("{stacks} {word}")
    } else {
        format!("{stacks} {word} + {rest}")
    })
}

/// The short line an error shows: `Chat Calculator: can't read '2**'`.
pub fn error_line(expression: &str, error: CalcError) -> String {
    match error {
        CalcError::Unreadable => format!("{NAME}: can't read '{expression}'"),
        CalcError::DivideByZero => format!("{NAME}: can't divide by zero in '{expression}'"),
        CalcError::TooBig => format!("{NAME}: '{expression}' is too big"),
        CalcError::NoAnswer => format!("{NAME}: '{expression}' has no real answer"),
    }
}

/// The line a sum shows: `2*64+5 = 133 (2 stacks + 5)`, without the stacks when `stacks` is off,
/// or the error's line.
pub fn reply(expression: &str, stacks: bool) -> String {
    match evaluate(expression) {
        Ok(value) => {
            let shown = format_number(value);
            let form = if stacks { stacks_form(&shown) } else { None };
            match form {
                Some(form) => format!("{expression} = {shown} ({form})"),
                None => format!("{expression} = {shown}"),
            }
        }
        Err(error) => error_line(expression, error),
    }
}

#[derive(Default)]
struct ChatCalculator;

#[mod_api::horizon_mod]
impl Hooks for ChatCalculator {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for setting in settings() {
            // Only a malformed spec is refused, and the tests check every spec.
            let _ = r.register_setting(setting);
        }
    }

    /// A message about to be sent from the chat box: one starting with `=` is answered with a
    /// line only this player sees, and cancelled, so the server never gets it, even when it can't
    /// be read.
    fn client_chat(&self, host: &mut dyn ClientHost, event: &mut ClientChatEvent<'_>) -> Flow {
        if !host.on(ID) {
            return Flow::Continue;
        }
        let Some(sum) = expression(event.message.as_str()) else {
            return Flow::Continue;
        };
        let stacks = host.setting_bool("stacks").unwrap_or(true);
        // Only text past the boundary's size limits is refused, and a chat message is at most
        // 256 characters.
        let _ = host.chat_line(Text::literal(reply(sum, stacks)));
        Flow::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The answer as the chat shows it, or the error.
    fn shown(expression: &str) -> Result<String, CalcError> {
        evaluate(expression).map(format_number)
    }

    fn ok(expression: &str, answer: &str) {
        assert_eq!(shown(expression), Ok(answer.to_owned()), "{expression}");
    }

    fn err(expression: &str, error: CalcError) {
        assert_eq!(evaluate(expression), Err(error), "{expression}");
    }

    #[test]
    fn every_setting_is_one_the_game_takes() {
        for setting in settings() {
            assert_eq!(setting.check(), Ok(()), "{}", setting.key);
        }
    }

    #[test]
    fn only_a_message_starting_with_one_equals_sign_is_a_sum() {
        assert_eq!(expression("=2*64+5"), Some("2*64+5"));
        assert_eq!(expression("  =  3st + 12 "), Some("3st + 12"));
        assert_eq!(expression("=2**"), Some("2**"));
        assert_eq!(expression("=)"), Some(")"));
        // Sent as typed.
        assert_eq!(expression("="), None);
        assert_eq!(expression("=   "), None);
        assert_eq!(expression("=="), None);
        assert_eq!(expression("==5"), None);
        assert_eq!(expression("== look here"), None);
        assert_eq!(expression("hello"), None);
        assert_eq!(expression("2=2"), None);
        assert_eq!(expression(""), None);
        assert_eq!(expression("/give @s stone"), None);
    }

    #[test]
    fn times_and_divide_before_plus_and_minus() {
        ok("2+3*4", "14");
        ok("2*3+4", "10");
        ok("(2+3)*4", "20");
        ok("2*64+5", "133");
        ok("10-2*3", "4");
        ok("1+6/3", "3");
        ok("2*3%4", "2");
        ok("7%4*2", "6");
        ok("7.5%2", "1.5");
        ok("-7%3", "-1");
        ok("2+3^2", "11");
        ok("2*3^2", "18");
        ok("2^2*3", "12");
    }

    #[test]
    fn plus_minus_times_and_divide_go_left_to_right() {
        ok("10-4-3", "3");
        ok("100/10/5", "2");
        ok("2*3/4", "1.5");
        ok("8/4*2", "4");
        ok("1-1+1", "1");
    }

    #[test]
    fn power_goes_right_to_left() {
        ok("2^3^2", "512");
        ok("(2^3)^2", "64");
        ok("2^-3", "0.125");
        ok("4^0.5", "2");
        ok("2^0", "1");
        ok("0^0", "1");
    }

    #[test]
    fn unary_minus() {
        ok("-2^2", "-4");
        ok("(-2)^2", "4");
        ok("-3+5", "2");
        ok("1--1", "2");
        ok("2*-3", "-6");
        ok("--2", "2");
        ok("-(2+3)", "-5");
        ok("+5", "5");
        ok("-0", "0");
        ok("2^-1^2", "0.5");
        err("2*", CalcError::Unreadable);
        err("-", CalcError::Unreadable);
    }

    #[test]
    fn parentheses() {
        ok("((1+2))*3", "9");
        ok("(1)", "1");
        ok("2*(3+4)*5", "70");
        err("(1+2", CalcError::Unreadable);
        err("1+2)", CalcError::Unreadable);
        err("()", CalcError::Unreadable);
        err("2(3)", CalcError::Unreadable);
        err(")(", CalcError::Unreadable);
    }

    #[test]
    fn decimals_and_spaces() {
        ok("1.5+1.25", "2.75");
        ok(".5*4", "2");
        ok("5.", "5");
        ok("0.1+0.2", "0.3");
        ok(" 2 *  64 + 5 ", "133");
        ok("1/3", "0.333333");
        ok("2/3", "0.666667");
        ok("-1/3", "-0.333333");
        ok("007", "7");
        err("1.2.3", CalcError::Unreadable);
        err(".", CalcError::Unreadable);
        err("1 2", CalcError::Unreadable);
        err("1,5", CalcError::Unreadable);
    }

    #[test]
    fn unit_words_after_a_number() {
        ok("3st+12", "204");
        ok("1.5sb", "2592");
        ok("1dc", "3456");
        ok("2 st", "128");
        ok("3ST", "192");
        ok("(1+1)st", "128");
        ok("1sb/1st", "27");
        ok("1dc/1sb", "2");
        ok("2st^2", "16384");
        ok("-3st", "-192");
        ok("0.5st", "32");
        err("st", CalcError::Unreadable);
        err("2st st", CalcError::Unreadable);
        err("2 stacks", CalcError::Unreadable);
        err("2xy", CalcError::Unreadable);
        err("2e5", CalcError::Unreadable);
        err("inf", CalcError::Unreadable);
        err("nan", CalcError::Unreadable);
    }

    #[test]
    fn division_by_zero_is_an_error() {
        err("1/0", CalcError::DivideByZero);
        err("0/0", CalcError::DivideByZero);
        err("5%0", CalcError::DivideByZero);
        err("1/(2-2)", CalcError::DivideByZero);
        err("1/-0", CalcError::DivideByZero);
        err("0^-1", CalcError::DivideByZero);
        ok("0/5", "0");
        ok("0^2", "0");
    }

    #[test]
    fn very_large_numbers() {
        ok("99999999999999", "99999999999999");
        // The 1e15 rule on typed numbers, then the same through `^`.
        ok("100000000000000", "100000000000000");
        ok("1000000000000000", "1e15");
        ok("10^14", "100000000000000");
        ok("10^15", "1e15");
        ok("2^60", "1.152922e18");
        ok("-10^20", "-1e20");
        ok("1.5*10^20", "1.5e20");
        ok("10^308", "1e308");
        err("10^308*10", CalcError::TooBig);
        err("10^400", CalcError::TooBig);
        err("9^9^9", CalcError::TooBig);
        err("-10^308*10", CalcError::TooBig);
        let digits = format!("1{}", "0".repeat(400));
        err(&digits, CalcError::TooBig);
        // Tiny answers round to zero at six decimals, never `-0`.
        ok("1/10000000", "0");
        ok("-1/10000000", "0");
    }

    #[test]
    fn no_real_answer() {
        err("(-8)^0.5", CalcError::NoAnswer);
        err("(-8)^(1/3)", CalcError::NoAnswer);
        ok("(-8)^3", "-512");
    }

    #[test]
    fn deep_nesting_is_refused_not_overflowed() {
        let nested = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
        ok(&nested(30), "1");
        err(&nested(200), CalcError::Unreadable);
        ok(&format!("{}1", "-".repeat(30)), "1");
        err(&format!("{}1", "-".repeat(300)), CalcError::Unreadable);
        err(&format!("2{}", "^2".repeat(100)), CalcError::Unreadable);
        // A long flat sum is not deep: the depth comes back after each term.
        let long = vec!["1"; 128].join("+");
        ok(&long, "128");
    }

    #[test]
    fn numbers_are_written_with_up_to_six_decimals() {
        assert_eq!(format_number(2.5), "2.5");
        assert_eq!(format_number(100.0), "100");
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(-0.000_000_1), "0");
        assert_eq!(format_number(1.000_000_4), "1");
        assert_eq!(format_number(1_234_567.891), "1234567.891");
        assert_eq!(format_number(0.125), "0.125");
        assert_eq!(format_number(-2.25), "-2.25");
        assert_eq!(format_number(1e15), "1e15");
        assert_eq!(format_number(-1e20), "-1e20");
        assert_eq!(format_number(1.234_567_89e300), "1.234568e300");
    }

    #[test]
    fn the_stacks_wording() {
        assert_eq!(stacks_form("63"), None);
        assert_eq!(stacks_form("64").as_deref(), Some("1 stack"));
        assert_eq!(stacks_form("65").as_deref(), Some("1 stack + 1"));
        assert_eq!(stacks_form("127").as_deref(), Some("1 stack + 63"));
        assert_eq!(stacks_form("128").as_deref(), Some("2 stacks"));
        assert_eq!(stacks_form("133").as_deref(), Some("2 stacks + 5"));
        assert_eq!(stacks_form("1728").as_deref(), Some("27 stacks"));
        assert_eq!(stacks_form("2592").as_deref(), Some("40 stacks + 32"));
        assert_eq!(stacks_form("3456").as_deref(), Some("54 stacks"));
        assert_eq!(
            stacks_form("99999999999999").as_deref(),
            Some("1562499999999 stacks + 63")
        );
        assert_eq!(stacks_form("0"), None);
        assert_eq!(stacks_form("-64"), None);
        assert_eq!(stacks_form("64.5"), None);
        assert_eq!(stacks_form("1e15"), None);
    }

    #[test]
    fn the_lines_in_chat() {
        assert_eq!(reply("2*64+5", true), "2*64+5 = 133 (2 stacks + 5)");
        assert_eq!(reply("2*64+5", false), "2*64+5 = 133");
        assert_eq!(reply("3st+12", true), "3st+12 = 204 (3 stacks + 12)");
        assert_eq!(reply("1.5sb", true), "1.5sb = 2592 (40 stacks + 32)");
        assert_eq!(reply("27*64", true), "27*64 = 1728 (27 stacks)");
        assert_eq!(reply("60+4", true), "60+4 = 64 (1 stack)");
        assert_eq!(reply("60+3", true), "60+3 = 63");
        assert_eq!(reply("1/3", true), "1/3 = 0.333333");
        assert_eq!(reply("-3st", true), "-3st = -192");
        assert_eq!(reply("100.5", true), "100.5 = 100.5");
        assert_eq!(reply("10^15", true), "10^15 = 1e15");
        assert_eq!(reply("2**", true), "Chat Calculator: can't read '2**'");
        assert_eq!(
            reply("1/0", true),
            "Chat Calculator: can't divide by zero in '1/0'"
        );
        assert_eq!(
            reply("10^400", true),
            "Chat Calculator: '10^400' is too big"
        );
        assert_eq!(
            reply("(-8)^0.5", true),
            "Chat Calculator: '(-8)^0.5' has no real answer"
        );
    }

    /// The stacks follow the answer shown, not the float behind it: 64.1-0.1 is
    /// 63.99999999999999 to a float, shown as 64, so it is one stack.
    #[test]
    fn a_float_just_off_a_whole_number_still_counts_in_stacks() {
        assert_eq!(reply("64.1-0.1", true), "64.1-0.1 = 64 (1 stack)");
        assert_eq!(reply("1.15*100", true), "1.15*100 = 115 (1 stack + 51)");
        assert_eq!(
            reply("1.1*1.1*100", true),
            "1.1*1.1*100 = 121 (1 stack + 57)"
        );
    }
}
