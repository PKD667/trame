// Compute each participant's byte range in a published segment.
//
// The partition rule is pure and lives here, once, because it is the one thing the origin and
// every participant must agree on without communicating: `slice` is called independently by each
// and the answers have to tile the segment exactly. What differs between backends is only the
// alignment, and that is a parameter rather than a compile-time guess — a host mapping wants page
// alignment because mprotect works in pages, while a device launch has no pages and wants none,
// and a rule that hard-coded one of them would silently penalise the other.

use crate::contract::Rank;

/// `(offset, length)` for `rank` in a `total`-byte segment, on its sharing domain.
///
/// This is the *published* rule, and it is the only way to obtain a domain's geometry from
/// outside that domain. `slice` is not an alternative to it: `slice` answers for the domain the
/// caller's context names, so a participant that is not a member of the domain it is asking about
/// gets a well-formed answer about the wrong one, and no backend can detect that. A reader such as
/// a sink therefore calls this with the domain stated, which is what its parameters are for.
///
/// `hosts` maps a rank to the host it lives on and `domain` lists the members of the sharing
/// domain being described; both are stated rather than inferred, for the same reason. `align` is
/// the backend's, because a host mapping works in pages and a device launch has none.
///
/// Ranges follow ascending cohort order, are `align`-aligned except the last, and tile
/// `[0, total)` exactly. A rank outside the cohort, or a domain smaller than one, is a caller
/// error: the spec makes `rank` dense in the cohort, so this cannot be reached from a correct
/// caller and there is no meaningful range to return.
pub fn slice_of(
    hosts: &[Rank],
    cohort: &[Rank],
    rank: Rank,
    total: usize,
    align: usize,
) -> (usize, usize) {
    let leader = |r: Rank| hosts.get(r as usize).copied().unwrap_or(r);
    let mine = leader(rank);
    let node_rank = cohort
        .iter()
        .filter(|&&r| leader(r) == mine)
        .position(|&r| r == rank)
        .unwrap_or_else(|| panic!("rank {rank} is outside the cohort"));
    let node_size = cohort.iter().filter(|&&r| leader(r) == mine).count();
    partition(total, node_rank, node_size, align)
}

/// Partition `[0, total)` among `node_size` members of one sharing domain by position in it.
///
/// The first `node_size - 1` members take equal `align`-aligned shares and the last takes the
/// remainder, so every byte belongs to exactly one member and no member's share is empty when the
/// segment is large enough to go round. A segment too small to give every member an aligned share
/// goes entirely to the first member: a partial page is not shareable, and splitting below the
/// alignment would produce ranges the mapping cannot express.
pub fn partition(total: usize, node_rank: usize, node_size: usize, align: usize) -> (usize, usize) {
    assert!(node_rank < node_size, "shared rank is outside its node");
    assert!(node_size > 0, "a sharing domain has at least one member");
    let align = align.max(1);
    if total < align.saturating_mul(node_size) {
        return if node_rank == 0 {
            (0, total)
        } else {
            (total, 0)
        };
    }
    let share = (total / align / node_size) * align;
    let offset = share * node_rank;
    let length = if node_rank + 1 == node_size {
        total - offset
    } else {
        share
    };
    (offset, length)
}

/// The alignment a host mapping needs: one page, or one byte when the host will not say.
///
/// Not part of the portable surface: a device backend passes 1, because it has no pages and
/// nothing to align for.
pub fn host_align() -> usize {
    // SAFETY: `sysconf` with a recognised name returns a long; `_SC_PAGESIZE` is one.
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the whole rule exists for: the shares tile `[0, total)` exactly, in ascending
    /// cohort order, with every share but the last aligned.
    fn check(total: usize, members: usize, align: usize) {
        let shares: Vec<_> = (0..members)
            .map(|r| partition(total, r, members, align))
            .collect();
        assert_eq!(shares[0].0, 0, "the first member starts at zero");
        for pair in shares.windows(2) {
            assert_eq!(
                pair[0].0 + pair[0].1,
                pair[1].0,
                "{total} over {members} at {align}: a gap or an overlap"
            );
        }
        let last = *shares.last().unwrap();
        assert_eq!(last.0 + last.1, total, "{total} over {members} at {align}");
        if total >= align * members {
            for &(offset, length) in &shares[..members - 1] {
                assert_eq!(offset % align, 0);
                assert_eq!(length % align, 0);
            }
        }
    }

    #[test]
    fn shares_tile_the_segment() {
        for align in [1usize, 8, 4096] {
            for total in [0, 1, 7, 4095, 4096, 10025, 1 << 20, (1 << 20) + 17] {
                for members in 1..=8 {
                    check(total, members, align);
                }
            }
        }
    }

    #[test]
    fn small_segments_belong_to_the_first_member() {
        // Below one aligned share per member there is nothing to cut: a partial alignment unit is
        // not a range a mapping can express, so the first member takes the whole segment.
        let total = 4096 * 2 - 1;
        assert_eq!(partition(total, 0, 4, 4096), (0, total));
        for member in 1..4 {
            assert_eq!(partition(total, member, 4, 4096), (total, 0));
        }
    }

    #[test]
    fn a_lone_participant_owns_the_segment() {
        // One member takes everything at any alignment, including the alignments that would
        // otherwise round a share down to nothing.
        for align in [1usize, 4096, 1 << 20] {
            assert_eq!(partition(10025, 0, 1, align), (0, 10025));
        }
    }

    #[test]
    fn the_host_alignment_is_a_whole_number_of_bytes() {
        // Never zero: a zero divisor would be a division by zero, and the rule falls back to one
        // byte when the host will not name a page.
        assert!(host_align() >= 1);
    }
}
