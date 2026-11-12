//! The units the calculator converts between, and the phrases people write them with.

pub const LENGTH: u8 = 0;
pub const MASS: u8 = 1;
pub const TEMP: u8 = 2;
pub const VOLUME: u8 = 3;
pub const AREA: u8 = 4;
pub const SPEED: u8 = 5;
pub const TIME: u8 = 6;
pub const DATA: u8 = 7;
pub const ENERGY: u8 = 8;
pub const PRESSURE: u8 = 9;
pub const ANGLE: u8 = 10;
pub const POWER: u8 = 11;
pub const FREQUENCY: u8 = 12;
pub const FORCE: u8 = 13;
pub const DATA_RATE: u8 = 14;
pub const FUEL: u8 = 15;
pub const TYPE: u8 = 16;
pub const CURRENCY: u8 = 17;

/// A unit a value is measured in. Currencies are units too, with a factor from the day's rates.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub label: String,
    pub symbol: String,
    pub kind: u8,
    /// Multiply by this to reach the kind's base unit.
    pub factor: f64,
    pub offset: f64,
    /// The base is reached by dividing instead: litres per 100 km against km per litre.
    pub inverse: bool,
    /// The symbol leads the number: `$20`.
    pub leads: bool,
}

impl Unit {
    pub fn to_base(&self, v: f64) -> f64 {
        if self.inverse { self.factor / v } else { (v + self.offset) * self.factor }
    }
    pub fn from_base(&self, base: f64) -> f64 {
        if self.inverse { self.factor / base } else { base / self.factor - self.offset }
    }
    /// The symbol for an amount: `1 day`, `2 days`.
    pub fn symbol_for(&self, n: f64) -> &str {
        if n.abs() == 1.0 && self.symbol.len() > 3 && self.symbol.is_ascii() && self.symbol.ends_with('s') && !self.symbol.contains('/') {
            &self.symbol[..self.symbol.len() - 1]
        } else {
            &self.symbol
        }
    }
}

struct Def {
    names: &'static [&'static str],
    label: &'static str,
    symbol: &'static str,
    kind: u8,
    factor: f64,
    offset: f64,
    inverse: bool,
}

const fn u(names: &'static [&'static str], label: &'static str, symbol: &'static str, kind: u8, factor: f64) -> Def {
    Def { names, label, symbol, kind, factor, offset: 0.0, inverse: false }
}

const fn t(names: &'static [&'static str], label: &'static str, symbol: &'static str, factor: f64, offset: f64) -> Def {
    Def { names, label, symbol, kind: TEMP, factor, offset, inverse: false }
}

const PI: f64 = std::f64::consts::PI;

static UNITS: &[Def] = &[
    // Length, in metres.
    u(&["nm", "nanometer", "nanometers", "nanometre", "nanometres"], "Nanometers", "nm", LENGTH, 1e-9),
    u(&["µm", "um", "micron", "microns", "micrometer", "micrometers", "micrometre", "micrometres"], "Micrometers", "µm", LENGTH, 1e-6),
    u(&["mm", "millimeter", "millimeters", "millimetre", "millimetres"], "Millimeters", "mm", LENGTH, 0.001),
    u(&["cm", "centimeter", "centimeters", "centimetre", "centimetres"], "Centimeters", "cm", LENGTH, 0.01),
    u(&["dm", "decimeter", "decimeters", "decimetre", "decimetres"], "Decimeters", "dm", LENGTH, 0.1),
    u(&["m", "meter", "meters", "metre", "metres"], "Meters", "m", LENGTH, 1.0),
    u(&["km", "kms", "kilometer", "kilometers", "kilometre", "kilometres"], "Kilometers", "km", LENGTH, 1000.0),
    u(&["in", "inch", "inches", "\""], "Inches", "in", LENGTH, 0.0254),
    u(&["ft", "foot", "feet", "'"], "Feet", "ft", LENGTH, 0.3048),
    u(&["yd", "yds", "yard", "yards"], "Yards", "yd", LENGTH, 0.9144),
    u(&["mi", "mile", "miles"], "Miles", "mi", LENGTH, 1609.344),
    u(&["nmi", "nauticalmile", "nauticalmiles"], "Nautical Miles", "nmi", LENGTH, 1852.0),
    u(&["furlong", "furlongs"], "Furlongs", "furlongs", LENGTH, 201.168),
    u(&["mil", "mils", "thou"], "Mils", "mil", LENGTH, 0.0000254),
    u(&["au", "astronomicalunit", "astronomicalunits"], "Astronomical Units", "AU", LENGTH, 149_597_870_700.0),
    u(&["ly", "lightyear", "lightyears"], "Light Years", "ly", LENGTH, 9.460_730_472_580_8e15),
    u(&["parsec", "parsecs"], "Parsecs", "pc", LENGTH, 3.085_677_581_491_367e16),
    // Mass, in kilograms.
    u(&["µg", "ug", "mcg", "microgram", "micrograms"], "Micrograms", "µg", MASS, 1e-9),
    u(&["mg", "milligram", "milligrams"], "Milligrams", "mg", MASS, 1e-6),
    u(&["g", "gram", "grams", "gramme", "grammes"], "Grams", "g", MASS, 0.001),
    u(&["kg", "kgs", "kilogram", "kilograms", "kilo", "kilos"], "Kilograms", "kg", MASS, 1.0),
    u(&["t", "tonne", "tonnes", "ton", "tons", "metricton", "metrictons"], "Tonnes", "t", MASS, 1000.0),
    u(&["oz", "ounce", "ounces"], "Ounces", "oz", MASS, 0.028_349_523_125),
    u(&["lb", "lbs", "pound", "pounds"], "Pounds", "lb", MASS, 0.453_592_37),
    u(&["st", "stone", "stones"], "Stone", "st", MASS, 6.350_293_18),
    u(&["shortton", "shorttons", "uston", "ustons"], "Short Tons", "short tons", MASS, 907.184_74),
    u(&["ct", "carat", "carats"], "Carats", "ct", MASS, 0.0002),
    // Temperature, in kelvin.
    t(&["c", "°c", "celsius", "centigrade", "degc"], "Celsius", "°C", 1.0, 273.15),
    t(&["f", "°f", "fahrenheit", "degf"], "Fahrenheit", "°F", 5.0 / 9.0, 459.67),
    t(&["k", "kelvin", "kelvins"], "Kelvin", "K", 1.0, 0.0),
    // Volume, in litres.
    u(&["ml", "milliliter", "milliliters", "millilitre", "millilitres"], "Milliliters", "mL", VOLUME, 0.001),
    u(&["cl", "centiliter", "centiliters", "centilitre", "centilitres"], "Centiliters", "cL", VOLUME, 0.01),
    u(&["dl", "deciliter", "deciliters", "decilitre", "decilitres"], "Deciliters", "dL", VOLUME, 0.1),
    u(&["l", "liter", "liters", "litre", "litres"], "Liters", "L", VOLUME, 1.0),
    u(&["tsp", "teaspoon", "teaspoons"], "Teaspoons", "tsp", VOLUME, 0.004_928_921_59),
    u(&["tbsp", "tablespoon", "tablespoons"], "Tablespoons", "tbsp", VOLUME, 0.014_786_764_8),
    u(&["floz", "fluidounce", "fluidounces", "oz"], "Fluid Ounces", "fl oz", VOLUME, 0.029_573_529_6),
    u(&["cup", "cups"], "Cups", "cups", VOLUME, 0.236_588_236_5),
    u(&["pt", "pint", "pints"], "Pints", "pt", VOLUME, 0.473_176_473),
    u(&["qt", "quart", "quarts"], "Quarts", "qt", VOLUME, 0.946_352_946),
    u(&["gal", "gallon", "gallons"], "Gallons", "gal", VOLUME, 3.785_411_784),
    u(&["bbl", "barrel", "barrels"], "Barrels", "bbl", VOLUME, 158.987_294_928),
    u(&["cc", "cm3", "cm³", "cubiccentimeter", "cubiccentimeters"], "Cubic Centimeters", "cm³", VOLUME, 0.001),
    u(&["m3", "m³", "cubicmeter", "cubicmeters", "cbm"], "Cubic Meters", "m³", VOLUME, 1000.0),
    u(&["cuin", "in3", "in³", "cubicinch", "cubicinches"], "Cubic Inches", "in³", VOLUME, 0.016_387_064),
    u(&["cuft", "ft3", "ft³", "cubicfoot", "cubicfeet"], "Cubic Feet", "ft³", VOLUME, 28.316_846_592),
    // Area, in square metres.
    u(&["sqmm", "mm2", "mm²"], "Square Millimeters", "mm²", AREA, 1e-6),
    u(&["sqcm", "cm2", "cm²"], "Square Centimeters", "cm²", AREA, 1e-4),
    u(&["sqm", "m2", "m²", "squaremeter", "squaremeters"], "Square Meters", "m²", AREA, 1.0),
    u(&["sqkm", "km2", "km²"], "Square Kilometers", "km²", AREA, 1e6),
    u(&["sqin", "in2", "in²"], "Square Inches", "in²", AREA, 0.000_645_16),
    u(&["sqft", "ft2", "ft²", "squarefeet", "squarefoot"], "Square Feet", "ft²", AREA, 0.092_903_04),
    u(&["sqyd", "yd2", "yd²"], "Square Yards", "yd²", AREA, 0.836_127_36),
    u(&["sqmi", "mi2", "mi²"], "Square Miles", "mi²", AREA, 2_589_988.110_336),
    u(&["acre", "acres"], "Acres", "acres", AREA, 4046.856_422_4),
    u(&["ha", "hectare", "hectares"], "Hectares", "ha", AREA, 10_000.0),
    // Speed, in metres per second.
    u(&["mps", "m/s"], "Meters per Second", "m/s", SPEED, 1.0),
    u(&["kph", "kmh", "kmph", "km/h", "km/hr"], "Kilometers per Hour", "km/h", SPEED, 1.0 / 3.6),
    u(&["mph", "mi/h", "mi/hr"], "Miles per Hour", "mph", SPEED, 0.447_04),
    u(&["fps", "ft/s"], "Feet per Second", "ft/s", SPEED, 0.3048),
    u(&["knot", "knots", "kn", "kt", "kts"], "Knots", "kn", SPEED, 1852.0 / 3600.0),
    u(&["mach"], "Mach", "Mach", SPEED, 340.29),
    // Time, in seconds.
    u(&["ns", "nanosecond", "nanoseconds"], "Nanoseconds", "ns", TIME, 1e-9),
    u(&["µs", "us", "microsecond", "microseconds"], "Microseconds", "µs", TIME, 1e-6),
    u(&["ms", "millisecond", "milliseconds"], "Milliseconds", "ms", TIME, 0.001),
    u(&["s", "sec", "secs", "second", "seconds"], "Seconds", "s", TIME, 1.0),
    u(&["min", "mins", "minute", "minutes"], "Minutes", "min", TIME, 60.0),
    u(&["h", "hr", "hrs", "hour", "hours"], "Hours", "h", TIME, 3600.0),
    u(&["d", "day", "days"], "Days", "days", TIME, 86_400.0),
    u(&["wk", "wks", "week", "weeks"], "Weeks", "weeks", TIME, 604_800.0),
    u(&["fortnight", "fortnights"], "Fortnights", "fortnights", TIME, 1_209_600.0),
    u(&["mo", "mos", "month", "months"], "Months", "months", TIME, 2_629_746.0),
    u(&["yr", "yrs", "year", "years"], "Years", "years", TIME, 31_556_952.0),
    u(&["decade", "decades"], "Decades", "decades", TIME, 315_569_520.0),
    u(&["century", "centuries"], "Centuries", "centuries", TIME, 3_155_695_200.0),
    // Data, in bytes.
    u(&["bit", "bits"], "Bits", "bit", DATA, 0.125),
    u(&["kbit", "kbits", "kilobit", "kilobits"], "Kilobits", "kbit", DATA, 125.0),
    u(&["mbit", "mbits", "megabit", "megabits"], "Megabits", "Mbit", DATA, 125_000.0),
    u(&["gbit", "gbits", "gigabit", "gigabits"], "Gigabits", "Gbit", DATA, 1.25e8),
    u(&["b", "byte", "bytes"], "Bytes", "B", DATA, 1.0),
    u(&["kb", "kilobyte", "kilobytes"], "Kilobytes", "kB", DATA, 1e3),
    u(&["mb", "megabyte", "megabytes"], "Megabytes", "MB", DATA, 1e6),
    u(&["gb", "gigabyte", "gigabytes"], "Gigabytes", "GB", DATA, 1e9),
    u(&["tb", "terabyte", "terabytes"], "Terabytes", "TB", DATA, 1e12),
    u(&["pb", "petabyte", "petabytes"], "Petabytes", "PB", DATA, 1e15),
    u(&["eb", "exabyte", "exabytes"], "Exabytes", "EB", DATA, 1e18),
    u(&["kib", "kibibyte", "kibibytes"], "Kibibytes", "KiB", DATA, 1024.0),
    u(&["mib", "mebibyte", "mebibytes"], "Mebibytes", "MiB", DATA, 1_048_576.0),
    u(&["gib", "gibibyte", "gibibytes"], "Gibibytes", "GiB", DATA, 1_073_741_824.0),
    u(&["tib", "tebibyte", "tebibytes"], "Tebibytes", "TiB", DATA, 1_099_511_627_776.0),
    u(&["pib", "pebibyte", "pebibytes"], "Pebibytes", "PiB", DATA, 1_125_899_906_842_624.0),
    // Data rate, in bytes per second.
    u(&["bps", "bit/s"], "Bits per Second", "bit/s", DATA_RATE, 0.125),
    u(&["kbps", "kbit/s"], "Kilobits per Second", "kbit/s", DATA_RATE, 125.0),
    u(&["mbps", "mbit/s"], "Megabits per Second", "Mbit/s", DATA_RATE, 125_000.0),
    u(&["gbps", "gbit/s"], "Gigabits per Second", "Gbit/s", DATA_RATE, 1.25e8),
    u(&["b/s"], "Bytes per Second", "B/s", DATA_RATE, 1.0),
    u(&["kb/s"], "Kilobytes per Second", "kB/s", DATA_RATE, 1e3),
    u(&["mb/s"], "Megabytes per Second", "MB/s", DATA_RATE, 1e6),
    u(&["gb/s"], "Gigabytes per Second", "GB/s", DATA_RATE, 1e9),
    u(&["mib/s"], "Mebibytes per Second", "MiB/s", DATA_RATE, 1_048_576.0),
    // Energy, in joules.
    u(&["ev", "electronvolt", "electronvolts"], "Electronvolts", "eV", ENERGY, 1.602_176_634e-19),
    u(&["j", "joule", "joules"], "Joules", "J", ENERGY, 1.0),
    u(&["kj", "kilojoule", "kilojoules"], "Kilojoules", "kJ", ENERGY, 1000.0),
    u(&["mj", "megajoule", "megajoules"], "Megajoules", "MJ", ENERGY, 1e6),
    u(&["cal", "calorie", "calories"], "Calories", "cal", ENERGY, 4.184),
    u(&["kcal", "kilocalorie", "kilocalories"], "Kilocalories", "kcal", ENERGY, 4184.0),
    u(&["wh", "watthour", "watthours"], "Watt Hours", "Wh", ENERGY, 3600.0),
    u(&["kwh", "kilowatthour", "kilowatthours"], "Kilowatt Hours", "kWh", ENERGY, 3.6e6),
    u(&["mwh", "megawatthour", "megawatthours"], "Megawatt Hours", "MWh", ENERGY, 3.6e9),
    u(&["btu", "btus"], "BTU", "BTU", ENERGY, 1055.055_852_62),
    // Pressure, in pascals.
    u(&["pa", "pascal", "pascals"], "Pascals", "Pa", PRESSURE, 1.0),
    u(&["hpa", "hectopascal", "hectopascals"], "Hectopascals", "hPa", PRESSURE, 100.0),
    u(&["kpa", "kilopascal", "kilopascals"], "Kilopascals", "kPa", PRESSURE, 1000.0),
    u(&["mpa", "megapascal", "megapascals"], "Megapascals", "MPa", PRESSURE, 1e6),
    u(&["bar", "bars"], "Bar", "bar", PRESSURE, 100_000.0),
    u(&["mbar", "millibar", "millibars"], "Millibar", "mbar", PRESSURE, 100.0),
    u(&["psi"], "PSI", "psi", PRESSURE, 6894.757_293),
    u(&["atm", "atmosphere", "atmospheres"], "Atmospheres", "atm", PRESSURE, 101_325.0),
    u(&["mmhg"], "Millimeters of Mercury", "mmHg", PRESSURE, 133.322_387_415),
    u(&["inhg"], "Inches of Mercury", "inHg", PRESSURE, 3386.389),
    u(&["torr"], "Torr", "Torr", PRESSURE, 101_325.0 / 760.0),
    // Angle, in radians.
    u(&["deg", "degree", "degrees", "°"], "Degrees", "°", ANGLE, PI / 180.0),
    u(&["rad", "radian", "radians"], "Radians", "rad", ANGLE, 1.0),
    u(&["grad", "grads", "gradian", "gradians", "gon"], "Gradians", "grad", ANGLE, PI / 200.0),
    u(&["turn", "turns", "rev", "revolution", "revolutions"], "Turns", "turns", ANGLE, 2.0 * PI),
    u(&["arcmin", "arcminute", "arcminutes"], "Arcminutes", "arcmin", ANGLE, PI / 10_800.0),
    u(&["arcsec", "arcsecond", "arcseconds"], "Arcseconds", "arcsec", ANGLE, PI / 648_000.0),
    // Power, in watts.
    u(&["milliwatt", "milliwatts"], "Milliwatts", "mW", POWER, 0.001),
    u(&["w", "watt", "watts"], "Watts", "W", POWER, 1.0),
    u(&["kw", "kilowatt", "kilowatts"], "Kilowatts", "kW", POWER, 1000.0),
    u(&["mw", "megawatt", "megawatts"], "Megawatts", "MW", POWER, 1e6),
    u(&["gw", "gigawatt", "gigawatts"], "Gigawatts", "GW", POWER, 1e9),
    u(&["hp", "horsepower"], "Horsepower", "hp", POWER, 745.699_872),
    // Frequency, in hertz.
    u(&["hz", "hertz"], "Hertz", "Hz", FREQUENCY, 1.0),
    u(&["khz", "kilohertz"], "Kilohertz", "kHz", FREQUENCY, 1e3),
    u(&["mhz", "megahertz"], "Megahertz", "MHz", FREQUENCY, 1e6),
    u(&["ghz", "gigahertz"], "Gigahertz", "GHz", FREQUENCY, 1e9),
    u(&["rpm"], "Revolutions per Minute", "rpm", FREQUENCY, 1.0 / 60.0),
    // Force, in newtons.
    u(&["n", "newton", "newtons"], "Newtons", "N", FORCE, 1.0),
    u(&["kilonewton", "kilonewtons"], "Kilonewtons", "kN", FORCE, 1000.0),
    u(&["lbf", "poundforce"], "Pound-force", "lbf", FORCE, 4.448_221_615_260_5),
    u(&["kgf", "kilogramforce"], "Kilogram-force", "kgf", FORCE, 9.806_65),
    u(&["dyn", "dyne", "dynes"], "Dynes", "dyn", FORCE, 1e-5),
    // Fuel economy, in kilometres per litre.
    u(&["mpg"], "Miles per Gallon", "mpg", FUEL, 0.425_143_707),
    u(&["mpguk", "ukmpg"], "Miles per Imperial Gallon", "mpg (UK)", FUEL, 0.354_006_19),
    u(&["kmpl", "km/l"], "Kilometers per Liter", "km/L", FUEL, 1.0),
    Def { names: &["lp100km", "l/100km"], label: "Liters per 100 km", symbol: "L/100km", kind: FUEL, factor: 100.0, offset: 0.0, inverse: true },
    // Type and screen sizes, in CSS pixels.
    u(&["px", "pixel", "pixels"], "Pixels", "px", TYPE, 1.0),
    u(&["pt", "pts", "point", "points"], "Points", "pt", TYPE, 96.0 / 72.0),
    u(&["em", "ems"], "Ems", "em", TYPE, 16.0),
    u(&["rem", "rems"], "Rems", "rem", TYPE, 16.0),
    u(&["pica", "picas"], "Picas", "pc", TYPE, 16.0),
];

/// Spellings whose capitals carry the meaning: `Mb` is megabits where `mb` is megabytes.
static CASED: &[(&str, &str)] = &[
    ("Kb", "kbit"), ("Mb", "mbit"), ("Gb", "gbit"), ("Kbit", "kbit"), ("Mbit", "mbit"), ("Gbit", "gbit"),
    ("Kb/s", "kbps"), ("Mb/s", "mbps"), ("Gb/s", "gbps"), ("b/s", "bps"),
    ("kN", "kilonewton"), ("mW", "milliwatt"), ("K", "kelvin"), ("C", "celsius"), ("F", "fahrenheit"),
];

/// Phrases folded to one name before the expression is read.
pub static PHRASES: &[(&str, &str)] = &[
    ("square meters", "sqm"), ("square metres", "sqm"), ("square meter", "sqm"), ("square metre", "sqm"), ("sq m", "sqm"), ("sq meters", "sqm"),
    ("square feet", "sqft"), ("square foot", "sqft"), ("sq ft", "sqft"), ("sq feet", "sqft"),
    ("square kilometers", "sqkm"), ("square kilometres", "sqkm"), ("square kilometer", "sqkm"), ("sq km", "sqkm"),
    ("square miles", "sqmi"), ("square mile", "sqmi"), ("sq mi", "sqmi"),
    ("square inches", "sqin"), ("square inch", "sqin"), ("sq in", "sqin"),
    ("square yards", "sqyd"), ("square yard", "sqyd"), ("sq yd", "sqyd"),
    ("square centimeters", "sqcm"), ("square centimetres", "sqcm"), ("sq cm", "sqcm"),
    ("square millimeters", "sqmm"), ("square millimetres", "sqmm"), ("sq mm", "sqmm"),
    ("cubic meters", "m3"), ("cubic metres", "m3"), ("cubic meter", "m3"), ("cubic metre", "m3"),
    ("cubic feet", "cuft"), ("cubic foot", "cuft"), ("cu ft", "cuft"),
    ("cubic inches", "cuin"), ("cubic inch", "cuin"), ("cu in", "cuin"),
    ("cubic centimeters", "cc"), ("cubic centimetres", "cc"), ("cubic centimeter", "cc"),
    ("fluid ounces", "floz"), ("fluid ounce", "floz"), ("fl oz", "floz"),
    ("nautical miles", "nmi"), ("nautical mile", "nmi"),
    ("light years", "ly"), ("light year", "ly"),
    ("astronomical units", "au"), ("astronomical unit", "au"),
    ("metric tons", "tonnes"), ("metric ton", "tonnes"), ("short tons", "shortton"), ("short ton", "shortton"),
    ("miles per hour", "mph"), ("kilometers per hour", "kph"), ("kilometres per hour", "kph"), ("km per hour", "kph"),
    ("meters per second", "mps"), ("metres per second", "mps"), ("feet per second", "fps"),
    ("miles per gallon", "mpg"), ("liters per 100 km", "lp100km"), ("litres per 100 km", "lp100km"),
    ("l/100km", "lp100km"), ("l/100 km", "lp100km"), ("km per liter", "kmpl"), ("km per litre", "kmpl"),
    ("degrees celsius", "celsius"), ("degree celsius", "celsius"), ("degrees fahrenheit", "fahrenheit"), ("degree fahrenheit", "fahrenheit"),
    ("degrees c", "celsius"), ("degrees f", "fahrenheit"), ("degrees kelvin", "kelvin"),
    ("watt hours", "wh"), ("kilowatt hours", "kwh"), ("megawatt hours", "mwh"),
    ("pound force", "lbf"), ("pounds of force", "lbf"),
];

fn make(d: &Def) -> Unit {
    Unit { label: d.label.into(), symbol: d.symbol.into(), kind: d.kind, factor: d.factor, offset: d.offset, inverse: d.inverse, leads: false }
}

/// Every unit a name could mean, the likeliest first: `oz` is ounces, and fluid ounces too.
pub fn find_all(name: &str) -> Vec<Unit> {
    let trimmed = name.trim();
    let exact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    let mut out = Vec::new();
    if let Some((_, canonical)) = CASED.iter().find(|(cased, _)| *cased == exact) {
        out.extend(UNITS.iter().filter(|d| d.names.contains(canonical)).map(make));
    }
    let key = exact.to_lowercase();
    for d in UNITS.iter().filter(|d| d.names.contains(&key.as_str())) {
        let unit = make(d);
        if !out.contains(&unit) {
            out.push(unit);
        }
    }
    out
}

/// The unit a name means, preferring one of the wanted kind.
pub fn find(name: &str, prefer: Option<u8>) -> Option<Unit> {
    let mut all = find_all(name);
    if let Some(at) = prefer.and_then(|kind| all.iter().position(|u| u.kind == kind)) {
        return Some(all.swap_remove(at));
    }
    (!all.is_empty()).then(|| all.swap_remove(0))
}
