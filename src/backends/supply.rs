//! The workers each host is supplied (INTERFACE.md 6, `trame::workers`). The launcher decides them and
//! passes TRAME_WORKERS, the counts of hosts 0, 1, ... separated by commas; a program only reads them.
//! Read at every call, so a count answers before the program declares anything. A deployment of one host
//! that was not given the variable answers its usable cores.

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

/// The workers host `h` of `hosts` is supplied. Panics when `h` is not a host, or when the counts
/// are not those of this deployment's hosts.
pub(crate) fn workers(h: u32, hosts: u32) -> u32 {
    assert!(h < hosts, "trame: workers(HostId({h})): the deployment has {hosts} hosts");
    let Ok(text) = std::env::var("TRAME_WORKERS") else {
        assert!(
            hosts == 1,
            "trame: workers(HostId({h})): TRAME_WORKERS is not set; start the program through its backend's launch"
        );
        return std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    };
    let counts = parse(&text).unwrap_or_else(|e| panic!("trame: {e}"));
    assert!(
        counts.len() == hosts as usize,
        "trame: TRAME_WORKERS names {} hosts and the deployment has {hosts}",
        counts.len()
    );
    counts[h as usize]
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
