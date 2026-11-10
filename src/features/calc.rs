//! The inline calculator: arithmetic, percentages, functions and unit conversion.

use crate::model::Card;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    Comma,
}

fn lex(src: &str) -> Option<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let (mut i, mut out) = (0, Vec::new());
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit())) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_'
                || (chars[i] == ',' && chars[i + 1..].iter().take_while(|d| d.is_ascii_digit()).count() == 3)) {
                i += 1;
            }
            // Scientific notation.
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E')
                && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit() || ((*n == '-' || *n == '+') && chars.get(i + 2).is_some_and(|d| d.is_ascii_digit()))) {
                i += 2;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = chars[start..i].iter().filter(|c| **c != ',' && **c != '_').collect();
            out.push(Tok::Num(text.parse().ok()?));
        } else if c.is_alphabetic() || c == '°' || c == 'µ' || c == '"' || c == '\'' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || matches!(chars[i], '°' | 'µ' | '"' | '\'' | '²' | '³')) {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect::<String>()));
        } else {
            out.push(match c {
                '+' | '-' | '*' | '/' | '^' | '%' | '!' => Tok::Op(c),
                '×' | '·' => Tok::Op('*'),
                '÷' => Tok::Op('/'),
                '−' => Tok::Op('-'),
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' => Tok::Comma,
                _ => return None,
            });
            i += 1;
        }
    }
    Some(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
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

    fn expr(&mut self) -> Option<f64> {
        let mut left = self.term()?;
        loop {
            match self.peek() {
                Some(Tok::Op('+')) => {
                    self.pos += 1;
                    let (right, pct) = self.term_pct()?;
                    left += if pct { left * right } else { right };
                }
                Some(Tok::Op('-')) => {
                    self.pos += 1;
                    let (right, pct) = self.term_pct()?;
                    left -= if pct { left * right } else { right };
                }
                _ if self.word("plus") => {
                    self.pos += 1;
                    left += self.term()?;
                }
                _ if self.word("minus") => {
                    self.pos += 1;
                    left -= self.term()?;
                }
                _ => return Some(left),
            }
        }
    }

    /// A term, and whether it was a bare percentage (`200 + 10%` adds ten percent of 200).
    fn term_pct(&mut self) -> Option<(f64, bool)> {
        let start = self.pos;
        let value = self.term()?;
        let bare = self.pos == start + 2 && matches!(self.toks.get(start), Some(Tok::Num(_))) && self.toks.get(start + 1) == Some(&Tok::Op('%'));
        Some((value, bare))
    }

    fn term(&mut self) -> Option<f64> {
        let mut left = self.unary()?;
        loop {
            match self.peek() {
                Some(Tok::Op('*')) => {
                    self.pos += 1;
                    left *= self.unary()?;
                }
                Some(Tok::Op('/')) => {
                    self.pos += 1;
                    left /= self.unary()?;
                }
                _ if self.word("x") || self.word("times") => {
                    self.pos += 1;
                    left *= self.unary()?;
                }
                _ if self.word("mod") => {
                    self.pos += 1;
                    left %= self.unary()?;
                }
                _ if self.word("of") => {
                    self.pos += 1;
                    left *= self.unary()?;
                }
                // Implicit multiplication: `2(3)`, `2 pi`.
                Some(Tok::LParen) => left *= self.unary()?,
                Some(Tok::Ident(name)) if constant(name).is_some() || function(name) => left *= self.unary()?,
                _ => return Some(left),
            }
        }
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                Some(-self.unary()?)
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.postfix()?;
        if self.peek() == Some(&Tok::Op('^')) {
            self.pos += 1;
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    fn postfix(&mut self) -> Option<f64> {
        let mut value = self.atom()?;
        loop {
            match self.peek() {
                Some(Tok::Op('%')) => {
                    // `10 % 3` is a remainder; `10%` is a hundredth.
                    let binary = matches!(self.toks.get(self.pos + 1), Some(Tok::Num(_)) | Some(Tok::LParen));
                    self.pos += 1;
                    if binary {
                        value %= self.unary()?;
                    } else {
                        value /= 100.0;
                    }
                }
                Some(Tok::Op('!')) => {
                    self.pos += 1;
                    if value < 0.0 || value.fract() != 0.0 || value > 170.0 {
                        return None;
                    }
                    value = (1..=value as u64).fold(1.0, |acc, n| acc * n as f64);
                }
                _ => return Some(value),
            }
        }
    }

    fn atom(&mut self) -> Option<f64> {
        match self.next()? {
            Tok::Num(n) => Some(n),
            Tok::LParen => {
                let v = self.expr()?;
                (self.next()? == Tok::RParen).then_some(v)
            }
            Tok::Ident(name) => {
                if let Some(c) = constant(&name) {
                    return Some(c);
                }
                if !function(&name) {
                    return None;
                }
                let args = if self.peek() == Some(&Tok::LParen) {
                    self.pos += 1;
                    let mut args = vec![self.expr()?];
                    while self.peek() == Some(&Tok::Comma) {
                        self.pos += 1;
                        args.push(self.expr()?);
                    }
                    if self.next()? != Tok::RParen {
                        return None;
                    }
                    args
                } else {
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
        "phi" => Some(1.618_033_988_749_895),
        _ => None,
    }
}

fn function(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "sqrt" | "cbrt" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "ln" | "log"
        | "log2" | "log10" | "abs" | "round" | "floor" | "ceil" | "exp" | "min" | "max" | "sinh" | "cosh" | "tanh")
}

fn apply(name: &str, a: &[f64]) -> Option<f64> {
    let x = *a.first()?;
    Some(match name {
        "sqrt" => x.sqrt(),
        "cbrt" => x.cbrt(),
        "sin" => x.sin(),
        "cos" => x.cos(),
        "tan" => x.tan(),
        "asin" => x.asin(),
        "acos" => x.acos(),
        "atan" => x.atan(),
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
        "round" => x.round(),
        "floor" => x.floor(),
        "ceil" => x.ceil(),
        "exp" => x.exp(),
        "min" => a.iter().copied().fold(f64::INFINITY, f64::min),
        "max" => a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        _ => return None,
    })
}

fn eval(src: &str) -> Option<f64> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return None;
    }
    let mut p = Parser { toks, pos: 0 };
    let v = p.expr()?;
    (p.pos == p.toks.len() && v.is_finite()).then_some(v)
}

/// A number as people write one: grouped thousands, up to ten significant decimals.
pub fn format_number(v: f64) -> String {
    if v == 0.0 {
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
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if frac.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{frac}") }
}

struct Unit {
    names: &'static [&'static str],
    label: &'static str,
    symbol: &'static str,
    kind: u8,
    /// Multiply by this to reach the kind's base unit.
    factor: f64,
    offset: f64,
}

const fn u(names: &'static [&'static str], label: &'static str, symbol: &'static str, kind: u8, factor: f64) -> Unit {
    Unit { names, label, symbol, kind, factor, offset: 0.0 }
}

const LENGTH: u8 = 0;
const MASS: u8 = 1;
const TEMP: u8 = 2;
const VOLUME: u8 = 3;
const AREA: u8 = 4;
const SPEED: u8 = 5;
const TIME: u8 = 6;
const DATA: u8 = 7;
const ENERGY: u8 = 8;
const PRESSURE: u8 = 9;
const ANGLE: u8 = 10;
const POWER: u8 = 11;

static UNITS: &[Unit] = &[
    u(&["mm", "millimeter", "millimeters", "millimetre", "millimetres"], "Millimeters", "mm", LENGTH, 0.001),
    u(&["cm", "centimeter", "centimeters", "centimetre", "centimetres"], "Centimeters", "cm", LENGTH, 0.01),
    u(&["m", "meter", "meters", "metre", "metres"], "Meters", "m", LENGTH, 1.0),
    u(&["km", "kilometer", "kilometers", "kilometre", "kilometres"], "Kilometers", "km", LENGTH, 1000.0),
    u(&["in", "inch", "inches", "\""], "Inches", "in", LENGTH, 0.0254),
    u(&["ft", "foot", "feet", "'"], "Feet", "ft", LENGTH, 0.3048),
    u(&["yd", "yard", "yards"], "Yards", "yd", LENGTH, 0.9144),
    u(&["mi", "mile", "miles"], "Miles", "mi", LENGTH, 1609.344),
    u(&["nmi", "nauticalmile", "nauticalmiles"], "Nautical Miles", "nmi", LENGTH, 1852.0),
    u(&["mg", "milligram", "milligrams"], "Milligrams", "mg", MASS, 1e-6),
    u(&["g", "gram", "grams"], "Grams", "g", MASS, 0.001),
    u(&["kg", "kilogram", "kilograms", "kilo", "kilos"], "Kilograms", "kg", MASS, 1.0),
    u(&["t", "tonne", "tonnes", "ton", "tons"], "Tonnes", "t", MASS, 1000.0),
    u(&["oz", "ounce", "ounces"], "Ounces", "oz", MASS, 0.028_349_523_125),
    u(&["lb", "lbs", "pound", "pounds"], "Pounds", "lb", MASS, 0.453_592_37),
    u(&["st", "stone", "stones"], "Stone", "st", MASS, 6.350_293_18),
    Unit { names: &["c", "°c", "celsius", "centigrade"], label: "Celsius", symbol: "°C", kind: TEMP, factor: 1.0, offset: 273.15 },
    Unit { names: &["f", "°f", "fahrenheit"], label: "Fahrenheit", symbol: "°F", kind: TEMP, factor: 5.0 / 9.0, offset: 459.67 },
    Unit { names: &["k", "kelvin"], label: "Kelvin", symbol: "K", kind: TEMP, factor: 1.0, offset: 0.0 },
    u(&["ml", "milliliter", "milliliters", "millilitre", "millilitres"], "Milliliters", "mL", VOLUME, 0.001),
    u(&["l", "liter", "liters", "litre", "litres"], "Liters", "L", VOLUME, 1.0),
    u(&["tsp", "teaspoon", "teaspoons"], "Teaspoons", "tsp", VOLUME, 0.004_928_921_59),
    u(&["tbsp", "tablespoon", "tablespoons"], "Tablespoons", "tbsp", VOLUME, 0.014_786_764_8),
    u(&["floz", "fluidounce", "fluidounces"], "Fluid Ounces", "fl oz", VOLUME, 0.029_573_529_6),
    u(&["cup", "cups"], "Cups", "cups", VOLUME, 0.236_588_236_5),
    u(&["pt", "pint", "pints"], "Pints", "pt", VOLUME, 0.473_176_473),
    u(&["qt", "quart", "quarts"], "Quarts", "qt", VOLUME, 0.946_352_946),
    u(&["gal", "gallon", "gallons"], "Gallons", "gal", VOLUME, 3.785_411_784),
    u(&["m3", "m³", "cubicmeter", "cubicmeters"], "Cubic Meters", "m³", VOLUME, 1000.0),
    u(&["sqm", "m2", "m²", "squaremeter", "squaremeters"], "Square Meters", "m²", AREA, 1.0),
    u(&["sqkm", "km2", "km²"], "Square Kilometers", "km²", AREA, 1e6),
    u(&["sqft", "ft2", "ft²", "squarefeet", "squarefoot"], "Square Feet", "ft²", AREA, 0.092_903_04),
    u(&["sqmi", "mi2", "mi²"], "Square Miles", "mi²", AREA, 2_589_988.110_336),
    u(&["acre", "acres"], "Acres", "acres", AREA, 4046.856_422_4),
    u(&["ha", "hectare", "hectares"], "Hectares", "ha", AREA, 10_000.0),
    u(&["mps", "m/s"], "Meters per Second", "m/s", SPEED, 1.0),
    u(&["kph", "kmh", "km/h"], "Kilometers per Hour", "km/h", SPEED, 1.0 / 3.6),
    u(&["mph"], "Miles per Hour", "mph", SPEED, 0.447_04),
    u(&["knot", "knots", "kn"], "Knots", "kn", SPEED, 0.514_444_444),
    u(&["ms", "millisecond", "milliseconds"], "Milliseconds", "ms", TIME, 0.001),
    u(&["s", "sec", "secs", "second", "seconds"], "Seconds", "s", TIME, 1.0),
    u(&["min", "mins", "minute", "minutes"], "Minutes", "min", TIME, 60.0),
    u(&["h", "hr", "hrs", "hour", "hours"], "Hours", "h", TIME, 3600.0),
    u(&["d", "day", "days"], "Days", "days", TIME, 86_400.0),
    u(&["wk", "week", "weeks"], "Weeks", "weeks", TIME, 604_800.0),
    u(&["mo", "month", "months"], "Months", "months", TIME, 2_629_746.0),
    u(&["yr", "year", "years"], "Years", "years", TIME, 31_556_952.0),
    u(&["bit", "bits"], "Bits", "bit", DATA, 0.125),
    u(&["b", "byte", "bytes"], "Bytes", "B", DATA, 1.0),
    u(&["kb", "kilobyte", "kilobytes"], "Kilobytes", "kB", DATA, 1e3),
    u(&["mb", "megabyte", "megabytes"], "Megabytes", "MB", DATA, 1e6),
    u(&["gb", "gigabyte", "gigabytes"], "Gigabytes", "GB", DATA, 1e9),
    u(&["tb", "terabyte", "terabytes"], "Terabytes", "TB", DATA, 1e12),
    u(&["kib", "kibibyte", "kibibytes"], "Kibibytes", "KiB", DATA, 1024.0),
    u(&["mib", "mebibyte", "mebibytes"], "Mebibytes", "MiB", DATA, 1_048_576.0),
    u(&["gib", "gibibyte", "gibibytes"], "Gibibytes", "GiB", DATA, 1_073_741_824.0),
    u(&["tib", "tebibyte", "tebibytes"], "Tebibytes", "TiB", DATA, 1_099_511_627_776.0),
    u(&["j", "joule", "joules"], "Joules", "J", ENERGY, 1.0),
    u(&["kj", "kilojoule", "kilojoules"], "Kilojoules", "kJ", ENERGY, 1000.0),
    u(&["cal", "calorie", "calories"], "Calories", "cal", ENERGY, 4.184),
    u(&["kcal", "kilocalorie", "kilocalories"], "Kilocalories", "kcal", ENERGY, 4184.0),
    u(&["wh", "watthour", "watthours"], "Watt Hours", "Wh", ENERGY, 3600.0),
    u(&["kwh", "kilowatthour", "kilowatthours"], "Kilowatt Hours", "kWh", ENERGY, 3.6e6),
    u(&["pa", "pascal", "pascals"], "Pascals", "Pa", PRESSURE, 1.0),
    u(&["kpa", "kilopascal", "kilopascals"], "Kilopascals", "kPa", PRESSURE, 1000.0),
    u(&["bar", "bars"], "Bar", "bar", PRESSURE, 100_000.0),
    u(&["psi"], "PSI", "psi", PRESSURE, 6894.757_293),
    u(&["atm", "atmosphere", "atmospheres"], "Atmospheres", "atm", PRESSURE, 101_325.0),
    u(&["deg", "degree", "degrees", "°"], "Degrees", "°", ANGLE, std::f64::consts::PI / 180.0),
    u(&["rad", "radian", "radians"], "Radians", "rad", ANGLE, 1.0),
    u(&["w", "watt", "watts"], "Watts", "W", POWER, 1.0),
    u(&["kw", "kilowatt", "kilowatts"], "Kilowatts", "kW", POWER, 1000.0),
    u(&["hp", "horsepower"], "Horsepower", "hp", POWER, 745.699_872),
];

fn unit(name: &str) -> Option<&'static Unit> {
    let key = name.trim().to_lowercase().replace(' ', "");
    // Case tells megabytes from millibits only where both exist; lower-case names win otherwise.
    UNITS.iter().find(|u| u.names.contains(&key.as_str()))
}

/// `500 km to mi`, `72 f in c`, `1.5 hours as minutes`.
fn convert(query: &str) -> Option<Card> {
    let lower = query.to_lowercase();
    let (lhs, rhs) = [" to ", " in ", " as ", " -> ", " → "].iter().find_map(|sep| {
        let at = lower.rfind(sep)?;
        Some((query[..at].trim(), query[at + sep.len()..].trim()))
    })?;
    let to = unit(rhs)?;
    let split = lhs.char_indices().rev().find(|(_, c)| c.is_ascii_digit() || *c == ')').map(|(i, c)| i + c.len_utf8())?;
    let (expr, from_name) = (lhs[..split].trim(), lhs[split..].trim());
    let from = unit(from_name)?;
    if from.kind != to.kind {
        return None;
    }
    let value = eval(expr)?;
    let base = (value + from.offset) * from.factor;
    let result = base / to.factor - to.offset;
    let glue = |sym: &str| if sym.starts_with('°') { "" } else { " " };
    Some(Card {
        left: format!("{}{}{}", format_number(value), glue(from.symbol), from.symbol),
        left_badge: from.label.into(),
        right: format!("{}{}{}", format_number(result), glue(to.symbol), to.symbol),
        right_badge: to.label.into(),
    })
}

/// The card for a query, when it is something to compute. A bare number is not.
pub fn evaluate(query: &str) -> Option<Card> {
    let q = query.trim().trim_end_matches('=').trim();
    if q.is_empty() || !q.chars().any(|c| c.is_ascii_digit() || matches!(c, 'π')) && constant(q).is_none() {
        return None;
    }
    if let Some(card) = convert(q) {
        return Some(card);
    }
    let toks = lex(q)?;
    // One number alone, or one word alone, is a search and not a sum.
    if toks.len() < 2 {
        return None;
    }
    let value = eval(q)?;
    Some(Card { left: q.to_string(), left_badge: "Expression".into(), right: format_number(value), right_badge: "Result".into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(q: &str) -> String {
        evaluate(q).map(|c| c.right).unwrap_or_default()
    }

    #[test]
    fn arithmetic() {
        assert_eq!(r("1 + 2 * 3"), "7");
        assert_eq!(r("(1 + 2) * 3"), "9");
        assert_eq!(r("2^10"), "1,024");
        assert_eq!(r("10 / 4"), "2.5");
        assert_eq!(r("1,000 + 1"), "1,001");
        assert_eq!(r("-3 + 5"), "2");
        assert_eq!(r("sqrt(16) + 1"), "5");
        assert_eq!(r("5!"), "120");
    }

    #[test]
    fn percentages() {
        assert_eq!(r("50% of 200"), "100");
        assert_eq!(r("200 + 10%"), "220");
        assert_eq!(r("200 - 25%"), "150");
        assert_eq!(r("10 % 3"), "1");
    }

    #[test]
    fn conversions() {
        assert_eq!(r("500 km to mi"), "310.6855961 mi");
        assert_eq!(r("32 f to c"), "0°C");
        assert_eq!(r("1 gb in mb"), "1,000 MB");
        assert_eq!(r("2 hours in minutes"), "120 min");
        assert_eq!(r("5 km to kg"), "");
    }

    #[test]
    fn not_a_sum() {
        assert!(evaluate("42").is_none());
        assert!(evaluate("firefox").is_none());
        assert!(evaluate("3d printer").is_none());
    }
}
