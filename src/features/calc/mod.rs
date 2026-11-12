//! The inline calculator: arithmetic in symbols or words, percentages, functions, number bases,
//! units and live currency, and (in `dates`) countdowns, date maths and time zones.

mod currency;
mod dates;
mod units;
pub mod zones;

use crate::model::Card;
use units::{Unit, ANGLE, AREA, CURRENCY, DATA, DATA_RATE, ENERGY, LENGTH, POWER, SPEED, TEMP, TIME, VOLUME};

pub use currency::{set_home as set_home_currency, set_waker, take_dirty, warm};

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    Comma,
}

/// A number, and the unit it is measured in when it has one.
#[derive(Clone, Debug)]
struct Val {
    n: f64,
    unit: Option<Unit>,
}

impl Val {
    fn plain(n: f64) -> Self {
        Self { n, unit: None }
    }
    fn base(&self) -> f64 {
        self.unit.as_ref().map_or(self.n, |u| u.to_base(self.n))
    }
}

/// Words folded to symbols before the expression is read.
static WORDS: &[(&str, &str)] = &[
    ("divided by", "/"), ("multiplied by", "*"), ("to the power of", "^"), ("square root of", "sqrt"), ("cube root of", "cbrt"),
    ("squared", "^2"), ("cubed", "^3"), ("half of", "0.5 *"), ("a quarter of", "0.25 *"), ("quarter of", "0.25 *"), ("a third of", "(1/3) *"),
    ("double", "2 *"), ("twice", "2 *"), ("triple", "3 *"), ("percentage", "%"), ("percent", "%"), ("per cent", "%"), ("pct", "%"),
];

/// Replaces whole-word phrases, whatever their case.
fn fold(src: &str, table: &[(&str, &str)]) -> String {
    let mut out = src.to_string();
    for (phrase, to) in table {
        let mut from = 0;
        while from < out.len() {
            let lower = out.to_ascii_lowercase();
            let Some(at) = lower.get(from..).and_then(|rest| rest.find(phrase)).map(|i| i + from) else { break };
            let end = at + phrase.len();
            let edge = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
            if edge(lower[..at].chars().next_back()) && edge(lower[end..].chars().next()) {
                out.replace_range(at..end, to);
                from = at + to.len();
            } else {
                from = end;
            }
        }
    }
    out
}

fn normalize(q: &str) -> String {
    let mut q = q.trim();
    let lower = q.to_ascii_lowercase();
    for lead in ["what is ", "what's ", "whats ", "how much is ", "calculate ", "compute ", "calc ", "convert "] {
        if lower.starts_with(lead) {
            q = q[lead.len()..].trim();
            break;
        }
    }
    fold(&fold(&fold(q, WORDS), units::PHRASES), currency::PHRASES)
}

/// `kelvin` reads `300k` as a temperature rather than as three hundred thousand.
fn lex(src: &str, kelvin: bool) -> Option<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let (mut i, mut out) = (0, Vec::new());
    let at = |i: usize| chars.get(i).copied();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '0' && at(i + 1).is_some_and(|p| matches!(p, 'x' | 'X' | 'b' | 'B' | 'o' | 'O'))
            && at(i + 2).is_some_and(|d| d.is_digit(match at(i + 1) { Some('x' | 'X') => 16, Some('b' | 'B') => 2, _ => 8 }))
        {
            let radix = match chars[i + 1] {
                'x' | 'X' => 16,
                'b' | 'B' => 2,
                _ => 8,
            };
            let start = i + 2;
            i = start;
            while i < chars.len() && (chars[i].is_digit(radix) || chars[i] == '_') {
                i += 1;
            }
            let text: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            out.push(Tok::Num(u64::from_str_radix(&text, radix).ok()? as f64));
        } else if c.is_ascii_digit() || (c == '.' && at(i + 1).is_some_and(|n| n.is_ascii_digit())) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_'
                || (chars[i] == ',' && chars[i + 1..].iter().take_while(|d| d.is_ascii_digit()).count() == 3)) {
                i += 1;
            }
            // Scientific notation.
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E')
                && at(i + 1).is_some_and(|n| n.is_ascii_digit() || ((n == '-' || n == '+') && at(i + 2).is_some_and(|d| d.is_ascii_digit()))) {
                i += 2;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = chars[start..i].iter().filter(|c| **c != ',' && **c != '_').collect();
            let mut n: f64 = text.parse().ok()?;
            // `5k`, `2bn`: a scale written against the number.
            let ends = |j: usize| at(j).is_none_or(|c| !c.is_alphanumeric());
            if !kelvin && at(i) == Some('k') && ends(i + 1) {
                n *= 1e3;
                i += 1;
            } else if at(i) == Some('b') && at(i + 1) == Some('n') && ends(i + 2) {
                n *= 1e9;
                i += 2;
            }
            out.push(Tok::Num(n));
        } else if c == '"' || c == '\'' || currency::is_symbol(c) {
            out.push(Tok::Ident(c.to_string()));
            i += 1;
        } else if c.is_alphabetic() || c == '°' || c == 'µ' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || matches!(chars[i], '°' | 'µ' | '²' | '³')) {
                i += 1;
            }
            let mut word: String = chars[start..i].iter().collect();
            // `2x3` multiplies.
            if word.len() > 1 && word.starts_with(['x', 'X']) && word[1..].chars().all(|c| c.is_ascii_digit()) {
                i = start + 1;
                word.truncate(1);
            }
            // `US$`, `A$`, `HK$`.
            if at(i) == Some('$') && matches!(word.to_lowercase().as_str(), "us" | "a" | "au" | "c" | "ca" | "hk" | "s" | "nz" | "nt" | "mx" | "r") {
                word.push('$');
                i += 1;
            }
            out.push(Tok::Ident(word));
        } else {
            let double = at(i + 1) == Some(c);
            out.push(match c {
                '*' if double => {
                    i += 1;
                    Tok::Op('^')
                }
                '<' | '>' if double => {
                    i += 1;
                    Tok::Op(c)
                }
                '+' | '-' | '*' | '/' | '^' | '%' | '!' | '&' | '|' => Tok::Op(c),
                '×' | '·' => Tok::Op('*'),
                '÷' => Tok::Op('/'),
                '−' | '–' => Tok::Op('-'),
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' | ';' => Tok::Comma,
                _ => return None,
            });
            i += 1;
        }
    }
    Some(out)
}

/// Words that join values rather than measure them.
fn joining(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "x" | "times" | "mod" | "of" | "off" | "plus" | "minus" | "xor" | "and" | "to" | "as")
}

fn scale(name: &str) -> Option<f64> {
    Some(match name.to_lowercase().as_str() {
        "hundred" => 1e2,
        "thousand" => 1e3,
        "lakh" | "lakhs" | "lac" => 1e5,
        "million" | "mn" | "mil" => 1e6,
        "crore" | "crores" => 1e7,
        "billion" => 1e9,
        "trillion" => 1e12,
        "dozen" => 12.0,
        _ => return None,
    })
}

fn imperial(unit: &Unit) -> bool {
    matches!(unit.symbol.as_str(), "in" | "ft" | "yd" | "mi" | "mph" | "ft/s" | "in²" | "ft²" | "yd²" | "mi²" | "acres")
}

/// A value of a kind worked out from others (a speed from a distance and a time), in the unit
/// people would read it in.
fn derived(kind: u8, base: f64, imperial: bool) -> Option<Val> {
    let name = match kind {
        AREA => if imperial { "sqft" } else { "sqm" },
        VOLUME => if imperial { "cuft" } else { "m3" },
        SPEED => if imperial { "mph" } else { "kph" },
        LENGTH if imperial => if base.abs() >= 1609.344 { "mi" } else { "ft" },
        LENGTH => if base.abs() >= 1000.0 { "km" } else { "m" },
        ENERGY => if base.abs() >= 3.6e6 { "kwh" } else { "wh" },
        POWER => if base.abs() >= 1000.0 { "kw" } else { "w" },
        DATA => match base.abs() {
            b if b >= 1e12 => "tb",
            b if b >= 1e9 => "gb",
            b if b >= 1e6 => "mb",
            _ => "kb",
        },
        DATA_RATE => "mb/s",
        TIME => match base.abs() {
            s if s >= 86_400.0 => "days",
            s if s >= 3600.0 => "h",
            s if s >= 60.0 => "min",
            _ => "s",
        },
        _ => return None,
    };
    let unit = units::find(name, Some(kind))?;
    Some(Val { n: unit.from_base(base), unit: Some(unit) })
}

fn add(l: Val, r: Val, sign: f64, percent: bool) -> Option<Val> {
    if percent && r.unit.is_none() {
        return Some(Val { n: l.n + sign * l.n * r.n, unit: l.unit });
    }
    match (&l.unit, &r.unit) {
        (Some(a), Some(b)) if a.kind != b.kind => None,
        (Some(a), Some(b)) if a == b => Some(Val { n: l.n + sign * r.n, unit: l.unit }),
        (Some(a), Some(b)) => Some(Val { n: l.n + sign * a.from_base(b.to_base(r.n)), unit: l.unit }),
        (None, Some(_)) => Some(Val { n: l.n + sign * r.n, unit: r.unit }),
        _ => Some(Val { n: l.n + sign * r.n, unit: l.unit }),
    }
}

fn mul(l: Val, r: Val) -> Option<Val> {
    let (a, b) = match (&l.unit, &r.unit) {
        (Some(a), Some(b)) => (a, b),
        (Some(_), None) => return Some(Val { n: l.n * r.n, unit: l.unit }),
        _ => return Some(Val { n: l.n * r.n, unit: r.unit }),
    };
    let product = l.base() * r.base();
    let (kind, base) = match (a.kind, b.kind) {
        (LENGTH, LENGTH) => (AREA, product),
        (AREA, LENGTH) | (LENGTH, AREA) => (VOLUME, product * 1000.0),
        (SPEED, TIME) | (TIME, SPEED) => (LENGTH, product),
        (POWER, TIME) | (TIME, POWER) => (ENERGY, product),
        (DATA_RATE, TIME) | (TIME, DATA_RATE) => (DATA, product),
        _ => return None,
    };
    derived(kind, base, imperial(a) || imperial(b))
}

fn div(l: Val, r: Val) -> Option<Val> {
    let (a, b) = match (&l.unit, &r.unit) {
        (Some(a), Some(b)) => (a, b),
        (_, None) => return Some(Val { n: l.n / r.n, unit: l.unit }),
        (None, Some(_)) => return None,
    };
    let ratio = l.base() / r.base();
    let (kind, base) = match (a.kind, b.kind) {
        (x, y) if x == y && x != TEMP => return Some(Val::plain(ratio)),
        (LENGTH, TIME) => (SPEED, ratio),
        (LENGTH, SPEED) => (TIME, ratio),
        (DATA, TIME) => (DATA_RATE, ratio),
        (DATA, DATA_RATE) => (TIME, ratio),
        (ENERGY, TIME) => (POWER, ratio),
        (ENERGY, POWER) => (TIME, ratio),
        (AREA, LENGTH) => (LENGTH, ratio),
        (VOLUME, AREA) => (LENGTH, ratio / 1000.0),
        (VOLUME, LENGTH) => (AREA, ratio / 1000.0),
        _ => return None,
    };
    derived(kind, base, imperial(a) || imperial(b))
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    /// The kind of unit an ambiguous name should mean: `oz` to `ml` is fluid.
    prefer: Option<u8>,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s.eq_ignore_ascii_case(w))
    }

    /// The unit named at a position, and the tokens that name it.
    fn unit_at(&self, pos: usize) -> Option<(Unit, usize)> {
        let Some(Tok::Ident(name)) = self.toks.get(pos) else { return None };
        if joining(name) || (function(name) && self.toks.get(pos + 1) == Some(&Tok::LParen)) {
            return None;
        }
        // `km/h`, `MB/s`.
        if let (Some(Tok::Op('/')), Some(Tok::Ident(per))) = (self.toks.get(pos + 1), self.toks.get(pos + 2))
            && let Some(unit) = units::find(&format!("{name}/{per}"), self.prefer)
        {
            return Some((unit, 3));
        }
        if self.prefer == Some(CURRENCY) && let Some(unit) = currency::find(name) {
            return Some((unit, 1));
        }
        units::find(name, self.prefer).or_else(|| currency::find(name)).map(|unit| (unit, 1))
    }

    /// Bitwise operators bind loosest: `0xff & 15`, `1 << 10`.
    fn bits(&mut self) -> Option<Val> {
        let mut left = self.expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(c @ ('&' | '|' | '<' | '>'))) => *c,
                _ if self.word("xor") => 'x',
                _ => return Some(left),
            };
            self.pos += 1;
            let right = self.expr()?;
            if left.unit.is_some() || right.unit.is_some() || left.n.fract() != 0.0 || right.n.fract() != 0.0 || left.n.abs() > 9e15 || right.n.abs() > 9e15 {
                return None;
            }
            let (a, b) = (left.n as i64, right.n as i64);
            left = Val::plain(match op {
                '&' => a & b,
                '|' => a | b,
                'x' => a ^ b,
                '<' => a.checked_shl(u32::try_from(b).ok()?)?,
                _ => a.checked_shr(u32::try_from(b).ok()?)?,
            } as f64);
        }
    }

    fn expr(&mut self) -> Option<Val> {
        let mut left = self.term()?;
        loop {
            let sign = match self.peek() {
                Some(Tok::Op('+')) => 1.0,
                Some(Tok::Op('-')) => -1.0,
                _ if self.word("plus") => 1.0,
                _ if self.word("minus") => -1.0,
                _ => return Some(left),
            };
            self.pos += 1;
            let (right, percent) = self.term_pct()?;
            left = add(left, right, sign, percent)?;
        }
    }

    /// A term, and whether it was a bare percentage (`200 + 10%` adds ten percent of 200).
    fn term_pct(&mut self) -> Option<(Val, bool)> {
        let start = self.pos;
        let value = self.term()?;
        let bare = self.pos == start + 2 && matches!(self.toks.get(start), Some(Tok::Num(_))) && self.toks.get(start + 1) == Some(&Tok::Op('%'));
        Some((value, bare))
    }

    fn term(&mut self) -> Option<Val> {
        let mut left = self.unary()?;
        loop {
            match self.peek() {
                Some(Tok::Op('*')) => {
                    self.pos += 1;
                    left = mul(left, self.unary()?)?;
                }
                Some(Tok::Op('/')) => {
                    self.pos += 1;
                    left = div(left, self.unary()?)?;
                }
                _ if self.word("x") || self.word("times") || self.word("of") => {
                    self.pos += 1;
                    left = mul(left, self.unary()?)?;
                }
                _ if self.word("mod") => {
                    self.pos += 1;
                    let right = self.unary()?;
                    left.n %= right.n;
                }
                // `10% off 80` takes a tenth away.
                _ if self.word("off") && left.unit.is_none() => {
                    self.pos += 1;
                    let right = self.unary()?;
                    left = Val { n: right.n * (1.0 - left.n), unit: right.unit };
                }
                // Implicit multiplication: `2(3)`, `2 pi`.
                Some(Tok::LParen) => left = mul(left, self.unary()?)?,
                Some(Tok::Ident(name)) if constant(name).is_some() || function(name) => left = mul(left, self.unary()?)?,
                _ => return Some(left),
            }
        }
    }

    fn unary(&mut self) -> Option<Val> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                let v = self.unary()?;
                Some(Val { n: -v.n, unit: v.unit })
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Option<Val> {
        let base = self.postfix()?;
        if self.peek() == Some(&Tok::Op('^')) {
            self.pos += 1;
            let exponent = self.unary()?;
            if base.unit.is_some() || exponent.unit.is_some() {
                return None;
            }
            return Some(Val::plain(base.n.powf(exponent.n)));
        }
        Some(base)
    }

    fn postfix(&mut self) -> Option<Val> {
        let mut value = self.atom()?;
        loop {
            if let Some(Tok::Ident(name)) = self.peek() && let Some(by) = scale(name) {
                value.n *= by;
                self.pos += 1;
                continue;
            }
            if value.unit.is_none() && let Some((unit, used)) = self.unit_at(self.pos) {
                self.pos += used;
                // `5 ft 3 in`, `1 h 30 min`: smaller units of the same kind add on.
                let mut smallest = unit.factor;
                while unit.offset == 0.0 && !unit.inverse && let Some(Tok::Num(n)) = self.peek().cloned()
                    && let Some((part, used)) = self.unit_at(self.pos + 1) && part.kind == unit.kind && part.factor < smallest
                {
                    value.n += unit.from_base(part.to_base(n));
                    smallest = part.factor;
                    self.pos += 1 + used;
                }
                value.unit = Some(unit);
                continue;
            }
            match self.peek() {
                Some(Tok::Op('%')) => {
                    // `10 % 3` is a remainder; `10%` is a hundredth.
                    let binary = matches!(self.toks.get(self.pos + 1), Some(Tok::Num(_)) | Some(Tok::LParen));
                    self.pos += 1;
                    if binary {
                        value.n %= self.unary()?.n;
                    } else {
                        value.n /= 100.0;
                    }
                }
                Some(Tok::Op('!')) => {
                    self.pos += 1;
                    if value.n < 0.0 || value.n.fract() != 0.0 || value.n > 170.0 || value.unit.is_some() {
                        return None;
                    }
                    value.n = (1..=value.n as u64).fold(1.0, |acc, n| acc * n as f64);
                }
                _ => return Some(value),
            }
        }
    }

    fn atom(&mut self) -> Option<Val> {
        match self.next()? {
            Tok::Num(n) => Some(Val::plain(n)),
            Tok::LParen => {
                let v = self.bits()?;
                (self.next()? == Tok::RParen).then_some(v)
            }
            Tok::Ident(name) => {
                if let Some(c) = constant(&name) {
                    return Some(Val::plain(c));
                }
                if !function(&name) {
                    // Money with its sign in front: `$20`, `usd 20`.
                    let unit = currency::find(&name)?;
                    let Some(Tok::Num(n)) = self.next() else { return None };
                    return Some(Val { n, unit: Some(unit) });
                }
                let args = if self.peek() == Some(&Tok::LParen) {
                    self.pos += 1;
                    let mut args = vec![self.bits()?];
                    while self.peek() == Some(&Tok::Comma) {
                        self.pos += 1;
                        args.push(self.bits()?);
                    }
                    if self.next()? != Tok::RParen {
                        return None;
                    }
                    args
                } else {
                    if self.word("of") {
                        self.pos += 1;
                    }
                    vec![self.unary()?]
                };
                apply(&name.to_lowercase(), &args)
            }
            _ => None,
        }
    }
}

fn constant(name: &str) -> Option<f64> {
    match name.to_lowercase().as_str() {
        "pi" | "π" => Some(std::f64::consts::PI),
        "e" => Some(std::f64::consts::E),
        "tau" | "τ" => Some(std::f64::consts::TAU),
        "phi" | "φ" => Some(1.618_033_988_749_895),
        _ => None,
    }
}

fn function(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "sqrt" | "cbrt" | "root" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "atan2" | "ln" | "log"
        | "log2" | "log10" | "abs" | "round" | "floor" | "ceil" | "trunc" | "sign" | "exp" | "min" | "max" | "sinh" | "cosh" | "tanh"
        | "sum" | "avg" | "mean" | "average" | "median" | "gcd" | "lcm" | "ncr" | "choose" | "npr" | "hypot" | "fact" | "factorial")
}

fn gcd(a: f64, b: f64) -> f64 {
    let (mut a, mut b) = (a.abs().round() as u64, b.abs().round() as u64);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a as f64
}

fn factorial(x: f64) -> Option<f64> {
    (x >= 0.0 && x.fract() == 0.0 && x <= 170.0).then(|| (1..=x as u64).fold(1.0, |acc, n| acc * n as f64))
}

fn apply(name: &str, args: &[Val]) -> Option<Val> {
    let first = args.first()?;
    let a: Vec<f64> = args.iter().map(|v| v.n).collect();
    let x = a[0];
    // Rounding keeps the unit; trigonometry reads an angle's; nothing else takes one.
    let keeps = matches!(name, "abs" | "round" | "floor" | "ceil" | "trunc" | "min" | "max" | "sum" | "avg" | "mean" | "average" | "median");
    let angle = matches!(name, "sin" | "cos" | "tan") && first.unit.as_ref().is_some_and(|u| u.kind == ANGLE);
    if !keeps && !angle && args.iter().any(|v| v.unit.is_some()) {
        return None;
    }
    if keeps && args.iter().any(|v| v.unit != first.unit) {
        return None;
    }
    let x = if angle { first.base() } else { x };
    let n = match name {
        "sqrt" => x.sqrt(),
        "cbrt" => x.cbrt(),
        "root" => x.powf(1.0 / a.get(1).copied().unwrap_or(2.0)),
        "sin" => x.sin(),
        "cos" => x.cos(),
        "tan" => x.tan(),
        "asin" => x.asin(),
        "acos" => x.acos(),
        "atan" => x.atan(),
        "atan2" => x.atan2(*a.get(1)?),
        "sinh" => x.sinh(),
        "cosh" => x.cosh(),
        "tanh" => x.tanh(),
        "ln" => x.ln(),
        "log" | "log10" => match a.get(1) {
            Some(base) => x.ln() / base.ln(),
            None => x.log10(),
        },
        "log2" => x.log2(),
        "abs" => x.abs(),
        "round" => match a.get(1) {
            Some(places) => {
                let scale = 10f64.powi(*places as i32);
                (x * scale).round() / scale
            }
            None => x.round(),
        },
        "floor" => x.floor(),
        "ceil" => x.ceil(),
        "trunc" => x.trunc(),
        "sign" => x.signum(),
        "exp" => x.exp(),
        "min" => a.iter().copied().fold(f64::INFINITY, f64::min),
        "max" => a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "sum" => a.iter().sum(),
        "avg" | "mean" | "average" => a.iter().sum::<f64>() / a.len() as f64,
        "median" => {
            let mut sorted = a.clone();
            sorted.sort_by(f64::total_cmp);
            let mid = sorted.len() / 2;
            if sorted.len() % 2 == 0 { (sorted[mid - 1] + sorted[mid]) / 2.0 } else { sorted[mid] }
        }
        "gcd" => a.iter().copied().reduce(gcd)?,
        "lcm" => a.iter().copied().reduce(|x, y| if x == 0.0 || y == 0.0 { 0.0 } else { (x * y).abs() / gcd(x, y) })?,
        "ncr" | "choose" => {
            let k = *a.get(1)?;
            factorial(x)? / (factorial(k)? * factorial(x - k)?)
        }
        "npr" => factorial(x)? / factorial(x - *a.get(1)?)?,
        "hypot" => a.iter().map(|v| v * v).sum::<f64>().sqrt(),
        "fact" | "factorial" => factorial(x)?,
        _ => return None,
    };
    // A rounded sine of a half turn is nothing, not 1.2e-16.
    let n = if angle && n.abs() < 1e-15 { 0.0 } else { n };
    Some(Val { n, unit: if keeps { first.unit.clone() } else { None } })
}

/// Reads a whole expression, with ambiguous unit names leaning toward `prefer`.
fn eval(src: &str, prefer: Option<u8>) -> Option<Val> {
    let toks = lex(src, prefer == Some(TEMP))?;
    if toks.is_empty() {
        return None;
    }
    let mut p = Parser { toks, pos: 0, prefer };
    let v = p.bits()?;
    (p.pos == p.toks.len() && v.n.is_finite()).then_some(v)
}

fn group(digits: &str) -> String {
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    grouped
}

/// A number with a set count of decimals and grouped thousands.
fn fixed(v: f64, decimals: usize) -> String {
    let text = format!("{v:.decimals$}");
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    let (sign, digits) = int.strip_prefix('-').map_or(("", int), |d| ("-", d));
    if frac.is_empty() { format!("{sign}{}", group(digits)) } else { format!("{sign}{}.{frac}", group(digits)) }
}

/// A number rounded to so many significant digits.
fn format_sig(v: f64, digits: i32) -> String {
    if v == 0.0 || !v.is_finite() {
        return format_number(v);
    }
    let scale = 10f64.powi(digits - 1 - v.abs().log10().floor() as i32);
    format_number((v * scale).round() / scale)
}

/// A number as people write one: grouped thousands, up to ten significant decimals.
pub fn format_number(v: f64) -> String {
    // Twelve significant digits: enough to be exact, few enough to hide float noise.
    let v = if v == 0.0 { 0.0 } else {
        let scale = 10f64.powi(11 - v.abs().log10().floor() as i32);
        (v * scale).round() / scale
    };
    if v == 0.0 || v.abs() < 1e-11 {
        return "0".into();
    }
    let abs = v.abs();
    if !(1e-9..1e15).contains(&abs) {
        return format!("{v:e}");
    }
    let decimals = if abs >= 1e9 { 2 } else if abs >= 1.0 { (10 - abs.log10().floor() as i32 - 1).clamp(0, 10) as usize } else { 10 };
    let fixed = format!("{v:.decimals$}");
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let frac = frac.trim_end_matches('0');
    let (sign, digits) = int.strip_prefix('-').map_or(("", int), |d| ("-", d));
    let grouped = group(digits);
    if frac.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{frac}") }
}

fn show(v: &Val) -> String {
    match &v.unit {
        None => format_number(v.n),
        Some(unit) if unit.kind == CURRENCY => currency::format(v.n, unit),
        Some(unit) => {
            let symbol = unit.symbol_for(v.n);
            format!("{}{}{symbol}", format_number(v.n), if symbol.starts_with('°') { "" } else { " " })
        }
    }
}

fn card(left: impl Into<String>, left_badge: &str, right: impl Into<String>, right_badge: &str) -> Card {
    Card { left: left.into(), left_badge: left_badge.into(), right: right.into(), right_badge: right_badge.into() }
}

/// The share one value is of another, both read in the same unit when they have one.
fn ratio(a: &str, b: &str) -> Option<(f64, f64)> {
    let (a, b) = (eval(a.trim(), None)?, eval(b.trim(), None)?);
    match (&a.unit, &b.unit) {
        (Some(x), Some(y)) if x.kind == y.kind => Some((a.base(), b.base())),
        (None, None) => Some((a.n, b.n)),
        _ => None,
    }
}

/// `15 as % of 60`, `what % of 80 is 20`, `3 out of 12`, `% change from 50 to 75`.
fn percentages(q: &str) -> Option<Card> {
    let lower = q.to_ascii_lowercase();
    let share = |a: &str, b: &str| {
        let (a, b) = ratio(a, b)?;
        (b != 0.0).then(|| card(q, "Share", format!("{}%", format_number(a / b * 100.0)), "Percentage"))
    };
    for sep in [" as a % of ", " as % of ", " is what % of ", " in % of ", " out of "] {
        if let Some(at) = lower.find(sep) {
            return share(&q[..at], &q[at + sep.len()..]);
        }
    }
    if let Some(rest) = lower.strip_prefix("what % of ").map(|r| &q[q.len() - r.len()..]) && let Some((whole, part)) = rest.split_once(" is ") {
        return share(part, whole);
    }
    // A change from one value to another.
    let change = |from: &str, to: &str| {
        let (a, b) = ratio(from, to)?;
        let pct = (b - a) / a * 100.0;
        (a != 0.0 && pct.is_finite()).then(|| {
            let text = format!("{}{}%", if pct > 0.0 { "+" } else { "" }, format_number(pct));
            card(q, "Change", text, if pct > 0.0 { "Increase" } else if pct < 0.0 { "Decrease" } else { "No Change" })
        })
    };
    for lead in ["% change from ", "% increase from ", "% decrease from ", "% difference from ", "% difference between ", "% change ", "% from "] {
        if let Some(rest) = lower.strip_prefix(lead).map(|r| &q[q.len() - r.len()..]) {
            let (from, to) = rest.split_once(" to ").or_else(|| rest.split_once(" and "))?;
            return change(from, to);
        }
    }
    for tail in [" as %", " in %", " as a %", " % change"] {
        if let Some(head) = lower.strip_suffix(tail).map(|h| &q[..h.len()]) && let Some((from, to)) = head.split_once(" to ") {
            return change(from, to);
        }
    }
    None
}

fn in_base(rhs: &str) -> Option<(u32, &'static str)> {
    Some(match rhs.to_ascii_lowercase().as_str() {
        "hex" | "hexadecimal" | "base 16" | "base16" => (16, "Hexadecimal"),
        "bin" | "binary" | "base 2" | "base2" => (2, "Binary"),
        "oct" | "octal" | "base 8" | "base8" => (8, "Octal"),
        "dec" | "decimal" | "base 10" | "base10" => (10, "Decimal"),
        _ => return None,
    })
}

/// One side of a conversion against the other: a number base, a percentage, or a unit.
fn convert_to(lhs: &str, rhs: &str) -> Option<Card> {
    if let Some((radix, label)) = in_base(rhs) {
        let v = eval(lhs, None).filter(|v| v.unit.is_none() && v.n.fract() == 0.0 && v.n.abs() < 9e15)?;
        let (sign, n) = (if v.n < 0.0 { "-" } else { "" }, v.n.abs() as u64);
        let text = match radix {
            16 => format!("{sign}0x{n:X}"),
            2 => format!("{sign}0b{n:b}"),
            8 => format!("{sign}0o{n:o}"),
            _ => format!("{sign}{n}"),
        };
        return Some(card(lhs, "Number", text, label));
    }
    if rhs == "%" {
        let v = eval(lhs, None).filter(|v| v.unit.is_none())?;
        return Some(card(lhs, "Number", format!("{}%", format_number(v.n * 100.0)), "Percentage"));
    }
    let mut targets = units::find_all(rhs);
    targets.extend(currency::find(rhs));
    targets.into_iter().find_map(|to| {
        // A unit alone stands for one of it: `km to mi`.
        let value = eval(lhs, Some(to.kind)).or_else(|| eval(&format!("1 {lhs}"), Some(to.kind)))?;
        let from = value.unit.clone().filter(|from| from.kind == to.kind)?;
        let result = Val { n: to.from_base(from.to_base(value.n)), unit: Some(to.clone()) };
        // A mixed amount reads best as it was typed: `5 ft 11 in`, not 5.9167 ft.
        let mixed = lex(lhs, false).is_some_and(|toks| toks.iter().filter(|t| matches!(t, Tok::Num(_))).count() > 1);
        let left = if mixed { lhs.to_string() } else { show(&value) };
        result.n.is_finite().then(|| card(left, &from.label, show(&result), &to.label))
    })
}

/// `500 km to mi`, `72 f in c`, `$100 in eur`, `255 to hex`.
fn convert(q: &str) -> Option<Card> {
    let lower = q.to_ascii_lowercase();
    [" to ", " in ", " as ", " into ", " -> ", " → "].iter().find_map(|sep| {
        let at = lower.rfind(sep)?;
        convert_to(q[..at].trim(), q[at + sep.len()..].trim())
    })
}

/// `how many ml in 2 cups`, `how many inches in a foot`.
fn how_many(q: &str) -> Option<Card> {
    let lower = q.to_ascii_lowercase();
    let rest = &q[q.len() - lower.strip_prefix("how many ")?.len()..];
    let at = rest.to_ascii_lowercase().find(" in ")?;
    let (unit, amount) = (rest[..at].trim(), rest[at + 4..].trim());
    let amount = ["a ", "an ", "one "].iter().find_map(|lead| amount.strip_prefix(lead)).unwrap_or(amount);
    convert_to(amount, unit)
}

/// The number in an answer, bare: no unit, sign of money or grouping.
pub fn plain(answer: &str) -> String {
    let Some(start) = answer.find(|c: char| c.is_ascii_digit()) else { return answer.to_string() };
    let negative = answer[..start].contains('-');
    let run: String = answer[start..].chars().take_while(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | 'e' | '-' | '+')).filter(|c| *c != ',').collect();
    // Dates and times are not numbers to strip.
    if answer.contains(':') || answer[..start].chars().any(|c| c.is_alphabetic() && !currency::is_symbol(c) && answer[..start].len() > 4) {
        return answer.to_string();
    }
    format!("{}{}", if negative { "-" } else { "" }, run.trim_end_matches(['e', '-', '+', '.']))
}

/// The card for a query, when it is something to compute. A bare number is not.
pub fn evaluate(query: &str) -> Option<Card> {
    let q = query.trim().trim_end_matches(['=', '?']).trim();
    let digits = q.chars().any(|c| c.is_ascii_digit());
    // One word alone is a search, not a sum.
    if q.is_empty() || q.len() > 240 || (!digits && !q.contains(|c: char| c.is_whitespace() || c == '(')) {
        return None;
    }
    if let Some(card) = dates::evaluate(q) {
        return Some(card);
    }
    if let Some(card) = how_many(q) {
        return Some(card);
    }
    let q = normalize(q);
    let q = q.as_str();
    if let Some(card) = percentages(q).or_else(|| convert(q)) {
        return Some(card);
    }
    let toks = lex(q, false)?;
    if toks.len() < 2 {
        return None;
    }
    let value = eval(q, None)?;
    let Some(unit) = &value.unit else { return Some(card(query.trim(), "Expression", show(&value), "Result")) };
    // A measure alone says nothing new, except money, which is shown in the currency at home.
    let worked = toks.iter().filter(|t| matches!(t, Tok::Num(_))).count() > 1
        || toks.iter().any(|t| matches!(t, Tok::Op(_) | Tok::LParen) || matches!(t, Tok::Ident(name) if joining(name) || function(name) || scale(name).is_some()));
    if unit.kind == CURRENCY && !worked {
        if toks.iter().any(|t| matches!(t, Tok::Ident(name) if currency::wordy(name))) {
            return None;
        }
        let to = currency::counterpart(unit)?;
        let result = Val { n: to.from_base(unit.to_base(value.n)), unit: Some(to.clone()) };
        return Some(card(show(&value), &unit.label, show(&result), &to.label));
    }
    worked.then(|| card(query.trim(), "Expression", show(&value), &unit.label))
}

/// `echospot calc <expression>`: the answer on stdout, waiting for exchange rates if it must.
pub fn cli(query: &str) {
    let mut answer = evaluate(query);
    for _ in 0..40 {
        if answer.is_some() || !currency::fetching() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        answer = evaluate(query);
    }
    match answer {
        Some(card) => println!("{} [{}]  →  {} [{}]", card.left, card.left_badge, card.right, card.right_badge),
        None => std::process::exit(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(q: &str) -> String {
        currency::set_test_rates();
        evaluate(q).map(|c| c.right).unwrap_or_default()
    }

    #[test]
    fn arithmetic() {
        assert_eq!(r("1 + 2 * 3"), "7");
        assert_eq!(r("(1 + 2) * 3"), "9");
        assert_eq!(r("2^10"), "1,024");
        assert_eq!(r("2**10"), "1,024");
        assert_eq!(r("10 / 4"), "2.5");
        assert_eq!(r("1,000 + 1"), "1,001");
        assert_eq!(r("-3 + 5"), "2");
        assert_eq!(r("sqrt(16) + 1"), "5");
        assert_eq!(r("5!"), "120");
        assert_eq!(r("2x3"), "6");
        assert_eq!(r("5k + 2k"), "7,000");
        assert_eq!(r("2 million / 4"), "500,000");
        assert_eq!(r("3 lakh + 2 crore"), "20,300,000");
        assert_eq!(r("what is 6 times 7?"), "42");
    }

    #[test]
    fn words() {
        assert_eq!(r("10 divided by 4"), "2.5");
        assert_eq!(r("3 multiplied by 4"), "12");
        assert_eq!(r("5 squared"), "25");
        assert_eq!(r("2 cubed + 1"), "9");
        assert_eq!(r("half of 90"), "45");
        assert_eq!(r("square root of 81"), "9");
        assert_eq!(r("2 to the power of 8"), "256");
        assert_eq!(r("double 21"), "42");
    }

    #[test]
    fn functions() {
        assert_eq!(r("sin(90 deg)"), "1");
        assert_eq!(r("cos 180°"), "-1");
        assert_eq!(r("sin(pi)"), "0");
        assert_eq!(r("log(1000)"), "3");
        assert_eq!(r("log2 1024"), "10");
        assert_eq!(r("max(3, 9, 4)"), "9");
        assert_eq!(r("avg(2, 4, 9)"), "5");
        assert_eq!(r("gcd(12, 18)"), "6");
        assert_eq!(r("lcm(4, 6)"), "12");
        assert_eq!(r("ncr(5, 2)"), "10");
        assert_eq!(r("round(3.14159, 2)"), "3.14");
        assert_eq!(r("hypot(3, 4)"), "5");
    }

    #[test]
    fn percentages() {
        assert_eq!(r("50% of 200"), "100");
        assert_eq!(r("200 + 10%"), "220");
        assert_eq!(r("200 - 25%"), "150");
        assert_eq!(r("10 % 3"), "1");
        assert_eq!(r("20% off 80"), "64");
        assert_eq!(r("15 percent of 60"), "9");
        assert_eq!(r("15 as % of 60"), "25%");
        assert_eq!(r("20 is what percent of 80"), "25%");
        assert_eq!(r("what % of 80 is 20"), "25%");
        assert_eq!(r("3 out of 12"), "25%");
        assert_eq!(r("% change from 50 to 75"), "+50%");
        assert_eq!(r("80 to 60 as %"), "-25%");
        assert_eq!(r("0.25 as %"), "25%");
    }

    #[test]
    fn bases() {
        assert_eq!(r("0xff + 1"), "256");
        assert_eq!(r("0b1010 * 2"), "20");
        assert_eq!(r("255 to hex"), "0xFF");
        assert_eq!(r("10 in binary"), "0b1010");
        assert_eq!(r("0x1F to decimal"), "31");
        assert_eq!(r("8 to octal"), "0o10");
        assert_eq!(r("1 << 10"), "1,024");
        assert_eq!(r("0xf0 | 0x0f"), "255");
        assert_eq!(r("6 xor 3"), "5");
    }

    #[test]
    fn conversions() {
        assert_eq!(r("500 km to mi"), "310.6855961 mi");
        assert_eq!(r("32 f to c"), "0°C");
        assert_eq!(r("300k to c"), "26.85°C");
        assert_eq!(r("1 gb in mb"), "1,000 MB");
        assert_eq!(r("2 hours in minutes"), "120 min");
        assert_eq!(r("5 km to kg"), "");
        assert_eq!(r("5 ft 3 in to cm"), "160.02 cm");
        assert_eq!(r("5'3\" in cm"), "160.02 cm");
        assert_eq!(r("1h 30min in minutes"), "90 min");
        assert_eq!(r("100 km/h to mph"), "62.13711922 mph");
        assert_eq!(r("60 mph in km/h"), "96.56064 km/h");
        assert_eq!(r("100 sq ft to square meters"), "9.290304 m²");
        assert_eq!(r("1 mile to feet"), "5,280 ft");
        assert_eq!(r("km to m"), "1,000 m");
        assert_eq!(r("8 oz to ml"), "236.5882368 mL");
        assert_eq!(r("8 oz to g"), "226.796185 g");
        assert_eq!(r("100 Mb/s to MB/s"), "12.5 MB/s");
        assert_eq!(r("30 mpg to l/100km"), "7.840486119 L/100km");
        assert_eq!(r("16 px to pt"), "12 pt");
        assert_eq!(r("how many ml in 2 cups"), "473.176473 mL");
        assert_eq!(r("how many inches in a foot"), "12 in");
        assert_eq!(r("1 week in days"), "7 days");
        assert_eq!(r("3 weeks in days"), "21 days");
        assert_eq!(r("7 days in weeks"), "1 week");
        assert_eq!(r("2 kwh to j"), "7,200,000 J");
    }

    #[test]
    fn unit_arithmetic() {
        assert_eq!(r("5 km + 300 m"), "5.3 km");
        assert_eq!(r("2 h - 45 min"), "1.25 h");
        assert_eq!(r("3 * 4 kg"), "12 kg");
        assert_eq!(r("10 km / 2"), "5 km");
        assert_eq!(r("10 km / 500 m"), "20");
        assert_eq!(r("100 km / 2 h"), "50 km/h");
        assert_eq!(r("60 mph * 30 min"), "30 mi");
        assert_eq!(r("3 m * 4 m"), "12 m²");
        assert_eq!(r("(1 kg + 500 g) to lb"), "3.306933933 lb");
        assert_eq!(r("5 km + 2 kg"), "");
    }

    #[test]
    fn money() {
        assert_eq!(r("100 usd to eur"), "€80.00");
        assert_eq!(r("$100 in inr"), "₹8,000.00");
        assert_eq!(r("50 euros in dollars"), "$62.50");
        assert_eq!(r("100 pounds to usd"), "$200.00");
        assert_eq!(r("100 pounds to kg"), "45.359237 kg");
        assert_eq!(r("1 btc to usd"), "$100,000.00");
        assert_eq!(r("$20 + 15%"), "$23.00");
        assert_eq!(r("$20 + €10"), "$32.50");
        assert_eq!(r("15% of $80"), "$12.00");
        assert_eq!(r("10% off $200"), "$180.00");
        assert_eq!(r("($20 + $30) to eur"), "€40.00");
        assert_eq!(r("20 eur"), "$25.00");
        assert_eq!(r("€20"), "$25.00");
        assert_eq!(r("20 usd"), "€16.00");
        assert_eq!(r("2 million yen to usd"), "$13,333.33");
        assert_eq!(r("5k inr to usd"), "$62.50");
        assert_eq!(r("100 us dollars to british pounds"), "£50.00");
        assert_eq!(r("100 cad to usd"), "$80.00");
        assert_eq!(r("C$100 to usd"), "$80.00");
    }

    #[test]
    fn not_a_sum() {
        for q in ["42", "firefox", "3d printer", "5 km", "7 days", "4k video", "8 gb ram", "windows 11", "sin city", "1080p", "5 try", "top 10", "pi"] {
            assert!(evaluate(q).is_none(), "{q}");
        }
    }

    #[test]
    fn plain_numbers() {
        assert_eq!(plain("$1,234.50"), "1234.50");
        assert_eq!(plain("310.6855961 mi"), "310.6855961");
        assert_eq!(plain("-25%"), "-25");
        assert_eq!(plain("Fri, 25 Dec 2026"), "Fri, 25 Dec 2026");
        assert_eq!(plain("2:00 AM"), "2:00 AM");
        assert_eq!(plain("43 days"), "43");
    }
}
