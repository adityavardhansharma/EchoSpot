//! Raw effects: `ui.fx { fire = "heat" }`. rune-fx knows what each effect takes and what it
//! costs, so the host checks a pack's effects before they run and holds each surface to a
//! budget. Over it, the cheapest to drop freeze to a still frame instead of running.

use rune_fx::{AuroraParams, DistortParams, ElectricParams, FireParams, Fx, GlowParams, GrainParams, PulseParams, ShimmerParams, SparkleParams};
use serde_json::Value;

use super::blocks;
use super::error::PackError;

/// Running effect layers a surface may have, unless its Look says otherwise.
pub const BUDGET: usize = 3;

/// Every effect a pack may name, and the presets each takes.
pub const EFFECTS: &[(&str, &[&str])] = &[
    ("glow", &["default"]), ("pulse", &["default"]), ("electric", &["default", "charged"]), ("fire", &["default", "heat", "plasma"]),
    ("distort", &["crt"]), ("shimmer", &["default"]), ("sparkles", &["default"]), ("aurora", &["default"]), ("grain", &["default"]),
];

/// The effect a name and a preset mean.
pub fn build(name: &str, preset: &str) -> Option<Fx> {
    Some(match (name, preset) {
        ("glow", _) => Fx::glow(GlowParams::default()),
        ("pulse", _) => Fx::pulse(PulseParams::default()),
        ("electric", "charged") => Fx::electric(ElectricParams::charged()),
        ("electric", _) => Fx::electric(ElectricParams::default()),
        ("fire", "heat") => Fx::fire(FireParams::heat()),
        ("fire", "plasma") => Fx::fire(FireParams::plasma()),
        ("fire", _) => Fx::fire(FireParams::default()),
        ("distort", _) => Fx::distort(DistortParams::crt()),
        ("shimmer", _) => Fx::shimmer(ShimmerParams::default()),
        ("sparkles", _) => Fx::sparkles(SparkleParams::default()),
        ("aurora", _) => Fx::aurora(AuroraParams::default()),
        ("grain", _) => Fx::grain(GrainParams::default()),
        _ => return None,
    })
}

/// One effect a surface asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub preset: String,
    /// What it costs per covered pixel: texture taps plus noise evaluations, weighted.
    pub cost: u32,
    /// Where in the tree it sits: `List › row 2`.
    pub at: String,
    /// Frozen to a still frame: the surface was over its budget.
    pub frozen: bool,
}

fn cost(fx: &Fx) -> u32 {
    let c = fx.cost();
    c.samples + c.alu * 4 + u32::from(c.backdrop) * 8 + u32::from(c.source) * 4
}

/// Checks one `effects` table: `{ fire = "heat" }`, `{ glow = true }`.
pub fn parse(effects: &Value, at: &str) -> Result<Vec<Layer>, PackError> {
    let Some(map) = effects.as_object() else {
        return Err(PackError::new("E0700", "ui.fx takes a table of effects: ui.fx({ fire = \"heat\" }, child)").at(at).learn("echospot dev docs design"));
    };
    let mut out = Vec::new();
    for (name, value) in map {
        let Some((_, presets)) = EFFECTS.iter().find(|(n, _)| n == name) else {
            return Err(PackError::new("E0700", format!("no effect {name:?}")).at(at).nearest(name, EFFECTS.iter().map(|(n, _)| *n)).learn("echospot dev docs design"));
        };
        let preset = value.as_str().unwrap_or("default");
        if !presets.contains(&preset) && preset != "default" {
            return Err(PackError::new("E0700", format!("{name} has no preset {preset:?}; it takes {}", presets.join(", "))).at(at).nearest(preset, presets.iter().copied()).learn("echospot dev docs design"));
        }
        let fx = build(name, preset).expect("named in EFFECTS");
        out.push(Layer { name: name.clone(), preset: preset.to_string(), cost: cost(&fx), at: at.to_string(), frozen: false });
    }
    Ok(out)
}

/// Every effect layer a tree of blocks would run: raw `ui.fx`, and the effects its intents
/// bring under `look`. The costliest past `budget` are marked frozen.
pub fn layers(root: &Value, look: &super::looks::Look, budget: usize) -> (Vec<Layer>, Vec<PackError>) {
    let (mut found, mut errors) = (Vec::new(), Vec::new());
    blocks::walk(root, &mut |block, _| {
        let kind = blocks::kind(block).to_string();
        if kind == "fx" {
            match parse(block.get("effects").unwrap_or(&Value::Null), "ui.fx") {
                Ok(layers) => found.extend(layers),
                Err(e) => errors.push(e),
            }
        }
        if let Some(effects) = block.get("fx").filter(|v| v.is_object()) {
            match parse(effects, &kind) {
                Ok(layers) => found.extend(layers),
                Err(e) => errors.push(e),
            }
        }
        let mut intent = |intent: &str, at: String| {
            let effect = look.intent(intent).effect;
            if let Some(fx) = build(effect, "default") {
                found.push(Layer { name: effect.to_string(), preset: "default".into(), cost: cost(&fx), at, frozen: false });
            }
        };
        intent(blocks::text(block, "intent"), kind.clone());
        for (i, row) in blocks::items(block, "rows").iter().enumerate() {
            intent(blocks::text(row, "intent"), format!("{kind} › row {}", i + 1));
        }
    });
    // The first asked for keep running; what is over the budget holds still, costliest first.
    if found.len() > budget {
        let mut order: Vec<usize> = (0..found.len()).collect();
        order.sort_by_key(|i| std::cmp::Reverse(found[*i].cost));
        for i in order.into_iter().take(found.len() - budget) {
            found[i].frozen = true;
        }
    }
    (found, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::looks;
    use serde_json::json;

    #[test]
    fn effects_are_checked_and_held_to_a_budget() {
        assert_eq!(parse(&json!({ "fire": "heat" }), "x").unwrap()[0].preset, "heat");
        let e = parse(&json!({ "flames": true }), "x").unwrap_err();
        assert_eq!((e.code, e.nearest.is_empty()), ("E0700", true));
        assert_eq!(parse(&json!({ "fier": true }), "x").unwrap_err().nearest[0], "fire");
        assert_eq!(parse(&json!({ "fire": "hot" }), "x").unwrap_err().nearest[0], "heat");
        let list = json!({ "_ui": "List", "rows": [{ "intent": "urgent" }, { "intent": "urgent" }, { "intent": "live" }, { "intent": "success" }, { "intent": "muted" }] });
        let (layers, errors) = layers(&list, looks::get("glass"), BUDGET);
        assert!(errors.is_empty());
        assert_eq!((layers.len(), layers.iter().filter(|l| l.frozen).count()), (4, 1));
        // The flat Look brings no effects at all.
        assert!(super::layers(&list, looks::get("ink"), 1).0.is_empty());
    }
}
