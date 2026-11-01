use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

use hypha_core::{
    Page, PageRequest, Recorded, RepoError, TransferId, VariantId, Versioned, stamp_insert,
    stamp_update,
};
use hypha_inventory::{
    IncomingTransfer, Transfer, TransferFilter, TransferNumber, TransferRepo, TransferSummary,
    incoming_transfers, repo::keys,
};

use crate::{InMemoryOutbox, table::Table};

/// Transfers between locations, writing their events to the shared outbox.
#[derive(Clone)]
pub struct InMemoryTransfers {
    pub(crate) transfers: Arc<Table<TransferId, Transfer>>,
    last_number: Arc<AtomicI64>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryTransfers {
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            transfers: Arc::default(),
            // The first number is 1001, as the database's sequence starts.
            last_number: Arc::new(AtomicI64::new(1000)),
            outbox: outbox.clone(),
        }
    }
}

/// One transfer per number.
pub(crate) fn unique(a: &Transfer, b: &Transfer) -> Option<&'static str> {
    (a.id() != b.id() && a.number() == b.number()).then_some(keys::TRANSFER_NUMBER)
}

impl TransferRepo for InMemoryTransfers {
    async fn get(&self, id: TransferId) -> Result<Option<Transfer>, RepoError> {
        Ok(self.transfers.get(&id))
    }

    async fn list(
        &self,
        filter: &TransferFilter,
        page: PageRequest<TransferId>,
    ) -> Result<Page<TransferSummary>, RepoError> {
        Ok(self
            .transfers
            .page(|t| filter.matches(t), page, Transfer::summary))
    }

    async fn next_number(&self) -> Result<TransferNumber, RepoError> {
        let n = self.last_number.fetch_add(1, Ordering::SeqCst) + 1;
        TransferNumber::new(n).map_err(|e| RepoError::Backend(e.to_string()))
    }

    async fn insert(&self, transfer: &mut Transfer) -> Result<(), RepoError> {
        stamp_insert(transfer, self.outbox.now());
        let events = transfer.take_events();
        self.transfers
            .insert(transfer.id(), transfer, keys::TRANSFER_ID, &unique)?;
        self.outbox.write(events, Some(transfer.version()));
        Ok(())
    }

    async fn update(&self, transfer: &mut Transfer) -> Result<(), RepoError> {
        stamp_update(transfer, self.outbox.now());
        let events = transfer.take_events();
        self.transfers.update(transfer.id(), transfer, &unique)?;
        self.outbox.write(events, Some(transfer.version()));
        Ok(())
    }

    async fn delete(&self, transfer: &mut Transfer) -> Result<(), RepoError> {
        let mut rows = self.transfers.lock();
        match rows.get(&transfer.id()) {
            Some(stored) if stored.version() == transfer.version() => {}
            _ => return Err(RepoError::StaleVersion),
        }
        rows.remove(&transfer.id());
        drop(rows);
        self.outbox.write(transfer.take_events(), None);
        Ok(())
    }

    async fn incoming(&self, variants: &[VariantId]) -> Result<Vec<IncomingTransfer>, RepoError> {
        Ok(incoming_transfers(self.transfers.all().iter(), variants))
    }
}
