use std::{collections::HashMap, sync::Mutex};

use hypha_core::{AssetId, Page, PageRequest, RepoError, Timestamp};
use hypha_media::{
    Asset, AssetKind, AssetRepo, AssetStatus, BlobError, BlobHead, BlobKey, BlobStore,
    PresignedUpload, UploadTerms, repo::keys,
};
use sha2::{Digest, Sha256};

use crate::table::{Table, no_unique_keys};

#[derive(Default)]
pub struct InMemoryAssetRepo {
    pub(crate) assets: Table<AssetId, Asset>,
}

impl AssetRepo for InMemoryAssetRepo {
    async fn get(&self, id: AssetId) -> Result<Option<Asset>, RepoError> {
        Ok(self.assets.get(&id))
    }

    async fn list(
        &self,
        kind: Option<AssetKind>,
        page: PageRequest<AssetId>,
    ) -> Result<Page<Asset>, RepoError> {
        Ok(self.assets.page(
            |a| a.status() == AssetStatus::Ready && kind.is_none_or(|k| a.kind() == k),
            page,
            Clone::clone,
        ))
    }

    async fn insert(&self, asset: &mut Asset) -> Result<(), RepoError> {
        self.assets
            .insert(asset.id(), asset, keys::ASSET_ID, &no_unique_keys)
    }

    async fn update(&self, asset: &mut Asset) -> Result<(), RepoError> {
        self.assets.update(asset.id(), asset, &no_unique_keys)
    }

    async fn delete(&self, id: AssetId) -> Result<(), RepoError> {
        self.assets.remove(&id);
        Ok(())
    }
}

/// Files in memory, with presigned uploads that check what real storage checks: the
/// headers, the size, the checksum and the expiry. A test plays the client with `upload`
/// (or stores a file directly with `put`).
#[derive(Default)]
pub struct InMemoryBlobStore {
    files: Mutex<HashMap<BlobKey, Vec<u8>>>,
    /// Signed uploads by URL: where they go, what they allow, until when.
    signed: Mutex<HashMap<String, (BlobKey, UploadTerms, Timestamp)>>,
}

impl InMemoryBlobStore {
    /// Stores `body` at `key` as if uploaded, with no checks.
    pub fn put(&self, key: &BlobKey, body: Vec<u8>) {
        self.files.lock().unwrap().insert(key.clone(), body);
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
        self.files.lock().unwrap().insert(key.clone(), body);
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
        self.files.lock().unwrap().get(&key).cloned()
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
            size: f.len() as u64,
        }))
    }

    async fn read_range(&self, key: &BlobKey, start: u64, len: u64) -> Result<Vec<u8>, BlobError> {
        let files = self.files.lock().unwrap();
        let file = files
            .get(key)
            .ok_or_else(|| BlobError::Backend(format!("no file at {key}")))?;
        let start = usize::try_from(start).unwrap_or(usize::MAX).min(file.len());
        let end = start
            .saturating_add(usize::try_from(len).unwrap_or(usize::MAX))
            .min(file.len());
        Ok(file[start..end].to_vec())
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
