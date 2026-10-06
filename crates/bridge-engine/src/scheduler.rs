use bridge_core::{BlockHeight, IntervalRange};

/// Calculates the next fixed interval chunk to synchronize.
pub fn next_interval(
    committed_height: BlockHeight,
    tip_height: BlockHeight,
    interval_size: u32,
) -> Option<IntervalRange> {
    if tip_height <= committed_height {
        return None;
    }

    let start = BlockHeight(committed_height.0 + 1);
    let end = BlockHeight((start.0 + interval_size - 1).min(tip_height.0));

    Some(IntervalRange::new(start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_interval_chunking() {
        // Committed 100, tip 250, batch 50 -> [101, 150]
        let next = next_interval(BlockHeight(100), BlockHeight(250), 50).unwrap();
        assert_eq!(next.start, BlockHeight(101));
        assert_eq!(next.end, BlockHeight(150));
        assert_eq!(next.len(), 50);

        // Tip is 120, batch 50 -> [101, 120]
        let next = next_interval(BlockHeight(100), BlockHeight(120), 50).unwrap();
        assert_eq!(next.start, BlockHeight(101));
        assert_eq!(next.end, BlockHeight(120));
        assert_eq!(next.len(), 20);

        // Committed equals tip -> None
        assert!(next_interval(BlockHeight(100), BlockHeight(100), 50).is_none());
    }
}
