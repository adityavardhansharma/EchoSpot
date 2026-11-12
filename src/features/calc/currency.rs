//! Currencies: the day's exchange rates, fetched with `curl` in the background and kept on disk,
//! and the names, codes and symbols people write money with.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use rune::Waker;
use serde::{Deserialize, Serialize};

use super::units::{CURRENCY, Unit};
use crate::util;

/// Rates older than this are refreshed the next time money is typed.
const FRESH_SECS: u64 = 3 * 3600;
const RETRY_SECS: u64 = 60;

const SOURCES: &[&str] = &[
    "https://cdn.jsdelivr.net/npm/@fawazahmed0/currency-api@latest/v1/currencies/usd.min.json",
    "https://latest.currency-api.pages.dev/v1/currencies/usd.min.json",
    "https://open.er-api.com/v6/latest/USD",
];

#[derive(Serialize, Deserialize, Default, Clone)]
struct Rates {
    fetched: u64,
    /// Units of each currency one US dollar buys, by lower-case code.
    rates: HashMap<String, f64>,
}

static RATES: Mutex<Option<Rates>> = Mutex::new(None);
static WAKER: OnceLock<Waker> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);
static FETCHING: AtomicBool = AtomicBool::new(false);
static LAST_TRY: AtomicU64 = AtomicU64::new(0);
static HOME: Mutex<Option<String>> = Mutex::new(None);

struct Def {
    code: &'static str,
    label: &'static str,
    symbol: &'static str,
    names: &'static [&'static str],
}

const fn c(code: &'static str, label: &'static str, symbol: &'static str, names: &'static [&'static str]) -> Def {
    Def { code, label, symbol, names }
}

static CURRENCIES: &[Def] = &[
    c("usd", "US Dollars", "$", &["$", "us$", "dollar", "dollars", "buck", "bucks"]),
    c("eur", "Euros", "€", &["€", "euro", "euros"]),
    c("gbp", "British Pounds", "£", &["£", "quid", "sterling", "pound", "pounds"]),
    c("jpy", "Japanese Yen", "¥", &["¥", "yen"]),
    c("inr", "Indian Rupees", "₹", &["₹", "rupee", "rupees", "rs"]),
    c("cny", "Chinese Yuan", "CN¥", &["yuan", "rmb", "renminbi", "cn¥"]),
    c("aud", "Australian Dollars", "A$", &["a$", "au$"]),
    c("cad", "Canadian Dollars", "CA$", &["c$", "ca$"]),
    c("chf", "Swiss Francs", "", &["franc", "francs"]),
    c("hkd", "Hong Kong Dollars", "HK$", &["hk$"]),
    c("sgd", "Singapore Dollars", "S$", &["s$"]),
    c("nzd", "New Zealand Dollars", "NZ$", &["nz$"]),
    c("krw", "South Korean Won", "₩", &["₩", "won"]),
    c("mxn", "Mexican Pesos", "MX$", &["mx$"]),
    c("brl", "Brazilian Reais", "R$", &["r$", "real", "reais"]),
    c("rub", "Russian Rubles", "₽", &["₽", "ruble", "rubles", "rouble", "roubles"]),
    c("try", "Turkish Lira", "₺", &["₺", "lira"]),
    c("zar", "South African Rand", "", &["rand"]),
    c("sek", "Swedish Kronor", "", &["kronor"]),
    c("nok", "Norwegian Kroner", "", &[]),
    c("dkk", "Danish Kroner", "", &[]),
    c("pln", "Polish Zloty", "", &["zloty", "zł"]),
    c("czk", "Czech Koruna", "", &["koruna"]),
    c("huf", "Hungarian Forint", "", &["forint"]),
    c("ron", "Romanian Lei", "", &[]),
    c("bgn", "Bulgarian Lev", "", &[]),
    c("isk", "Icelandic Krona", "", &[]),
    c("uah", "Ukrainian Hryvnia", "₴", &["₴", "hryvnia"]),
    c("ils", "Israeli Shekels", "₪", &["₪", "shekel", "shekels"]),
    c("aed", "UAE Dirhams", "", &["dirham", "dirhams"]),
    c("sar", "Saudi Riyals", "", &["riyal", "riyals"]),
    c("qar", "Qatari Riyals", "", &[]),
    c("kwd", "Kuwaiti Dinars", "", &[]),
    c("bhd", "Bahraini Dinars", "", &[]),
    c("omr", "Omani Rials", "", &[]),
    c("jod", "Jordanian Dinars", "", &[]),
    c("egp", "Egyptian Pounds", "", &[]),
    c("mad", "Moroccan Dirhams", "", &[]),
    c("ngn", "Nigerian Naira", "₦", &["₦", "naira"]),
    c("kes", "Kenyan Shillings", "", &[]),
    c("ghs", "Ghanaian Cedis", "", &[]),
    c("tzs", "Tanzanian Shillings", "", &[]),
    c("ugx", "Ugandan Shillings", "", &[]),
    c("etb", "Ethiopian Birr", "", &[]),
    c("pkr", "Pakistani Rupees", "", &[]),
    c("bdt", "Bangladeshi Taka", "৳", &["৳", "taka"]),
    c("lkr", "Sri Lankan Rupees", "", &[]),
    c("npr", "Nepalese Rupees", "", &[]),
    c("thb", "Thai Baht", "฿", &["฿", "baht"]),
    c("vnd", "Vietnamese Dong", "₫", &["₫", "dong"]),
    c("idr", "Indonesian Rupiah", "", &["rupiah"]),
    c("myr", "Malaysian Ringgit", "", &["ringgit"]),
    c("php", "Philippine Pesos", "₱", &["₱"]),
    c("twd", "New Taiwan Dollars", "NT$", &["nt$"]),
    c("mmk", "Myanmar Kyat", "", &[]),
    c("khr", "Cambodian Riel", "", &[]),
    c("kzt", "Kazakhstani Tenge", "", &[]),
    c("uzs", "Uzbekistani Som", "", &[]),
    c("gel", "Georgian Lari", "", &[]),
    c("amd", "Armenian Dram", "", &[]),
    c("azn", "Azerbaijani Manat", "", &[]),
    c("byn", "Belarusian Rubles", "", &[]),
    c("rsd", "Serbian Dinars", "", &[]),
    c("ars", "Argentine Pesos", "", &[]),
    c("clp", "Chilean Pesos", "", &[]),
    c("cop", "Colombian Pesos", "", &[]),
    c("pen", "Peruvian Soles", "", &[]),
    c("uyu", "Uruguayan Pesos", "", &[]),
    c("bob", "Bolivian Bolivianos", "", &[]),
    c("pyg", "Paraguayan Guarani", "", &[]),
    c("dop", "Dominican Pesos", "", &[]),
    c("crc", "Costa Rican Colones", "", &[]),
    c("gtq", "Guatemalan Quetzales", "", &[]),
    c("jmd", "Jamaican Dollars", "", &[]),
    c("ttd", "Trinidad and Tobago Dollars", "", &[]),
    c("dzd", "Algerian Dinars", "", &[]),
    c("tnd", "Tunisian Dinars", "", &[]),
    c("iqd", "Iraqi Dinars", "", &[]),
    c("irr", "Iranian Rials", "", &[]),
    c("lbp", "Lebanese Pounds", "", &[]),
    c("mur", "Mauritian Rupees", "", &[]),
    c("xaf", "Central African CFA Francs", "", &[]),
    c("xof", "West African CFA Francs", "", &[]),
    c("mop", "Macanese Patacas", "", &[]),
    c("fjd", "Fijian Dollars", "", &[]),
    c("xau", "Gold Ounces", "", &[]),
    c("xag", "Silver Ounces", "", &[]),
    c("btc", "Bitcoin", "₿", &["₿", "bitcoin", "bitcoins"]),
    c("eth", "Ethereum", "", &["ethereum", "ether"]),
    c("sol", "Solana", "", &["solana"]),
    c("usdt", "Tether", "", &["tether"]),
    c("usdc", "USD Coin", "", &[]),
    c("bnb", "BNB", "", &[]),
    c("xrp", "XRP", "", &["ripple"]),
    c("ada", "Cardano", "", &["cardano"]),
    c("doge", "Dogecoin", "", &["dogecoin"]),
    c("ltc", "Litecoin", "", &["litecoin"]),
    c("bch", "Bitcoin Cash", "", &[]),
    c("xmr", "Monero", "", &["monero"]),
    c("xlm", "Stellar Lumens", "", &[]),
    c("avax", "Avalanche", "", &[]),
    c("trx", "TRON", "", &[]),
    c("shib", "Shiba Inu", "", &[]),
    c("matic", "Polygon", "", &[]),
];

/// Codes that are also everyday words: money only when a conversion or a sum says so.
const WORDY: &[&str] = &["try", "pen", "bob", "gel", "mop", "mad", "sol", "ron", "ada", "rs", "real", "rand", "won", "franc", "sterling", "ether"];

/// Currency names of more than one word, folded to their code before the expression is read.
pub static PHRASES: &[(&str, &str)] = &[
    ("us dollars", "usd"), ("us dollar", "usd"), ("american dollars", "usd"),
    ("australian dollars", "aud"), ("australian dollar", "aud"), ("canadian dollars", "cad"), ("canadian dollar", "cad"),
    ("hong kong dollars", "hkd"), ("singapore dollars", "sgd"), ("new zealand dollars", "nzd"), ("taiwan dollars", "twd"),
    ("british pounds", "gbp"), ("british pound", "gbp"), ("pounds sterling", "gbp"), ("pound sterling", "gbp"),
    ("indian rupees", "inr"), ("indian rupee", "inr"), ("pakistani rupees", "pkr"), ("sri lankan rupees", "lkr"), ("nepalese rupees", "npr"),
    ("japanese yen", "jpy"), ("chinese yuan", "cny"), ("swiss francs", "chf"), ("swiss franc", "chf"),
    ("south korean won", "krw"), ("korean won", "krw"), ("mexican pesos", "mxn"), ("philippine pesos", "php"),
    ("argentine pesos", "ars"), ("chilean pesos", "clp"), ("colombian pesos", "cop"),
    ("brazilian reais", "brl"), ("brazilian real", "brl"), ("south african rand", "zar"),
    ("russian rubles", "rub"), ("turkish lira", "try"), ("uae dirhams", "aed"), ("saudi riyals", "sar"),
    ("swedish krona", "sek"), ("swedish kronor", "sek"), ("norwegian krone", "nok"), ("norwegian kroner", "nok"),
    ("danish krone", "dkk"), ("danish kroner", "dkk"), ("polish zloty", "pln"), ("thai baht", "thb"),
    ("egyptian pounds", "egp"), ("israeli shekels", "ils"), ("bitcoin cash", "bch"),
];

pub fn set_waker(waker: Waker) {
    let _ = WAKER.set(waker);
}

/// True once since the last call if new rates landed.
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::SeqCst)
}

fn cache_path() -> PathBuf {
    let dir = std::env::var("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|_| util::home().join(".cache")).join("echospot");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("rates.json")
}

/// Reads either source's answer into rates per US dollar.
fn parse(body: &str) -> Option<HashMap<String, f64>> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let table = json.get("usd").or_else(|| json.get("rates"))?.as_object()?;
    let rates: HashMap<String, f64> = table.iter()
        .filter_map(|(code, rate)| Some((code.to_lowercase(), rate.as_f64().filter(|r| *r > 0.0 && r.is_finite())?)))
        .collect();
    (rates.len() > 20 && rates.contains_key("eur")).then_some(rates)
}

fn fetch() -> Option<HashMap<String, f64>> {
    SOURCES.iter().find_map(|url| parse(&util::output("curl", &["-sL", "--max-time", "12", url])?))
}

/// Starts a refresh when the rates are missing or stale. Safe to call on every keystroke.
pub fn warm() {
    let mut guard = RATES.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        let cached: Rates = util::read_json(&cache_path());
        *guard = Some(cached);
    }
    let fetched = guard.as_ref().map_or(0, |r| r.fetched);
    drop(guard);
    let now = util::now_secs();
    if now.saturating_sub(fetched) < FRESH_SECS || now.saturating_sub(LAST_TRY.load(Ordering::SeqCst)) < RETRY_SECS || cfg!(test) {
        return;
    }
    if FETCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    LAST_TRY.store(now, Ordering::SeqCst);
    std::thread::spawn(|| {
        if let Some(rates) = fetch() {
            let fresh = Rates { fetched: util::now_secs(), rates };
            util::write_json(&cache_path(), &fresh);
            *RATES.lock().unwrap_or_else(|e| e.into_inner()) = Some(fresh);
            DIRTY.store(true, Ordering::SeqCst);
            if let Some(waker) = WAKER.get() {
                waker();
            }
        }
        FETCHING.store(false, Ordering::SeqCst);
    });
}

pub fn fetching() -> bool {
    FETCHING.load(Ordering::SeqCst)
}

/// Units of a currency one US dollar buys.
fn rate(code: &str) -> Option<f64> {
    warm();
    RATES.lock().ok()?.as_ref()?.rates.get(code).copied()
}

#[cfg(test)]
pub fn set_test_rates() {
    let rates = [("usd", 1.0), ("eur", 0.8), ("gbp", 0.5), ("inr", 80.0), ("jpy", 150.0), ("btc", 0.00001), ("cad", 1.25)];
    *RATES.lock().unwrap() = Some(Rates { fetched: util::now_secs(), rates: rates.iter().map(|(k, v)| (k.to_string(), *v)).collect() });
    *HOME.lock().unwrap() = Some("usd".into());
}

fn def(name: &str) -> Option<&'static Def> {
    let key = name.trim().to_lowercase();
    CURRENCIES.iter().find(|d| d.code == key || d.names.contains(&key.as_str()))
}

fn unit_of(d: &Def) -> Option<Unit> {
    let rate = rate(d.code)?;
    let (symbol, leads) = if d.symbol.is_empty() { (d.code.to_uppercase(), false) } else { (d.symbol.to_string(), true) };
    Some(Unit { label: d.label.into(), symbol, kind: CURRENCY, factor: 1.0 / rate, offset: 0.0, inverse: false, leads })
}

/// The currency a code, symbol or name means, once there is a rate for it.
pub fn find(name: &str) -> Option<Unit> {
    unit_of(def(name)?)
}

/// Whether a name is one of the short codes that is also an ordinary word.
pub fn wordy(name: &str) -> bool {
    WORDY.contains(&name.to_lowercase().as_str())
}

pub fn is_symbol(c: char) -> bool {
    matches!(c, '$' | '€' | '£' | '¥' | '₹' | '₩' | '₽' | '₺' | '₪' | '₫' | '₱' | '₦' | '₴' | '฿' | '₿' | '৳')
}

/// Settings can name the currency a bare amount converts to; empty means the system's.
pub fn set_home(code: &str) {
    let code = code.trim().to_lowercase();
    *HOME.lock().unwrap_or_else(|e| e.into_inner()) = def(&code).map(|d| d.code.to_string());
}

/// The currency a bare amount converts to: the setting, else the one spent where this machine's
/// clock says it is, else the locale's.
fn home_code() -> String {
    let mut guard = HOME.lock().unwrap_or_else(|e| e.into_inner());
    guard.get_or_insert_with(|| by_zone().or_else(by_locale).unwrap_or("usd").to_string()).clone()
}

/// The unit a bare amount of `from` is shown in: the home currency, or dollars (euros) when the
/// amount already is the home currency.
pub fn counterpart(from: &Unit) -> Option<Unit> {
    let home = find(&home_code())?;
    if home.label != from.label {
        return Some(home);
    }
    find(if home.label == "US Dollars" { "eur" } else { "usd" })
}

const EUROZONE: &[&str] = &["AT", "BE", "BG", "CY", "DE", "EE", "ES", "FI", "FR", "GR", "HR", "IE", "IT", "LT", "LU", "LV", "MT", "NL", "PT", "SI", "SK"];

const COUNTRIES: &[(&str, &str)] = &[
    ("US", "usd"), ("GB", "gbp"), ("IN", "inr"), ("JP", "jpy"), ("CN", "cny"), ("AU", "aud"), ("CA", "cad"), ("CH", "chf"), ("HK", "hkd"),
    ("SG", "sgd"), ("NZ", "nzd"), ("KR", "krw"), ("MX", "mxn"), ("BR", "brl"), ("RU", "rub"), ("TR", "try"), ("ZA", "zar"), ("SE", "sek"),
    ("NO", "nok"), ("DK", "dkk"), ("PL", "pln"), ("CZ", "czk"), ("HU", "huf"), ("RO", "ron"), ("IS", "isk"), ("UA", "uah"), ("IL", "ils"),
    ("AE", "aed"), ("SA", "sar"), ("QA", "qar"), ("KW", "kwd"), ("EG", "egp"), ("NG", "ngn"), ("KE", "kes"), ("PK", "pkr"), ("BD", "bdt"),
    ("LK", "lkr"), ("NP", "npr"), ("TH", "thb"), ("VN", "vnd"), ("ID", "idr"), ("MY", "myr"), ("PH", "php"), ("TW", "twd"), ("AR", "ars"),
    ("CL", "clp"), ("CO", "cop"), ("PE", "pen"), ("MA", "mad"), ("GH", "ghs"),
];

fn by_country(country: &str) -> Option<&'static str> {
    if EUROZONE.contains(&country) {
        return Some("eur");
    }
    COUNTRIES.iter().find(|(c, _)| *c == country).map(|(_, code)| *code)
}

fn by_zone() -> Option<&'static str> {
    let zone = super::zones::local_name()?;
    let table = std::fs::read_to_string("/usr/share/zoneinfo/zone1970.tab").ok()?;
    let country = table.lines().filter(|l| !l.starts_with('#')).find_map(|line| {
        let mut cols = line.split('\t');
        let (countries, _, name) = (cols.next()?, cols.next()?, cols.next()?);
        (name == zone).then(|| countries.split(',').next().unwrap_or("").to_string())
    })?;
    by_country(&country)
}

fn by_locale() -> Option<&'static str> {
    let locale = ["LC_ALL", "LC_MONETARY", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))?;
    let country = locale.split('.').next()?.split('_').nth(1)?;
    by_country(country)
}

/// Money as people write it: two decimals, or enough digits to show a small coin's worth.
pub fn format(n: f64, unit: &Unit) -> String {
    let number = if n == 0.0 || n.abs() >= 0.995 { super::fixed(n, 2) } else { super::format_sig(n, 4) };
    match (unit.leads, number.strip_prefix('-')) {
        (true, Some(rest)) => format!("-{}{}", unit.symbol, rest),
        (true, None) => format!("{}{}", unit.symbol, number),
        _ => format!("{} {}", number, unit.symbol),
    }
}
