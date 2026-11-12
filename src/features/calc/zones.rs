//! Time zones by the names people use for them: cities, countries and abbreviations. Offsets
//! come from the system's zone database through `date`, so daylight saving is always right.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// An IANA name, or empty for a fixed offset.
    pub iana: String,
    pub fixed: i32,
    /// What the card calls it: "Tokyo", "UTC+5:30".
    pub label: String,
}

static ALIASES: &[(&str, &str)] = &[
    ("utc", "UTC"), ("gmt", "UTC"), ("z", "UTC"), ("zulu", "UTC"),
    ("est", "America/New_York"), ("edt", "America/New_York"), ("et", "America/New_York"), ("eastern", "America/New_York"),
    ("cst", "America/Chicago"), ("cdt", "America/Chicago"), ("ct", "America/Chicago"), ("central", "America/Chicago"),
    ("mst", "America/Denver"), ("mdt", "America/Denver"), ("mt", "America/Denver"), ("mountain", "America/Denver"),
    ("pst", "America/Los_Angeles"), ("pdt", "America/Los_Angeles"), ("pt", "America/Los_Angeles"), ("pacific", "America/Los_Angeles"),
    ("akst", "America/Anchorage"), ("akdt", "America/Anchorage"), ("hst", "Pacific/Honolulu"), ("ast", "America/Halifax"),
    ("nst", "America/St_Johns"), ("brt", "America/Sao_Paulo"), ("art", "America/Argentina/Buenos_Aires"),
    ("ist", "Asia/Kolkata"), ("bst", "Europe/London"), ("wet", "Europe/Lisbon"), ("cet", "Europe/Paris"), ("cest", "Europe/Paris"),
    ("eet", "Europe/Athens"), ("eest", "Europe/Athens"), ("msk", "Europe/Moscow"), ("trt", "Europe/Istanbul"),
    ("gst", "Asia/Dubai"), ("pkt", "Asia/Karachi"), ("npt", "Asia/Kathmandu"), ("ict", "Asia/Bangkok"), ("wib", "Asia/Jakarta"),
    ("sgt", "Asia/Singapore"), ("hkt", "Asia/Hong_Kong"), ("pht", "Asia/Manila"), ("jst", "Asia/Tokyo"), ("kst", "Asia/Seoul"),
    ("awst", "Australia/Perth"), ("acst", "Australia/Adelaide"), ("aest", "Australia/Sydney"), ("aedt", "Australia/Sydney"), ("aet", "Australia/Sydney"),
    ("nzst", "Pacific/Auckland"), ("nzdt", "Pacific/Auckland"), ("sast", "Africa/Johannesburg"), ("cat", "Africa/Maputo"),
    ("wat", "Africa/Lagos"), ("eat", "Africa/Nairobi"),
    // Cities and countries the zone database files under another name.
    ("nyc", "America/New_York"), ("new york city", "America/New_York"), ("boston", "America/New_York"), ("washington", "America/New_York"),
    ("washington dc", "America/New_York"), ("dc", "America/New_York"), ("miami", "America/New_York"), ("atlanta", "America/New_York"),
    ("philadelphia", "America/New_York"), ("florida", "America/New_York"), ("usa", "America/New_York"), ("us", "America/New_York"),
    ("dallas", "America/Chicago"), ("houston", "America/Chicago"), ("austin", "America/Chicago"), ("texas", "America/Chicago"),
    ("minneapolis", "America/Chicago"), ("nashville", "America/Chicago"), ("new orleans", "America/Chicago"),
    ("salt lake city", "America/Denver"), ("colorado", "America/Denver"), ("arizona", "America/Phoenix"),
    ("la", "America/Los_Angeles"), ("sf", "America/Los_Angeles"), ("san francisco", "America/Los_Angeles"), ("seattle", "America/Los_Angeles"),
    ("san diego", "America/Los_Angeles"), ("san jose", "America/Los_Angeles"), ("las vegas", "America/Los_Angeles"), ("portland", "America/Los_Angeles"),
    ("california", "America/Los_Angeles"), ("silicon valley", "America/Los_Angeles"), ("hawaii", "Pacific/Honolulu"), ("alaska", "America/Anchorage"),
    ("canada", "America/Toronto"), ("ottawa", "America/Toronto"), ("montreal", "America/Toronto"), ("calgary", "America/Edmonton"),
    ("mexico", "America/Mexico_City"), ("brazil", "America/Sao_Paulo"), ("rio", "America/Sao_Paulo"), ("rio de janeiro", "America/Sao_Paulo"),
    ("argentina", "America/Argentina/Buenos_Aires"), ("buenos aires", "America/Argentina/Buenos_Aires"), ("chile", "America/Santiago"),
    ("colombia", "America/Bogota"), ("peru", "America/Lima"),
    ("uk", "Europe/London"), ("england", "Europe/London"), ("britain", "Europe/London"), ("scotland", "Europe/London"),
    ("edinburgh", "Europe/London"), ("manchester", "Europe/London"), ("ireland", "Europe/Dublin"),
    ("france", "Europe/Paris"), ("germany", "Europe/Berlin"), ("munich", "Europe/Berlin"), ("frankfurt", "Europe/Berlin"), ("hamburg", "Europe/Berlin"),
    ("spain", "Europe/Madrid"), ("barcelona", "Europe/Madrid"), ("italy", "Europe/Rome"), ("milan", "Europe/Rome"),
    ("netherlands", "Europe/Amsterdam"), ("belgium", "Europe/Brussels"), ("switzerland", "Europe/Zurich"), ("geneva", "Europe/Zurich"),
    ("austria", "Europe/Vienna"), ("sweden", "Europe/Stockholm"), ("norway", "Europe/Oslo"), ("denmark", "Europe/Copenhagen"),
    ("finland", "Europe/Helsinki"), ("portugal", "Europe/Lisbon"), ("poland", "Europe/Warsaw"), ("greece", "Europe/Athens"),
    ("czechia", "Europe/Prague"), ("hungary", "Europe/Budapest"), ("romania", "Europe/Bucharest"), ("ukraine", "Europe/Kyiv"),
    ("russia", "Europe/Moscow"), ("turkey", "Europe/Istanbul"), ("israel", "Asia/Jerusalem"), ("tel aviv", "Asia/Jerusalem"),
    ("uae", "Asia/Dubai"), ("abu dhabi", "Asia/Dubai"), ("saudi arabia", "Asia/Riyadh"), ("saudi", "Asia/Riyadh"), ("doha", "Asia/Qatar"),
    ("india", "Asia/Kolkata"), ("mumbai", "Asia/Kolkata"), ("delhi", "Asia/Kolkata"), ("new delhi", "Asia/Kolkata"),
    ("bangalore", "Asia/Kolkata"), ("bengaluru", "Asia/Kolkata"), ("hyderabad", "Asia/Kolkata"), ("chennai", "Asia/Kolkata"),
    ("pune", "Asia/Kolkata"), ("calcutta", "Asia/Kolkata"), ("jaipur", "Asia/Kolkata"), ("ahmedabad", "Asia/Kolkata"),
    ("pakistan", "Asia/Karachi"), ("islamabad", "Asia/Karachi"), ("lahore", "Asia/Karachi"), ("bangladesh", "Asia/Dhaka"),
    ("sri lanka", "Asia/Colombo"), ("nepal", "Asia/Kathmandu"), ("thailand", "Asia/Bangkok"), ("vietnam", "Asia/Ho_Chi_Minh"),
    ("hanoi", "Asia/Ho_Chi_Minh"), ("saigon", "Asia/Ho_Chi_Minh"), ("indonesia", "Asia/Jakarta"), ("bali", "Asia/Makassar"),
    ("philippines", "Asia/Manila"), ("malaysia", "Asia/Kuala_Lumpur"), ("china", "Asia/Shanghai"), ("beijing", "Asia/Shanghai"),
    ("shenzhen", "Asia/Shanghai"), ("guangzhou", "Asia/Shanghai"), ("taiwan", "Asia/Taipei"), ("korea", "Asia/Seoul"),
    ("south korea", "Asia/Seoul"), ("japan", "Asia/Tokyo"), ("osaka", "Asia/Tokyo"), ("kyoto", "Asia/Tokyo"),
    ("australia", "Australia/Sydney"), ("canberra", "Australia/Sydney"), ("new zealand", "Pacific/Auckland"), ("wellington", "Pacific/Auckland"),
    ("south africa", "Africa/Johannesburg"), ("cape town", "Africa/Johannesburg"), ("nigeria", "Africa/Lagos"), ("kenya", "Africa/Nairobi"),
    ("egypt", "Africa/Cairo"), ("morocco", "Africa/Casablanca"), ("ghana", "Africa/Accra"), ("ethiopia", "Africa/Addis_Ababa"),
];

const ZONEINFO: &str = "/usr/share/zoneinfo";

/// Every zone the system knows, by its city in lower case: "los angeles", "kolkata".
fn cities() -> &'static HashMap<String, String> {
    static CITIES: OnceLock<HashMap<String, String>> = OnceLock::new();
    CITIES.get_or_init(|| {
        let mut map = HashMap::new();
        for file in ["zone1970.tab", "zone.tab"] {
            let Ok(table) = std::fs::read_to_string(format!("{ZONEINFO}/{file}")) else { continue };
            for name in table.lines().filter(|l| !l.starts_with('#')).filter_map(|l| l.split('\t').nth(2)) {
                let city = name.rsplit('/').next().unwrap_or(name).replace('_', " ").to_lowercase();
                map.entry(city).or_insert_with(|| name.to_string());
            }
        }
        map
    })
}

fn title(words: &str) -> String {
    words.split(' ').map(|w| {
        let mut chars = w.chars();
        chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" ")
}

/// The zone this machine keeps, from where `/etc/localtime` points.
pub fn local_name() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':').to_string();
        if !tz.is_empty() && std::path::Path::new(ZONEINFO).join(&tz).is_file() {
            return Some(tz);
        }
    }
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let text = target.to_string_lossy();
    text.split_once("zoneinfo/").map(|(_, name)| name.to_string())
}

pub fn local() -> Zone {
    let iana = local_name().unwrap_or_else(|| "UTC".into());
    let label = iana.rsplit('/').next().unwrap_or("Local").replace('_', " ");
    Zone { iana, fixed: 0, label }
}

fn utc_label(secs: i32) -> String {
    let (h, m) = (secs.abs() / 3600, secs.abs() % 3600 / 60);
    let sign = if secs < 0 { '-' } else { '+' };
    if m == 0 { format!("UTC{sign}{h}") } else { format!("UTC{sign}{h}:{m:02}") }
}

/// `utc+5:30`, `gmt-8`, `utc+0530`.
fn fixed(word: &str) -> Option<Zone> {
    let rest = word.strip_prefix("utc").or_else(|| word.strip_prefix("gmt"))?;
    let sign = match rest.chars().next()? {
        '+' => 1,
        '-' | '−' => -1,
        _ => return None,
    };
    let digits = &rest[rest.chars().next()?.len_utf8()..];
    let (h, m): (i32, i32) = match digits.split_once(':') {
        Some((h, m)) => (h.parse().ok()?, m.parse().ok()?),
        None if digits.len() == 4 => (digits[..2].parse().ok()?, digits[2..].parse().ok()?),
        None => (digits.parse().ok()?, 0),
    };
    if h > 14 || m > 59 {
        return None;
    }
    let label = utc_label(sign * (h * 3600 + m * 60));
    Some(Zone { iana: String::new(), fixed: sign * (h * 3600 + m * 60), label })
}

fn named(name: &str) -> Option<Zone> {
    if matches!(name, "local" | "here" | "my time" | "local time" | "home") {
        return Some(local());
    }
    if let Some(zone) = fixed(name) {
        return Some(zone);
    }
    let known = |iana: &str| std::path::Path::new(ZONEINFO).join(iana).is_file();
    if let Some((alias, iana)) = ALIASES.iter().find(|(alias, _)| *alias == name).filter(|(_, iana)| known(iana)) {
        // An abbreviation is its own best label; a place is titled.
        let short = alias.len() <= 4 && !alias.contains(' ') && !matches!(*alias, "rio" | "bali" | "doha" | "pune" | "peru" | "uae" | "usa" | "uk" | "us");
        let label = if short || matches!(*alias, "uae" | "usa" | "uk" | "us" | "nyc" | "la" | "sf" | "dc") { alias.to_uppercase() } else { title(alias) };
        return Some(Zone { iana: iana.to_string(), fixed: 0, label });
    }
    let iana = cities().get(name)?;
    known(iana).then(|| Zone { iana: iana.clone(), fixed: 0, label: title(name) })
}

/// The zone the leading words name, and how many of them it took. Longer names win.
pub fn find(words: &[&str]) -> Option<(Zone, usize)> {
    (1..=words.len().min(4)).rev().find_map(|n| named(&words[..n].join(" ")).map(|zone| (zone, n)))
}

/// A zone's offset from UTC, in seconds, at an instant, and the abbreviation it goes by then.
pub fn offset(zone: &Zone, utc: i64) -> Option<(i32, String)> {
    if zone.iana.is_empty() {
        return Some((zone.fixed, zone.label.clone()));
    }
    static CACHE: Mutex<Option<HashMap<(String, i64), (i32, String)>>> = Mutex::new(None);
    // Offsets change on the quarter hour at the finest.
    let key = (zone.iana.clone(), utc.div_euclid(900));
    if let Some(hit) = CACHE.lock().ok()?.get_or_insert_with(HashMap::new).get(&key) {
        return Some(hit.clone());
    }
    let out = std::process::Command::new("date").env("TZ", &zone.iana).env("LC_ALL", "C")
        .args(["-d", &format!("@{utc}"), "+%z %Z"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (digits, abbr) = text.trim().split_once(' ')?;
    let sign = if digits.starts_with('-') { -1 } else { 1 };
    let (h, m): (i32, i32) = (digits.get(1..3)?.parse().ok()?, digits.get(3..5)?.parse().ok()?);
    let secs = sign * (h * 3600 + m * 60);
    // The database names some zones by their offset alone.
    let abbr = if abbr.starts_with(['+', '-']) { utc_label(secs) } else { abbr.to_string() };
    let hit = (secs, abbr);
    CACHE.lock().ok()?.get_or_insert_with(HashMap::new).insert(key, hit.clone());
    Some(hit)
}

/// The instant a wall-clock time in a zone names.
pub fn to_utc(zone: &Zone, wall: i64) -> Option<i64> {
    let (first, _) = offset(zone, wall)?;
    let (second, _) = offset(zone, wall - first as i64)?;
    Some(wall - second as i64)
}
