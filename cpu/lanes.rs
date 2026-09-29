use crate::contract::{Addr, Edge, Error, Invalid};

/// Check lane declarations against the job and the selected CPU backend's frame limit.
pub(crate) fn validate(
    workers: &[u32],
    edges: &[Edge],
    bytes: usize,
    job_size: u32,
    frame_limit: usize,
) -> Result<(), Error> {
    if workers.windows(2).any(|w| w[0] >= w[1]) {
        return Err(Error::Invalid(Invalid::UnorderedWorkers));
    }
    if workers.iter().any(|&worker| worker >= job_size) {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    if edges.windows(2).any(|w| {
        (w[0].source(), w[0].destination()) >= (w[1].source(), w[1].destination())
    }) {
        return Err(Error::Invalid(Invalid::UnorderedEdges));
    }
    // A `Remote` end is another host's worker and never in `workers`. A pair must still start or
    // end here, and a `Local` end must be one of the load's workers.
    let among = |end: Addr| match end {
        Addr::Local(rank) => workers.contains(&rank),
        Addr::Remote { .. } => true,
    };
    for edge in edges {
        let (source, destination) = (edge.source(), edge.destination());
        let here = matches!(source, Addr::Local(_)) || matches!(destination, Addr::Local(_));
        if !here || !among(source) || !among(destination) {
            return Err(Error::Invalid(Invalid::EdgeOutsideWorkers));
        }
    }
    if bytes > frame_limit {
        return Err(Error::TooLarge { limit: frame_limit });
    }
    Ok(())
}
