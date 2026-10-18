//! Fetching apps' manifests (design note 0029, AP5): the host's `ManifestFetcher`, a
//! plain GET through the host's outbound client (`HostCalls`), so natively it reaches
//! only public addresses unless `outbound.allow_private`, and on Workers it's `fetch`.
//! Answers are read up to `outbound::MAX_ANSWER_BYTES`; `hypha_apps::fetch_manifest`
//! then caps them at the manifest's own limit and checks they're a manifest (JSON).

use std::time::Duration;

use hypha_apps::ManifestFetcher;

use crate::{
    host::HostCalls,
    outbound::{CallError, OutboundPolicy},
};

/// How long a manifest may take to arrive.
pub const MANIFEST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HostManifests<'a> {
    pub calls: &'a HostCalls,
    pub outbound: OutboundPolicy,
}

impl ManifestFetcher for HostManifests<'_> {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        let call =
            crate::outbound::prepare_get(self.outbound, url, MANIFEST_TIMEOUT).map_err(describe)?;
        let answer = self.calls.send(call).await.map_err(describe)?;
        answer.into_success().map_err(describe)
    }
}

fn describe(e: CallError) -> String {
    match e {
        CallError::Timeout => format!("no answer within {} s", MANIFEST_TIMEOUT.as_secs()),
        CallError::Failed(reason) => reason,
        CallError::TooLarge => "the manifest is too large".to_string(),
    }
}
