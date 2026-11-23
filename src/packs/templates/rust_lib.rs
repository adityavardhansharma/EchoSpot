//! __TITLE__: abilities in Rust, compiled to a WebAssembly component (API-DESIGN.md §20).
//! `echo.rs`, `calls.rs` and `wit/` are EchoSpot's and are rewritten before each build; this
//! file is yours.

mod calls;
mod echo;

use echo::{Ability, Cx, Pack, Result, t};
use serde_json::{Value, json};

pub fn pack() -> Pack {
    Pack::new("__TITLE__")
        .ability("nth", Ability::read("Nth Prime")
            .description("The nth prime number, worked out in Rust")
            .icon("icons/prime.svg")
            .input([("n", t::number().describe("Which prime"))])
            .output(t::number())
            .examples(["nth prime 1000"])
            .run(nth))
        .ability("primes", Ability::read("Primes Up To")
            .icon("icons/prime.svg")
            .input([("limit", t::number().default(100))])
            .output(t::list(t::number()))
            .hidden(true)
            .run(primes))
}

fn whole(input: &Value, field: &str) -> Result<u64> {
    match input[field].as_f64() {
        Some(n) if n >= 1.0 && n.fract() == 0.0 && n <= 10_000_000.0 => Ok(n as u64),
        _ => Err(echo::Error::new("E0301", format!("{field} must be a whole number from 1 to 10,000,000"))),
    }
}

/// Every prime up to `limit`, by the sieve of Eratosthenes. The arithmetic is u64 on purpose:
/// a component is 32-bit, so `usize` overflows at 4 billion, and `i * i` gets there quickly.
fn sieve(limit: u64) -> Vec<u64> {
    let mut composite = vec![false; limit as usize + 1];
    let mut out = Vec::new();
    for i in 2..=limit {
        if !composite[i as usize] {
            out.push(i);
            let mut multiple = i * i;
            while multiple <= limit {
                composite[multiple as usize] = true;
                multiple += i;
            }
        }
    }
    out
}

fn nth(input: Value, _cx: &Cx) -> Result {
    let n = whole(&input, "n")?;
    // The nth prime is below n(ln n + ln ln n) for n ≥ 6.
    let x = n.max(6) as f64;
    let bound = (x * (x.ln() + x.ln().ln())).ceil() as u64;
    Ok(json!(sieve(bound)[n as usize - 1]))
}

fn primes(input: Value, _cx: &Cx) -> Result {
    Ok(json!(sieve(whole(&input, "limit")?)))
}
