//! The workers a launcher supplied to each host (INTERFACE.md 6, `trame::workers`). The launcher decides them and
//! passes TRAME_WORKERS, the counts of hosts 0, 1, ... separated by commas. A backend answers from it only until
//! its hosts have discovered each other; after that it answers what the hosts hold.

extern crate std;

use std::string::String;
use std::vec::Vec;

/// The counts in `text`, one per host: each a number of 1 or more.
pub(crate) fn parse(text: &str) -> Result<Vec<u32>, String> {
    let mut counts = Vec::new();
    for part in text.split(',') {
        match part.trim().parse::<u32>() {
            Ok(n) if n >= 1 => counts.push(n),
            _ => {
                return Err(std::format!(
                    "TRAME_WORKERS must list a worker count of 1 or more for each host, got {text:?}"
                ));
            }
        }
    }
    Ok(counts)
}

/// The workers the launcher supplied for host `h`: `None` when TRAME_WORKERS is not set, or names no host `h`.
/// Panics when the variable is set and is not a list of counts.
pub(crate) fn supplied(h: u32) -> Option<u32> {
    let text = std::env::var("TRAME_WORKERS").ok()?;
    let counts = parse(&text).unwrap_or_else(|e| panic!("trame: {e}"));
    counts.get(h as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_a_list() {
        assert_eq!(parse("4,2,8"), Ok(std::vec![4, 2, 8]));
        assert_eq!(parse(" 3 "), Ok(std::vec![3]));
    }

    #[test]
    fn refuses_what_is_not_a_count() {
        for bad in ["", "0", "2,,3", "a", "-1", "2,0"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
