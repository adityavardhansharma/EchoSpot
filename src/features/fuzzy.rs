//! Search relevance: tiers first (exact, prefix, word start, acronym, substring, subsequence),
//! then a score inside the tier, so a short query lands on what it most plainly names.

/// Higher is better; `None` is no match. Every whitespace-separated term must match.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let text_l = text.to_lowercase();
    let mut total = 0;
    let mut any = false;
    for term in query.split_whitespace() {
        total += term_score(&term.to_lowercase(), &text_l)?;
        any = true;
    }
    any.then_some(total)
}

fn term_score(q: &str, t: &str) -> Option<i32> {
    if q.is_empty() {
        return Some(0);
    }
    let len_penalty = (t.chars().count() as i32).min(60);
    if t == q {
        return Some(1000);
    }
    if t.starts_with(q) {
        return Some(900 - len_penalty);
    }
    // A word inside the text starting with the query.
    let mut at_word = true;
    for (i, c) in t.char_indices() {
        if at_word && t[i..].starts_with(q) {
            return Some(780 - len_penalty - (i as i32).min(40));
        }
        at_word = !c.is_alphanumeric();
    }
    // Acronym: the first letter of each word.
    let initials: String = t.split(|c: char| !c.is_alphanumeric()).filter_map(|w| w.chars().next()).collect();
    if initials.starts_with(q) && q.chars().count() > 1 {
        return Some(700 - len_penalty);
    }
    if let Some(i) = t.find(q) {
        return Some(560 - len_penalty - (i as i32).min(60));
    }
    // Subsequence, rewarding runs and early hits. The letters must sit close together and inside
    // one word: a query scattered across a name, or pieced together from two words ("fire" out
    // of "wi-fi wireless"), is noise, not a match.
    t.split(|c: char| !c.is_alphanumeric()).filter_map(|word| subsequence(q, word)).max().map(|s| (s - len_penalty).max(1))
}

fn subsequence(q: &str, word: &str) -> Option<i32> {
    let (mut score, mut run, mut last, mut first) = (300, 0, None::<usize>, None::<usize>);
    let mut chars = word.char_indices();
    for qc in q.chars() {
        let (i, _) = chars.by_ref().find(|&(_, c)| c == qc)?;
        first.get_or_insert(i);
        if last.is_some_and(|l| l + 1 == i) {
            run += 1;
            score += 6 * run;
        } else {
            run = 0;
            score -= 4;
        }
        last = Some(i);
    }
    let span = last? - first? + 1;
    let n = q.chars().count();
    (n >= 3 && span <= n * 2).then_some(score)
}

/// The best score over a row's title and its quieter keywords, which count for less.
pub fn score_item(query: &str, title: &str, keywords: &str) -> Option<i32> {
    let by_title = score(query, title);
    let by_keys = if keywords.is_empty() { None } else { score(query, keywords).map(|s| s / 2) };
    by_title.max(by_keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_rank_in_order() {
        let exact = score("code", "code").unwrap();
        let prefix = score("code", "codex").unwrap();
        let word = score("code", "vs code").unwrap();
        let inside = score("ode", "vs code").unwrap();
        assert!(exact > prefix && prefix > word && word > inside);
    }

    #[test]
    fn acronyms_match() {
        assert!(score("vsc", "visual studio code").is_some());
    }

    #[test]
    fn subsequence_stays_inside_one_word() {
        assert!(score("frfx", "firefox").is_some());
        assert!(score("fire", "wifi wireless network").is_none());
    }

    #[test]
    fn every_term_must_match() {
        assert!(score("toggle wifi", "toggle wi-fi wifi").is_some());
        assert!(score("toggle moon", "toggle wi-fi").is_none());
    }

    #[test]
    fn keywords_count_for_less_than_the_title() {
        let by_title = score_item("mute", "Toggle Mute", "").unwrap();
        let by_keys = score_item("mute", "Silence", "mute").unwrap();
        assert!(by_title > by_keys);
    }
}
