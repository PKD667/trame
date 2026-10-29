//! The data the rules read: the effect table and the allow-lists and denies (driver/lists/).
//! Compiled in with `include_str!`, so the driver binary is self-contained.
//!
//! Matching (driver/lists/deny.txt says it for readers): an entry is a path, or `P::*` for every
//! item under `P`. Of all the entries that match a callee's names, the most specific decides. An
//! exact entry is more specific than a prefix, and a deny wins a tie.

use std::collections::{BTreeMap, BTreeSet};

use super::Level;

const EFFECTS: &str = include_str!("../../lists/effects.txt");
const DENY: &str = include_str!("../../lists/deny.txt");
const A_I: &str = include_str!("../../lists/A_I.txt");
const A_C: &str = include_str!("../../lists/A_C.txt");
const A_W: &str = include_str!("../../lists/A_W.txt");

/// One effect of spec.md's Sigma sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    Send,
    Recv,
    Now,
    Parallel,
    Concurrent,
    Barrier,
}

impl Effect {
    fn parse(name: &str) -> Option<Effect> {
        Some(match name {
            "send" => Effect::Send,
            "recv" => Effect::Recv,
            "now" => Effect::Now,
            "parallel" => Effect::Parallel,
            "concurrent" => Effect::Concurrent,
            "barrier" => Effect::Barrier,
            _ => return None,
        })
    }

    /// Whether `level` may perform this effect (Sigma_I = {}, Sigma_C, Sigma_W).
    pub fn allowed_at(self, level: Level) -> bool {
        match level {
            Level::Item => false,
            Level::Step => matches!(self, Effect::Send | Effect::Recv | Effect::Now | Effect::Parallel),
            Level::Worker => true,
        }
    }
}

/// What the lists say about a callee at one level.
pub enum Verdict {
    /// An allow entry matches, and no deny is at least as specific.
    Allow,
    /// A deny entry matches (the entry is named, for the message).
    Deny(String),
    /// No entry matches: the callee is not listed at this level.
    Refuse,
}

pub struct Lists {
    effects: BTreeMap<String, Effect>,
    /// The entries of deny.txt: denied at every level, cited as Requirement (All levels).
    global: BTreeSet<String>,
    /// Per level (indexed by `Level as usize`): the allowed entries.
    allow: [BTreeSet<String>; 3],
    /// Per level: the denies of deny.txt and of that level's file (`-` lines).
    deny: [BTreeSet<String>; 3],
}

/// The entries of one list file: `(is_deny, entry)`. `#` starts a comment; `-` marks a deny.
fn entries(text: &str) -> impl Iterator<Item = (bool, &str)> {
    text.lines().filter_map(|line| {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            return None;
        }
        Some(match line.strip_prefix('-') {
            Some(rest) => (true, rest.trim()),
            None => (false, line),
        })
    })
}

/// A `*` is allowed only as the last `::*` of an entry.
fn well_formed(entry: &str) -> bool {
    match entry.find('*') {
        None => true,
        Some(i) => i + 1 == entry.len() && entry.ends_with("::*") && entry.len() > 3,
    }
}

/// The specificity of `entry` matching `name`, if it does: twice the length of the matched text, plus
/// one for an exact entry, so `P` beats `P::*` and both lose to any longer match.
fn matches(entry: &str, name: &str) -> Option<usize> {
    match entry.strip_suffix("::*") {
        Some(base) => {
            let under = name.strip_prefix(base).is_some_and(|rest| rest.starts_with("::"));
            (under || name == base).then_some(2 * base.len())
        }
        None => (name == entry).then_some(2 * entry.len() + 1),
    }
}

/// The most specific entry of `set` that matches one of `names`: (specificity, entry).
fn best(set: &BTreeSet<String>, names: &[String]) -> Option<(usize, String)> {
    let mut best: Option<(usize, String)> = None;
    for entry in set {
        for name in names {
            if let Some(s) = matches(entry, name)
                && best.as_ref().is_none_or(|(b, _)| s > *b)
            {
                best = Some((s, entry.clone()));
            }
        }
    }
    best
}

impl Lists {
    /// Parse the bundled files. Fails loudly if they are malformed, if an allow is also denied,
    /// or if the levels do not nest: A_I <= A_C <= A_W as allows, and denies grow as the level
    /// gets stricter (Item denies at least what Step denies, Step at least what Worker denies).
    pub fn load() -> Result<Lists, String> {
        let mut effects = BTreeMap::new();
        for line in EFFECTS.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (name, path) = line.split_once(' ').ok_or(format!("effects.txt: bad line `{line}`"))?;
            let effect = Effect::parse(name).ok_or(format!("effects.txt: unknown effect `{name}`"))?;
            effects.insert(path.trim().to_string(), effect);
        }

        let mut allow: [BTreeSet<String>; 3] = Default::default();
        let mut deny: [BTreeSet<String>; 3] = Default::default();
        let mut global = BTreeSet::new();
        for (is_deny, entry) in entries(DENY) {
            if is_deny || !well_formed(entry) {
                return Err(format!("deny.txt: bad entry `{entry}`"));
            }
            global.insert(entry.to_string());
        }
        let global_entries = global.clone();
        for (i, (name, text)) in [("A_I", A_I), ("A_C", A_C), ("A_W", A_W)].into_iter().enumerate() {
            for (is_deny, entry) in entries(text) {
                if !well_formed(entry) {
                    return Err(format!("{name}.txt: bad entry `{entry}`"));
                }
                let set = if is_deny { &mut deny[i] } else { &mut allow[i] };
                if !set.insert(entry.to_string()) {
                    return Err(format!("{name}.txt: `{entry}` listed twice"));
                }
            }
            deny[i].extend(global.iter().cloned());
        }

        for i in 0..3 {
            if let Some(dead) = allow[i].intersection(&deny[i]).next() {
                return Err(format!("A_{} allows `{dead}` and denies it", ["I", "C", "W"][i]));
            }
        }
        for (small, big) in [(0, 1), (1, 2)] {
            if let Some(m) = allow[small].difference(&allow[big]).next() {
                return Err(format!("allow `{m}` is not in level {}; A_I <= A_C <= A_W must nest", big));
            }
        }
        for (strict, loose) in [(0, 1), (1, 2)] {
            if let Some(m) = deny[loose].difference(&deny[strict]).next() {
                return Err(format!("deny `{m}` of level {} is not denied at the stricter level {}", loose, strict));
            }
        }
        Ok(Lists { effects, global: global_entries, allow, deny })
    }

    pub fn effect(&self, path: &str) -> Option<Effect> {
        self.effects.get(path).copied()
    }

    /// The verdict at `level` for a callee with these names (its path, its self-type shape, ...).
    pub fn verdict(&self, level: Level, names: &[String]) -> Verdict {
        let l = level as usize;
        match (best(&self.deny[l], names), best(&self.allow[l], names)) {
            (Some((d, entry)), Some((a, _))) if d >= a => Verdict::Deny(entry),
            (Some((_, entry)), None) => Verdict::Deny(entry),
            (_, Some(_)) => Verdict::Allow,
            (None, None) => Verdict::Refuse,
        }
    }

    /// Whether `entry` is one of deny.txt's (denied at every level) rather than a per-level deny.
    pub fn is_global(&self, entry: &str) -> bool {
        self.global.contains(entry)
    }

    /// The deny entry, if any, that names the type or path `name` at `level`.
    pub fn denied(&self, level: Level, name: &str) -> Option<String> {
        best(&self.deny[level as usize], &[name.to_string()]).map(|(_, e)| e)
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn an_exact_entry_beats_the_prefix_of_the_same_text() {
        assert!(matches("core::ptr", "core::ptr") > matches("core::ptr::*", "core::ptr"));
    }

    #[test]
    fn a_longer_match_beats_an_exact_shorter_one() {
        let name = "core::ptr::read::x";
        assert_eq!(matches("core::ptr", name), None);
        assert!(matches("core::ptr::read::*", name) > matches("core::ptr::*", name));
    }

    #[test]
    fn a_prefix_does_not_match_a_sibling_with_the_same_start() {
        assert_eq!(matches("core::ptr::*", "core::ptrx"), None);
    }
}
