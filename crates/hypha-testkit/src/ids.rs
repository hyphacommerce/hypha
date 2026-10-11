use std::sync::atomic::{AtomicU64, Ordering};

use hypha_core::IdGenerator;
use uuid::Uuid;

#[derive(Default)]
pub struct SequentialIds {
    next: AtomicU64,
}

impl IdGenerator for SequentialIds {
    fn new_uuid(&self) -> Uuid {
        let n = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        Uuid::from_u128(u128::from(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_ids_count_up_from_one() {
        let ids = SequentialIds::default();
        assert_eq!(ids.new_uuid(), Uuid::from_u128(1));
        assert_eq!(ids.new_uuid(), Uuid::from_u128(2));
    }
}
