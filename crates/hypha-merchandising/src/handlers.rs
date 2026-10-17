//! In-process event handlers for merchandising's own data (design note 0019, 5.1).

use hypha_categories::CategoryDeleted;
use hypha_core::{CategoryId, Event, EventHandler, HasBatch, StoredEvent};
use hypha_media::{StageAssetUsages, UsageOwner};

/// `category.deleted`: removes the deleted category's image usage, so the asset can be
/// deleted from the library again. Removing usages that are already gone does nothing,
/// so running it twice is harmless.
pub struct CategoryImageCleanup;

impl<P> EventHandler<P> for CategoryImageCleanup
where
    P: HasBatch<Batch: StageAssetUsages>,
{
    fn name(&self) -> &'static str {
        "category_image_cleanup"
    }

    fn wants(&self, event_type: &str) -> bool {
        event_type == CategoryDeleted::TYPE
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        let category: CategoryId = event.data["category"]
            .as_str()
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| format!("event {} names no category", event.id))?;
        let mut batch = ports.batch();
        batch.replace_asset_usages(&UsageOwner::category(category), &[]);
        ports.commit(batch).await.map_err(|e| e.to_string())
    }
}
