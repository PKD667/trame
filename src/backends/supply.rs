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

/// The bytes of memory this host has free, as the launch reads them for its workers' share (INTERFACE.md 6,
/// Definition (Memory of a host)): `TRAME_AVAILABLE` when the launcher declared a number, else the smaller of the
/// cgroup's headroom (`memory.max` less `memory.current`, cgroup v2) and the machine's `MemAvailable`. `None` when
/// neither can be read.
pub(crate) fn available() -> Option<u64> {
    if let Ok(text) = std::env::var("TRAME_AVAILABLE") {
        return Some(
            text.trim()
                .parse::<u64>()
                .unwrap_or_else(|_| panic!("trame: TRAME_AVAILABLE must be a number of bytes, got {text:?}")),
        );
    }
    let cgroup = cgroup_v2().and_then(|dir| {
        let max = std::fs::read_to_string(dir.join("memory.max")).ok()?;
        let max: u64 = max.trim().parse().ok()?; // "max": no limit set
        let used: u64 = std::fs::read_to_string(dir.join("memory.current")).ok()?.trim().parse().ok()?;
        Some(max.saturating_sub(used))
    });
    let machine = mem_available();
    match (cgroup, machine) {
        (Some(c), Some(m)) => Some(c.min(m)),
        (c, m) => c.or(m),
    }
}

/// The cgroup v2 directory of this process: from `/proc/self/cgroup` (`0::/path`), under `/sys/fs/cgroup`.
fn cgroup_v2() -> Option<std::path::PathBuf> {
    let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let path = text.lines().find_map(|l| l.strip_prefix("0::"))?.trim();
    Some(std::path::Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/')))
}

/// `MemAvailable` of `/proc/meminfo`, in bytes.
fn mem_available() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = text
        .lines()
        .find_map(|l| l.strip_prefix("MemAvailable:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()?;
    Some(kb * 1024)
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
