//! `calc.*`: the calculator, free to every pack.

use serde_json::{Value, json};

use super::{Done, Gate, Run, Spec, missing, num, text};
use crate::features::calc::{self, currency};
use crate::packs::error::PackError;
use crate::packs::index::Kind;

/// Waits for the day's exchange rates when a conversion needs them.
fn with_rates<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
    currency::warm();
    for _ in 0..40 {
        if let Some(out) = f() {
            return Some(out);
        }
        if !currency::fetching() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    f()
}

fn eval(expression: &str) -> Run {
    if expression.trim().is_empty() {
        return Run::err(missing("expression"));
    }
    let expression = expression.to_string();
    Run::Later(Box::new(move |_| match with_rates(|| calc::evaluate(&expression)) {
        Some(card) => Done::value(json!({ "text": card.right, "label": card.right_badge, "number": calc::plain(&card.right).parse::<f64>().ok() })),
        None => Err(PackError::new("E0301", format!("{expression:?} isn't something to compute")).learn("echospot calc \"2 + 2\"")),
    }))
}

pub fn register(out: &mut Vec<Spec>) {
    out.push(Spec::new("calc.eval", Kind::Read, Gate::Free, "Evaluate an expression: arithmetic, units, percentages", "expression:text", "record", |_, i| eval(text(i, "expression"))));
    out.push(Spec::new("calc.date", Kind::Read, Gate::Free, "Date arithmetic: \"days until 25 dec\", \"3 weeks from now\"", "expression:text", "record", |_, i| eval(text(i, "expression"))));
    out.push(Spec::new("calc.convert", Kind::Read, Gate::Free, "Convert an amount between currencies or units", "amount:number, from:text, to:text", "any", |_, i| {
        let (Some(amount), from, to) = (num(i, "amount"), text(i, "from").to_string(), text(i, "to").to_string()) else { return Run::err(missing("amount")) };
        if from.is_empty() || to.is_empty() {
            return Run::err(missing("from or to"));
        }
        Run::Later(Box::new(move |_| {
            // Two currencies give money; anything else is a quantity in the unit asked for.
            if let Some((a, b)) = with_rates(|| Some((currency::find(&from)?, currency::find(&to)?))) {
                let code = if b.symbol.chars().all(|c| c.is_ascii_alphabetic()) && b.symbol.len() == 3 { b.symbol.clone() } else { to.to_uppercase() };
                let code = if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) { code } else { b.label.clone() };
                return Done::value(json!({ "amount": (b.from_base(a.to_base(amount)) * 100.0).round() / 100.0, "currency": code }));
            }
            match calc::evaluate(&format!("{amount} {from} to {to}")) {
                Some(card) => match calc::plain(&card.right).parse::<f64>() {
                    Ok(value) => Done::value(json!({ "value": value, "unit": card.right_badge })),
                    Err(_) => Done::value(Value::String(card.right)),
                },
                None => Err(PackError::new("E0301", format!("can't convert {from} to {to}"))),
            }
        }))
    }));
}
