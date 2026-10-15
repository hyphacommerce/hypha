//! The `blob_store` contract against a real S3 server: the development server from
//! `compose.yaml` by default (`.cargo/config.toml` sets `HYPHA_S3_*`), or any other,
//! R2 included, with the same variables. Uploads and downloads go over HTTP like a
//! browser's, so the server's own checks (signature, size, type, checksum, expiry) are
//! what's tested.

use std::time::{SystemTime, UNIX_EPOCH};

use hypha_core::{Clock, Timestamp};
use hypha_store_s3::{ReqwestClient, S3BlobStore, S3Config};
use hypha_testkit::contract;

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_micros() as i64);
        Timestamp::from_unix_micros(micros)
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} (see .cargo/config.toml)"))
}

#[tokio::test]
async fn blob_store() {
    let config = S3Config {
        endpoint: env("HYPHA_S3_ENDPOINT"),
        region: env("HYPHA_S3_REGION"),
        bucket: env("HYPHA_S3_BUCKET"),
        access_key_id: env("HYPHA_S3_ACCESS_KEY_ID"),
        secret_access_key: env("HYPHA_S3_SECRET_ACCESS_KEY"),
    };
    let endpoint = config.endpoint.clone();
    let store = S3BlobStore::new(config, ReqwestClient::default(), SystemClock);
    store.ensure_bucket().await.unwrap_or_else(|e| {
        panic!("the S3 tests need a server at {endpoint} (`docker compose up -d`): {e}")
    });
    let http = reqwest::Client::new();
    contract::blob_store(
        &store,
        SystemClock.now(),
        async |upload, body| {
            let mut request = http.put(&upload.url).body(body);
            for (name, value) in &upload.headers {
                request = request.header(name, value);
            }
            match request.send().await {
                Ok(response) => response.status().is_success(),
                Err(_) => false,
            }
        },
        async |url| {
            let response = http.get(url).send().await.ok()?;
            if !response.status().is_success() {
                return None;
            }
            Some(response.bytes().await.ok()?.to_vec())
        },
    )
    .await;
}
