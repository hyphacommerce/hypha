use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, MutexGuard},
};

use hypha_core::{AssetId, Keyset, Page, PageRequest, RepoError, Timestamp};
use hypha_media::{
    Asset, AssetFilter, AssetRepo, AssetSort, AssetStatus, AssetUsage, AssetUsageRepo, BlobError,
    BlobHead, BlobKey, BlobStore, HasAssetUsages, HasAssets, PresignedUpload, UploadTerms,
    UsageOwner, UsageRole, repo::keys,
};
use sha2::{Digest, Sha256};

use crate::table::{Table, no_unique_keys};

/// Assets and their usages together, like one database: the usages' reference to their
/// asset is checked under both locks (always assets first, then usages).
#[derive(Default)]
pub struct InMemoryAssetRepo {
    pub(crate) assets: Table<AssetId, Asset>,
    pub(crate) usages: Mutex<Usages>,
}

/// Every usage, by owner and asset.
pub(crate) type Usages = BTreeMap<(UsageOwner, AssetId), UsageRole>;

/// The repo is all of media's ports, so media's own tests need no `InMemoryPorts`.
impl HasAssets for InMemoryAssetRepo {
    type Assets = Self;
    fn assets(&self) -> &Self {
        self
    }
}

impl HasAssetUsages for InMemoryAssetRepo {
    type AssetUsages = Self;
    fn asset_usages(&self) -> &Self {
        self
    }
}

impl InMemoryAssetRepo {
    pub(crate) fn lock_usages(&self) -> MutexGuard<'_, Usages> {
        self.usages.lock().unwrap()
    }
}

/// `StageAssetUsages::replace_asset_usages` on locked tables: fails, changing nothing,
/// if an asset is listed twice or doesn't exist.
pub(crate) fn replace_usages(
    assets: &HashMap<AssetId, Asset>,
    usages: &mut Usages,
    owner: &UsageOwner,
    replacement: &[(AssetId, UsageRole)],
) -> Result<(), RepoError> {
    let mut seen = std::collections::HashSet::new();
    for (asset, _) in replacement {
        if !seen.insert(*asset) {
            return Err(RepoError::Backend(format!(
                "asset {asset} listed twice for {owner}"
            )));
        }
        if !assets.contains_key(asset) {
            return Err(RepoError::Reference(keys::USAGE_ASSET));
        }
    }
    usages.retain(|(o, _), _| o != owner);
    for (asset, role) in replacement {
        usages.insert((owner.clone(), *asset), role.clone());
    }
    Ok(())
}

impl AssetRepo for InMemoryAssetRepo {
    async fn get(&self, id: AssetId) -> Result<Option<Asset>, RepoError> {
        Ok(self.assets.get(&id))
    }

    async fn get_many(&self, ids: &[AssetId]) -> Result<Vec<Asset>, RepoError> {
        Ok(self.assets.get_many(ids))
    }

    async fn list(
        &self,
        filter: &AssetFilter,
        sort: AssetSort,
        page: PageRequest<Keyset<AssetId>>,
    ) -> Result<Page<Asset>, RepoError> {
        if let Some(after) = &page.after
            && !sort.fits(&after.key)
        {
            return Err(RepoError::Backend(format!(
                "a {:?} position for a {sort:?} list",
                after.key
            )));
        }
        Ok(self.assets.page_sorted(
            |a| a.status() == AssetStatus::Ready && filter.matches(a),
            &page,
            sort.direction(),
            Clone::clone,
            |a: &Asset| sort.key(a),
        ))
    }

    async fn abandoned(&self, before: Timestamp, limit: u32) -> Result<Vec<Asset>, RepoError> {
        let mut found: Vec<Asset> = self
            .assets
            .all()
            .into_iter()
            .filter(|a| match a.status() {
                AssetStatus::Pending => a.created_at() < before,
                AssetStatus::Failed => a.updated_at() < before,
                AssetStatus::Ready => false,
            })
            .collect();
        found.sort_by_key(Asset::id);
        found.truncate(limit as usize);
        Ok(found)
    }

    async fn insert(&self, asset: &mut Asset) -> Result<(), RepoError> {
        self.assets
            .insert(asset.id(), asset, keys::ASSET_ID, &no_unique_keys)
    }

    async fn update(&self, asset: &mut Asset) -> Result<(), RepoError> {
        self.assets.update(asset.id(), asset, &no_unique_keys)
    }

    async fn delete(&self, id: AssetId) -> Result<(), RepoError> {
        let mut assets = self.assets.lock();
        if self.lock_usages().keys().any(|(_, asset)| *asset == id) {
            return Err(RepoError::Reference(keys::USAGE_ASSET));
        }
        assets.remove(&id);
        Ok(())
    }
}

impl AssetUsageRepo for InMemoryAssetRepo {
    async fn list_usages(
        &self,
        asset: AssetId,
        page: PageRequest<UsageOwner>,
    ) -> Result<Page<AssetUsage>, RepoError> {
        let usages = self.lock_usages();
        let fetched = usages
            .iter()
            .rev()
            .filter(|((owner, a), _)| {
                *a == asset && page.after.as_ref().is_none_or(|after| owner < after)
            })
            .take(page.limit.get() as usize + 1)
            .map(|((owner, a), role)| AssetUsage {
                asset: *a,
                owner: owner.clone(),
                role: role.clone(),
            })
            .collect();
        Ok(Page::from_overfetch(fetched, page.limit))
    }
}

/// Files in memory, with presigned uploads that check what real storage checks: the
/// headers, the size, the checksum and the expiry. A test plays the client with `upload`
/// (or stores a file directly with `put`).
#[derive(Default)]
pub struct InMemoryBlobStore {
    files: Mutex<HashMap<BlobKey, StoredFile>>,
    /// Signed uploads by URL: where they go, what they allow, until when.
    signed: Mutex<HashMap<String, (BlobKey, UploadTerms, Timestamp)>>,
}

/// A stored file and the headers it was uploaded with.
#[derive(Clone)]
struct StoredFile {
    body: Vec<u8>,
    content_type: Option<String>,
    cache_control: Option<String>,
}

impl InMemoryBlobStore {
    /// Stores `body` at `key` as if uploaded, with no checks and no headers.
    pub fn put(&self, key: &BlobKey, body: Vec<u8>) {
        let file = StoredFile {
            body,
            content_type: None,
            cache_control: None,
        };
        self.files.lock().unwrap().insert(key.clone(), file);
    }

    /// Sends `body` with the upload's URL and headers at `now`, as a client would.
    /// `false` when storage would refuse it: unknown or expired URL, headers other than
    /// those signed, another size, or contents that don't match the checksum.
    pub fn upload(&self, upload: &PresignedUpload, body: Vec<u8>, now: Timestamp) -> bool {
        let signed = self.signed.lock().unwrap();
        let Some((key, terms, expires_at)) = signed.get(&upload.url) else {
            return false;
        };
        let checksum_ok = terms
            .checksum
            .is_none_or(|c| <[u8; 32]>::from(Sha256::digest(&body)) == *c.as_bytes());
        if now > *expires_at
            || upload.headers != signed_headers(terms)
            || body.len() as u64 != terms.content_length
            || !checksum_ok
        {
            return false;
        }
        let file = StoredFile {
            body,
            content_type: Some(terms.content_type.to_string()),
            cache_control: Some(terms.cache_control.to_string()),
        };
        self.files.lock().unwrap().insert(key.clone(), file);
        true
    }

    /// What a download URL from `presign_download` returns at `now`: the file, or `None`
    /// when the URL expired or nothing is stored.
    pub fn download(&self, url: &str, now: Timestamp) -> Option<Vec<u8>> {
        let rest = url.strip_prefix("memory://download/")?;
        let (key, expires) = rest.rsplit_once("?expires=")?;
        let expires: i64 = expires.parse().ok()?;
        if now.unix_micros() > expires {
            return None;
        }
        let key = BlobKey::restore(key.to_string());
        self.files.lock().unwrap().get(&key).map(|f| f.body.clone())
    }
}

/// The headers a client must send with an upload under `terms`.
fn signed_headers(terms: &UploadTerms) -> Vec<(String, String)> {
    let mut headers = vec![
        ("content-type".to_string(), terms.content_type.to_string()),
        ("cache-control".to_string(), terms.cache_control.to_string()),
    ];
    if let Some(checksum) = terms.checksum {
        headers.push(("x-checksum-sha256".to_string(), checksum.to_string()));
    }
    headers
}

impl BlobStore for InMemoryBlobStore {
    async fn presign_upload(
        &self,
        key: &BlobKey,
        terms: &UploadTerms,
        now: Timestamp,
    ) -> Result<PresignedUpload, BlobError> {
        let mut signed = self.signed.lock().unwrap();
        let url = format!("memory://upload/{}", signed.len() + 1);
        let expires_at = now.plus_secs(i64::from(terms.expires_in_secs));
        signed.insert(url.clone(), (key.clone(), terms.clone(), expires_at));
        Ok(PresignedUpload {
            url,
            method: "PUT",
            headers: signed_headers(terms),
            expires_at,
        })
    }

    async fn head(&self, key: &BlobKey) -> Result<Option<BlobHead>, BlobError> {
        Ok(self.files.lock().unwrap().get(key).map(|f| BlobHead {
            size: f.body.len() as u64,
            content_type: f.content_type.clone(),
            cache_control: f.cache_control.clone(),
        }))
    }

    async fn read_range(&self, key: &BlobKey, start: u64, len: u64) -> Result<Vec<u8>, BlobError> {
        let files = self.files.lock().unwrap();
        let file = &files
            .get(key)
            .ok_or_else(|| BlobError::Backend(format!("no file at {key}")))?
            .body;
        let start = usize::try_from(start).unwrap_or(usize::MAX).min(file.len());
        let end = start
            .saturating_add(usize::try_from(len).unwrap_or(usize::MAX))
            .min(file.len());
        Ok(file[start..end].to_vec())
    }

    async fn copy(&self, from: &BlobKey, to: &BlobKey) -> Result<(), BlobError> {
        let mut files = self.files.lock().unwrap();
        let file = files
            .get(from)
            .cloned()
            .ok_or_else(|| BlobError::Backend(format!("no file at {from}")))?;
        files.insert(to.clone(), file);
        Ok(())
    }

    async fn delete(&self, key: &BlobKey) -> Result<(), BlobError> {
        self.files.lock().unwrap().remove(key);
        Ok(())
    }

    async fn presign_download(
        &self,
        key: &BlobKey,
        now: Timestamp,
        expires_in_secs: u32,
    ) -> Result<String, BlobError> {
        let expires = now.plus_secs(i64::from(expires_in_secs)).unix_micros();
        Ok(format!("memory://download/{key}?expires={expires}"))
    }
}
