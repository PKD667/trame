use crate::nv::error::LayoutError;
use crate::nv::layout::Layout;

#[test]
fn layout_requires_an_unambiguous_wrapping_ring() {
    for depth in [0, 1, 3, 6] {
        assert_eq!(Layout::new(depth, 1), Err(LayoutError::InvalidDepth));
    }
    assert_eq!(Layout::new(2, 0).unwrap().words(), 8);
    assert_eq!(Layout::new(4, 5).unwrap().words(), 24);
    assert_eq!(Layout::new(1 << 31, u32::MAX), Err(LayoutError::TooLarge));
}

#[test]
fn initialization_sets_each_free_sequence() {
    let layout = Layout::new(4, 8).unwrap();
    let mut arena = vec![u32::MAX; layout.words()];
    layout.init(&mut arena);
    for slot in 0..4 {
        assert_eq!(arena[slot * layout.slot_words()], slot as u32);
    }
}
