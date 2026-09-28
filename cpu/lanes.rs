use crate::contract::{Edge, Error, Invalid, Rank};

/// Check lane declarations against the job and the selected CPU backend's frame limit.
pub(crate) fn validate(
    workers: &[Rank],
    edges: &[Edge],
    bytes: usize,
    job_size: u32,
    frame_limit: usize,
) -> Result<(), Error> {
    if workers.windows(2).any(|w| w[0] >= w[1]) {
        return Err(Error::Invalid(Invalid::UnorderedWorkers));
    }
    if workers.iter().any(|worker| worker.get() >= job_size) {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    if edges.windows(2).any(|w| {
        (w[0].source(), w[0].destination()) >= (w[1].source(), w[1].destination())
    }) {
        return Err(Error::Invalid(Invalid::UnorderedEdges));
    }
    for edge in edges {
        if !workers.contains(&edge.source()) || !workers.contains(&edge.destination()) {
            return Err(Error::Invalid(Invalid::EdgeOutsideWorkers));
        }
    }
    if bytes > frame_limit {
        return Err(Error::TooLarge { limit: frame_limit });
    }
    Ok(())
}
