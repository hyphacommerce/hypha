//! The configured email sender: the log or Resend, its HTTP through `fetch`. SMTP is native
//! only (a Worker can't open SMTP connections through our host layer), so `email.sender =
//! "smtp"` stops setup.

use std::pin::pin;
use std::sync::Arc;

use futures_util::future::{Either, select};
use hypha_config::{EmailConfig, SenderConfig};
use hypha_http::host::HostMail;
use hypha_mail_resend::{HttpClient, HttpRequest, HttpResponse, MAX_ANSWER_BYTES, ResendSender};
use hypha_notifications::EmailSender;
use worker::send::SendFuture;
use worker::{AbortController, Delay, Fetch, Headers, Request, RequestInit};

use crate::platform::read_body;

/// Builds the sender `config` names, its key read through `env`
/// (`HYPHA_RESEND_API_KEY`, a Worker secret).
pub(crate) fn email_sender(
    config: &EmailConfig,
    env: impl Fn(&str) -> Option<String>,
) -> Result<HostMail, String> {
    match &config.sender {
        SenderConfig::Log => {
            tracing::warn!(
                "emails are logged, not sent (email.sender = \"log\"): development only"
            );
            Ok(HostMail::log())
        }
        SenderConfig::Smtp(_) => Err(
            "email.sender = \"smtp\" isn't available on Workers: use \"resend\" (or \"log\")"
                .to_string(),
        ),
        SenderConfig::Resend => {
            let key = env("HYPHA_RESEND_API_KEY")
                .filter(|k| !k.trim().is_empty())
                .ok_or(
                    "HYPHA_RESEND_API_KEY is required with email.sender = \"resend\" \
                     (`wrangler secret put HYPHA_RESEND_API_KEY`)",
                )?;
            let sender = Arc::new(ResendSender::new(key, FetchClient));
            tracing::info!(from = %config.from, "emails: sent through Resend");
            Ok(HostMail::new("resend", move |email| {
                let sender = Arc::clone(&sender);
                Box::pin(SendFuture::new(async move { sender.send(&email).await }))
            }))
        }
    }
}

/// How long one request to the email provider may take, as natively
/// (`hypha_mail_resend::ReqwestClient`).
const TIMEOUT: std::time::Duration = hypha_notifications::SEND_TIMEOUT;

/// The provider's HTTP through `fetch`, aborted after `TIMEOUT`.
struct FetchClient;

impl HttpClient for FetchClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        let controller = AbortController::default();
        let signal = controller.signal();
        match select(pin!(send(request, &signal)), pin!(Delay::from(TIMEOUT))).await {
            Either::Left((result, _)) => result,
            Either::Right(((), _)) => {
                controller.abort();
                Err(format!("no answer within {} s", TIMEOUT.as_secs()))
            }
        }
    }
}

async fn send(request: HttpRequest, signal: &worker::AbortSignal) -> Result<HttpResponse, String> {
    let headers = Headers::new();
    for (name, value) in &request.headers {
        headers
            .set(name, value)
            .map_err(|e| format!("invalid header {name:?}: {e}"))?;
    }
    let mut init = RequestInit::new();
    init.with_method(worker::Method::from(request.method.to_string()))
        .with_headers(headers)
        .with_body(Some(
            worker::js_sys::Uint8Array::from(&request.body[..]).into(),
        ));
    let mut response =
        Fetch::Request(Request::new_with_init(&request.url, &init).map_err(|e| e.to_string())?)
            .send_with_signal(signal)
            .await
            .map_err(|e| e.to_string())?;
    let status = response.status_code();
    let body = read_body(&mut response, MAX_ANSWER_BYTES).await?;
    Ok(HttpResponse { status, body })
}
