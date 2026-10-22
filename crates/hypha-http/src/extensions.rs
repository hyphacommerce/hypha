//! The host's `ExtensionCaller`: calls to apps' extension points go out as every call to a
//! merchant URL does, through the host's `HostCalls` (signed by `outbound::prepare`, public
//! addresses only natively, `fetch` on Workers), with the store named in `Hypha-Store`.

use hypha_core::{Clock, StoreId};
use hypha_extensions::{Answer, ExtensionCaller, OutgoingCall, STORE_HEADER, TransportError};

use crate::{
    host::{HostCalls, HostClock},
    outbound::{CallError, CallPurpose, Method, OutboundPolicy, SignedCall, prepare},
};

/// Sends extension calls for one store.
pub struct HostExtensions<'a> {
    pub calls: &'a HostCalls,
    pub outbound: OutboundPolicy,
    /// For the signature's time.
    pub clock: &'a HostClock,
    pub store: StoreId,
}

impl ExtensionCaller for HostExtensions<'_> {
    async fn send(&self, call: OutgoingCall<'_>) -> Result<Answer, TransportError> {
        let mut headers = call.headers;
        headers.push((STORE_HEADER, self.store.to_string()));
        let signed = SignedCall {
            method: Method::Post,
            url: call.url,
            timeout: call.timeout,
            secret: call.secret,
            at: self.clock.now().unix_secs(),
            headers: &headers,
            body: call.body,
            discard_answer: false,
        };
        let sent = match prepare(self.outbound, signed) {
            Ok(mut prepared) => {
                prepared.purpose = CallPurpose::Extension {
                    point: call.point,
                    app: call.app.to_string(),
                    read_only: call.read_only,
                };
                self.calls.send(prepared).await
            }
            Err(e) => Err(e),
        };
        match sent {
            Ok(answer) => Ok(Answer {
                status: answer.status,
                body: answer.body,
            }),
            Err(CallError::Timeout) => Err(TransportError::Timeout),
            Err(CallError::TooLarge) => Err(TransportError::TooLarge),
            Err(CallError::Failed(reason)) => Err(TransportError::Unreachable(reason)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use hypha_core::Timestamp;
    use hypha_extensions::{EXTENSION_HEADER, MAX_ANSWER_BYTES};
    use uuid::Uuid;

    use super::*;
    use crate::outbound::{OutboundAnswer, OutboundCall, SIGNATURE_HEADER, sign};

    struct Epoch;
    impl Clock for Epoch {
        fn now(&self) -> Timestamp {
            Timestamp::from_unix_micros(1_700_000_000_500_000)
        }
    }

    /// Calls that answer `answer` and keep what was sent.
    fn calls(
        answer: Result<OutboundAnswer, CallError>,
    ) -> (HostCalls, Arc<Mutex<Vec<OutboundCall>>>) {
        let sent: Arc<Mutex<Vec<OutboundCall>>> = Arc::default();
        let log = Arc::clone(&sent);
        let calls = HostCalls::new(move |call| {
            log.lock().unwrap().push(call);
            let answer = answer.clone();
            Box::pin(async move { answer })
        });
        (calls, sent)
    }

    fn send(calls: &HostCalls, url: &str) -> Result<Answer, TransportError> {
        let caller = HostExtensions {
            calls,
            outbound: OutboundPolicy::default(),
            clock: &HostClock::new(Epoch),
            store: StoreId::from_uuid(Uuid::from_u128(7)),
        };
        pollster::block_on(caller.send(OutgoingCall {
            point: "shipping.rates",
            read_only: true,
            app: &hypha_core::AppKey::new("app_rates").unwrap(),
            url,
            secret: "s3cret",
            headers: vec![(EXTENSION_HEADER, "shipping.rates".to_string())],
            body: b"{}".to_vec(),
            timeout: std::time::Duration::from_secs(1),
        }))
    }

    #[test]
    fn a_call_is_signed_and_names_the_store() {
        let (calls, sent) = calls(Ok(OutboundAnswer {
            status: 200,
            body: b"{}".to_vec(),
            content_type: None,
        }));
        assert_eq!(
            send(&calls, "https://app.example/x"),
            Ok(Answer {
                status: 200,
                body: b"{}".to_vec()
            })
        );
        let sent = sent.lock().unwrap();
        let headers = &sent[0].headers;
        let header = |name: &str| {
            headers
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            header(SIGNATURE_HEADER),
            Some(sign("s3cret", 1_700_000_000, b"{}"))
        );
        assert_eq!(header("Hypha-Store"), Some(format!("store_{:032x}", 7)));
        assert_eq!(header("Hypha-Extension").as_deref(), Some("shipping.rates"));
        assert_eq!(sent[0].method, Method::Post);
    }

    #[test]
    fn any_status_comes_back_as_an_answer() {
        let (calls, _) = calls(Ok(OutboundAnswer {
            status: 500,
            body: b"{\"code\":\"handler_failed\"}".to_vec(),
            content_type: None,
        }));
        assert_eq!(
            send(&calls, "https://app.example/x").map(|a| a.status),
            Ok(500)
        );
    }

    #[test]
    fn transport_failures_keep_their_kind() {
        for (error, want) in [
            (CallError::Timeout, TransportError::Timeout),
            (CallError::TooLarge, TransportError::TooLarge),
            (
                CallError::Failed("unreachable".to_string()),
                TransportError::Unreachable("unreachable".to_string()),
            ),
        ] {
            let (calls, _) = calls(Err(error));
            assert_eq!(send(&calls, "https://app.example/x"), Err(want));
        }
    }

    #[test]
    fn a_private_literal_is_refused_before_sending() {
        let (calls, sent) = calls(Ok(OutboundAnswer {
            status: 200,
            body: Vec::new(),
            content_type: None,
        }));
        assert_eq!(
            send(&calls, "http://169.254.169.254/latest"),
            Err(TransportError::Unreachable(
                "the URL is not a public address".to_string()
            ))
        );
        assert!(sent.lock().unwrap().is_empty());
        // The cap is the extension points' own.
        assert_eq!(crate::outbound::MAX_ANSWER_BYTES, MAX_ANSWER_BYTES);
    }
}
