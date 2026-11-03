use super::*;

fn context() -> Context {
    Context { wake: Arc::new(NativeWake::default()), closed: Arc::new(AtomicBool::new(false)),
        lost: AtomicBool::new(false), run_loop: std::ptr::null_mut() }
}

#[test]
fn matching_iterators_are_drained_and_every_observed_handle_is_released() {
    let context = context(); let mut remaining = 3; let mut released = vec![];
    assert!(context.drain(7, |iterator| {
        assert_eq!(iterator, 7);
        let next = remaining; remaining = remaining.saturating_sub(1); next
    }, |service| released.push(service)));
    assert_eq!(released, [3, 2, 1]);
    assert!(!context.lost.load(Ordering::Acquire));
    assert!(!context.drain(7, |_| 0, |_| panic!("Empty iterator cannot release a device")));
}

#[test]
fn iterator_overflow_is_bounded_and_withdraws_continuity() {
    let context = context(); let mut released = 0;
    assert!(context.drain(1, |_| 2, |_| released += 1));
    assert_eq!(released, MAX_MATCHES);
    assert!(context.lost.load(Ordering::Acquire));
}
