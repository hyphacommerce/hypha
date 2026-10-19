//! Spores in the HTTP layer (design note 0030): the host's capabilities as a spore's
//! environment (`spore_host`).

use std::sync::Arc;

use hypha_spore_core::{HttpError, HttpMethod, HttpRequest, HttpResponse, SporeHost, SporeHttp};

use crate::{
    host::HostCalls,
    outbound::{CallError, Method, OutboundCall, OutboundPolicy, check_literal},
    state::HostEnv,
};

/// The host's capabilities for its spores (`hypha_spore_core::SporeHost`): its clock,
/// IDs, randomness and `SecretBox` (`env`), and HTTP through `calls`, the client calls
/// to apps go through, under the same `policy` (natively only public addresses unless
/// `outbound.allow_private`; no redirects; the answer capped).
pub fn spore_host(env: &HostEnv, calls: HostCalls, policy: OutboundPolicy) -> SporeHost {
    let runtime = env.runtime();
    SporeHost {
        clock: runtime.clock.shared(),
        ids: Arc::new(runtime.ids.clone()),
        random: Arc::new(runtime.random.clone()),
        secrets: Arc::new(env.secret_box().clone()),
        http: SporeHttp::new(move |request| {
            let calls = calls.clone();
            Box::pin(async move { send(&calls, policy, request).await })
        }),
    }
}

/// Sends a spore's request through `calls`.
async fn send(
    calls: &HostCalls,
    policy: OutboundPolicy,
    request: HttpRequest,
) -> Result<HttpResponse, HttpError> {
    check_literal(policy, &request.url).map_err(http_error)?;
    let call = OutboundCall {
        method: match request.method {
            HttpMethod::Get => Method::Get,
            HttpMethod::Post => Method::Post,
            HttpMethod::Put => Method::Put,
            HttpMethod::Patch => Method::Patch,
            HttpMethod::Delete => Method::Delete,
        },
        url: request.url,
        timeout: request.timeout,
        headers: request.headers,
        body: request.body,
        discard_answer: false,
        max_answer: request.max_answer,
    };
    let answer = calls.send(call).await.map_err(http_error)?;
    Ok(HttpResponse {
        status: answer.status,
        content_type: answer.content_type,
        body: answer.body,
    })
}

fn http_error(e: CallError) -> HttpError {
    match e {
        CallError::Timeout => HttpError::Timeout,
        CallError::TooLarge => HttpError::TooLarge,
        CallError::Failed(reason) => HttpError::Failed(reason),
    }
}
