//! The host's `RateProvider` (design note 0015, section 5): it routes `app_*` keys to
//! external apps' HTTP callbacks and `spore_*` keys to the spores this host was built
//! with. Static dispatch, a branch per kind, so futures stay provably `Send`.

use hypha_shipping::{ProviderError, ProviderKey, Rate, RateProvider, RateRequest, ServiceInfo};

/// Knows no provider yet: the callback client and spore registration come next.
pub struct HostRates;

impl RateProvider for HostRates {
    async fn services(&self, provider: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
        Err(ProviderError::Unknown(provider.clone()))
    }

    async fn quote(
        &self,
        provider: &ProviderKey,
        _: &RateRequest,
    ) -> Result<Vec<Rate>, ProviderError> {
        Err(ProviderError::Unknown(provider.clone()))
    }
}
