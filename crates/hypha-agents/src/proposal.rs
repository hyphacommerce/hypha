use std::fmt;
use std::time::Duration;

use hypha_core::{
    AgentId, IdempotencyKey, PendingEvent, ProposalId, Recorded, Recorder, Scope, Stamped, Stamps,
    Subject, Timestamp, UserId, Version, Versioned,
};
use serde_json::{Map, Value};

use crate::{
    OperationId, ProposalCreated, ProposalDecided, ProposalError, ProposalUpdated, events::PROPOSAL,
};

/// The largest request a proposal keeps, serialized: an admin write's body is far smaller.
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
/// The largest preview (or applied response) a proposal keeps, serialized.
pub const MAX_PREVIEW_BYTES: usize = 4 * 1024 * 1024;
/// The longest digest: a hash in hex or base64 with its algorithm's name.
pub const MAX_DIGEST_LEN: usize = 128;
/// The longest rejection note, in characters.
pub const MAX_NOTE_LEN: usize = 2000;
/// How long an approval holds a proposal `applying`: past it, the approval is taken to
/// have died before recording its write's answer, and the proposal counts as pending
/// again. Far longer than a write takes.
pub const APPLYING_LEASE: Duration = Duration::from_secs(5 * 60);

/// Where a proposal stands. `Pending` leaves to one of the others; `Applying` to a
/// decision or back to `Pending`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProposalStatus {
    Pending,
    /// Approved (`decided_by`, at `decided_at`), its write being sent: nobody else
    /// decides it meanwhile. Pending again when the write may go through later or the
    /// store moved, or once `APPLYING_LEASE` has passed.
    Applying,
    /// Approved, and its write committed.
    Applied,
    Rejected,
    /// Nobody decided within its lifetime.
    Expired,
    /// Approved, but its write failed (stale preview, a conflict, an app's refusal).
    Failed,
}

impl ProposalStatus {
    pub const ALL: [ProposalStatus; 6] = [
        ProposalStatus::Pending,
        ProposalStatus::Applying,
        ProposalStatus::Applied,
        ProposalStatus::Rejected,
        ProposalStatus::Expired,
        ProposalStatus::Failed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ProposalStatus::Pending => "pending",
            ProposalStatus::Applying => "applying",
            ProposalStatus::Applied => "applied",
            ProposalStatus::Rejected => "rejected",
            ProposalStatus::Expired => "expired",
            ProposalStatus::Failed => "failed",
        }
    }

    /// The status `as_str` spells, or the text that isn't one.
    pub fn parse(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|st| st.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

impl fmt::Display for ProposalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything needed to send the write again: its path parameters (text), its query and
/// its JSON body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProposalRequest {
    pub params: Map<String, Value>,
    pub query: Map<String, Value>,
    pub body: Option<Value>,
}

impl ProposalRequest {
    /// As one JSON object (`{"params", "query", "body"}`), how adapters store it.
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("params".to_string(), Value::Object(self.params.clone()));
        object.insert("query".to_string(), Value::Object(self.query.clone()));
        object.insert("body".to_string(), self.body.clone().unwrap_or(Value::Null));
        Value::Object(object)
    }

    /// `to_json`'s inverse; `None` for anything else.
    pub fn from_json(value: Value) -> Option<Self> {
        let Value::Object(mut object) = value else {
            return None;
        };
        let Some(Value::Object(params)) = object.remove("params") else {
            return None;
        };
        let Some(Value::Object(query)) = object.remove("query") else {
            return None;
        };
        let body = match object.remove("body") {
            None | Some(Value::Null) => None,
            Some(body) => Some(body),
        };
        Some(Self {
            params,
            query,
            body,
        })
    }
}

/// An HTTP answer kept on a decided proposal: the applied write's response, or why it
/// failed (Hypha's error body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalResponse {
    status: u16,
    body: Value,
}

impl ProposalResponse {
    pub fn new(status: u16, body: Value) -> Result<Self, ProposalError> {
        if !(100..=599).contains(&status) {
            return Err(ProposalError::InvalidStatus(status));
        }
        check_size("response", &body, MAX_PREVIEW_BYTES)?;
        Ok(Self { status, body })
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn body(&self) -> &Value {
        &self.body
    }
}

fn check_size(what: &'static str, value: &Value, max: usize) -> Result<(), ProposalError> {
    // Serializing a `Value` can't fail: its keys are strings.
    let len = serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len());
    if len > max {
        return Err(ProposalError::TooLarge { what, max });
    }
    Ok(())
}

/// What a new proposal is made of: the write as the agent sent it, and its preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProposal {
    pub agent: AgentId,
    pub operation: OperationId,
    pub request: ProposalRequest,
    pub idempotency_key: Option<IdempotencyKey>,
    /// The preview as the host made it; opaque here.
    pub preview: Value,
    /// The preview's event digest, which applying must match.
    pub digest: String,
    /// What a staff member must hold to approve it.
    pub scopes: Vec<Scope>,
    pub expires_at: Timestamp,
}

/// A write an agent previewed and left for a person: approving applies it as previewed
/// (or fails), rejecting or letting it expire drops it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    id: ProposalId,
    version: Version,
    stamps: Option<Stamps>,
    agent: AgentId,
    operation: OperationId,
    request: ProposalRequest,
    idempotency_key: Option<IdempotencyKey>,
    preview: Value,
    digest: String,
    scopes: Vec<Scope>,
    status: ProposalStatus,
    decided_by: Option<UserId>,
    decided_at: Option<Timestamp>,
    note: Option<String>,
    response: Option<ProposalResponse>,
    expires_at: Timestamp,
    /// How many times approving found it stale and previewed it again.
    refreshes: u32,
    events: Recorder,
}

fn check_digest(digest: &str) -> Result<(), ProposalError> {
    let visible = digest.bytes().all(|b| b.is_ascii_graphic());
    if digest.is_empty() || digest.len() > MAX_DIGEST_LEN || !visible {
        return Err(ProposalError::InvalidDigest(digest.to_string()));
    }
    Ok(())
}

fn sorted_once(mut scopes: Vec<Scope>) -> Vec<Scope> {
    scopes.sort_by_key(|s| Scope::ALL.iter().position(|a| a == s));
    scopes.dedup();
    scopes
}

impl Proposal {
    /// A new pending proposal. Path parameters must be text; the request and preview are
    /// bounded in size.
    pub fn new(id: ProposalId, new: NewProposal) -> Result<Self, ProposalError> {
        if let Some((name, _)) = new.request.params.iter().find(|(_, v)| !v.is_string()) {
            return Err(ProposalError::InvalidParam(name.clone()));
        }
        check_size("request", &new.request.to_json(), MAX_REQUEST_BYTES)?;
        check_size("preview", &new.preview, MAX_PREVIEW_BYTES)?;
        check_digest(&new.digest)?;
        let mut proposal = Self {
            id,
            version: Version::NEW,
            stamps: None,
            agent: new.agent,
            operation: new.operation,
            request: new.request,
            idempotency_key: new.idempotency_key,
            preview: new.preview,
            digest: new.digest,
            scopes: sorted_once(new.scopes),
            status: ProposalStatus::Pending,
            decided_by: None,
            decided_at: None,
            note: None,
            response: None,
            expires_at: new.expires_at,
            refreshes: 0,
            events: Recorder::default(),
        };
        let created = ProposalCreated {
            proposal: id,
            agent: proposal.agent,
            operation: proposal.operation.to_string(),
            scopes: proposal.scopes.iter().map(|s| s.as_str()).collect(),
        };
        proposal.events.record(proposal.subject(), &created);
        Ok(proposal)
    }

    fn subject(&self) -> Subject {
        Subject::new(PROPOSAL, self.id.as_uuid())
    }

    /// Fails unless it's open at `now`: pending, or applying past `APPLYING_LEASE`.
    fn open(&self, now: Timestamp) -> Result<(), ProposalError> {
        match self.status {
            ProposalStatus::Pending => Ok(()),
            ProposalStatus::Applying
                if self
                    .decided_at
                    .is_some_and(|since| since.plus(APPLYING_LEASE) <= now) =>
            {
                Ok(())
            }
            ProposalStatus::Applying => Err(ProposalError::Applying),
            done => Err(ProposalError::Decided(done)),
        }
    }

    /// Fails unless an approval is applying it.
    fn applying(&self) -> Result<(), ProposalError> {
        match self.status {
            ProposalStatus::Applying => Ok(()),
            other => Err(ProposalError::NotApplying(other)),
        }
    }

    /// Whether it's past its lifetime at `now` (whatever its status).
    pub fn is_expired(&self, now: Timestamp) -> bool {
        now >= self.expires_at
    }

    /// Fails unless a person may decide it at `now`: open and within its lifetime.
    pub fn check_decidable(&self, now: Timestamp) -> Result<(), ProposalError> {
        self.open(now)?;
        if self.is_expired(now) {
            return Err(ProposalError::Expired);
        }
        Ok(())
    }

    /// Approved by `by` at `now`: applying until its write's answer is recorded
    /// (`applied`, `failed`, `refresh`, `release`). Nobody else decides it meanwhile.
    pub fn start_applying(&mut self, by: UserId, now: Timestamp) -> Result<(), ProposalError> {
        self.check_decidable(now)?;
        self.status = ProposalStatus::Applying;
        self.decided_by = Some(by);
        self.decided_at = Some(now);
        Ok(())
    }

    /// Pending again: its approved write wasn't made and may go through later.
    pub fn release(&mut self) -> Result<(), ProposalError> {
        self.applying()?;
        self.status = ProposalStatus::Pending;
        self.decided_by = None;
        self.decided_at = None;
        Ok(())
    }

    fn decide(&mut self, status: ProposalStatus, by: Option<UserId>, at: Timestamp) {
        self.status = status;
        self.decided_by = by;
        self.decided_at = Some(at);
        let decided = ProposalDecided {
            proposal: self.id,
            agent: self.agent,
            status: status.as_str(),
            decided_by: by,
        };
        self.events.record(self.subject(), &decided);
    }

    /// Its write was applied, with `response`. Not checked against the lifetime: the
    /// approval was (`check_decidable`), and applying takes a moment.
    pub fn applied(
        &mut self,
        by: UserId,
        at: Timestamp,
        response: ProposalResponse,
    ) -> Result<(), ProposalError> {
        self.applying()?;
        self.response = Some(response);
        self.decide(ProposalStatus::Applied, Some(by), at);
        Ok(())
    }

    /// Approved, but its write failed with `response` (Hypha's error body).
    pub fn failed(
        &mut self,
        by: UserId,
        at: Timestamp,
        response: ProposalResponse,
    ) -> Result<(), ProposalError> {
        self.applying()?;
        self.response = Some(response);
        self.decide(ProposalStatus::Failed, Some(by), at);
        Ok(())
    }

    /// Approving found the store changed since its preview: it now holds `preview` (with
    /// the IDs the new run was handed) and `digest`, what the write would do today, and
    /// is pending again for a person to review and approve again.
    pub fn refresh(
        &mut self,
        by: UserId,
        preview: Value,
        digest: String,
    ) -> Result<(), ProposalError> {
        self.applying()?;
        check_size("preview", &preview, MAX_PREVIEW_BYTES)?;
        check_digest(&digest)?;
        self.release()?;
        self.preview = preview;
        self.digest = digest;
        self.refreshes = self.refreshes.saturating_add(1);
        let updated = ProposalUpdated {
            proposal: self.id,
            agent: self.agent,
            refreshes: self.refreshes,
            by,
        };
        self.events.record(self.subject(), &updated);
        Ok(())
    }

    /// Rejected, with an optional note for the agent (trimmed; blank is none).
    pub fn reject(
        &mut self,
        by: UserId,
        at: Timestamp,
        note: Option<String>,
    ) -> Result<(), ProposalError> {
        self.check_decidable(at)?;
        let note = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        if note
            .as_ref()
            .is_some_and(|n| n.chars().count() > MAX_NOTE_LEN)
        {
            return Err(ProposalError::NoteTooLong { max: MAX_NOTE_LEN });
        }
        self.note = note;
        self.decide(ProposalStatus::Rejected, Some(by), at);
        Ok(())
    }

    /// Expired at `at`, which must be at or past its lifetime.
    pub fn expire(&mut self, at: Timestamp) -> Result<(), ProposalError> {
        self.open(at)?;
        if !self.is_expired(at) {
            return Err(ProposalError::NotDue);
        }
        self.decide(ProposalStatus::Expired, None, at);
        Ok(())
    }

    pub fn id(&self) -> ProposalId {
        self.id
    }

    pub fn agent(&self) -> AgentId {
        self.agent
    }

    pub fn operation(&self) -> &OperationId {
        &self.operation
    }

    pub fn request(&self) -> &ProposalRequest {
        &self.request
    }

    pub fn idempotency_key(&self) -> Option<&IdempotencyKey> {
        self.idempotency_key.as_ref()
    }

    pub fn preview(&self) -> &Value {
        &self.preview
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// In `Scope::ALL` order, each once.
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    pub fn status(&self) -> ProposalStatus {
        self.status
    }

    pub fn decided_by(&self) -> Option<UserId> {
        self.decided_by
    }

    pub fn decided_at(&self) -> Option<Timestamp> {
        self.decided_at
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn response(&self) -> Option<&ProposalResponse> {
        self.response.as_ref()
    }

    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    pub fn refreshes(&self) -> u32 {
        self.refreshes
    }

    pub fn snapshot(&self) -> ProposalSnapshot {
        ProposalSnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            agent: self.agent,
            operation: self.operation.clone(),
            request: self.request.clone(),
            idempotency_key: self.idempotency_key.clone(),
            preview: self.preview.clone(),
            digest: self.digest.clone(),
            scopes: self.scopes.clone(),
            status: self.status,
            decided_by: self.decided_by,
            decided_at: self.decided_at,
            note: self.note.clone(),
            response: self.response.clone(),
            expires_at: self.expires_at,
            refreshes: self.refreshes,
        }
    }

    /// Rebuilds a proposal from storage, with the same checks as `new` and its decision
    /// consistent with its status: pending has none, the rest a time, a person for all but
    /// `expired`, a response for `applied` and `failed`.
    pub fn restore(s: ProposalSnapshot) -> Result<Proposal, ProposalError> {
        let mut proposal = Proposal::new(
            s.id,
            NewProposal {
                agent: s.agent,
                operation: s.operation,
                request: s.request,
                idempotency_key: s.idempotency_key,
                preview: s.preview,
                digest: s.digest,
                scopes: s.scopes,
                expires_at: s.expires_at,
            },
        )?;
        let by_person = matches!(
            s.status,
            ProposalStatus::Applying
                | ProposalStatus::Applied
                | ProposalStatus::Rejected
                | ProposalStatus::Failed
        );
        let with_response = matches!(s.status, ProposalStatus::Applied | ProposalStatus::Failed);
        let note_fits = s
            .note
            .as_ref()
            .is_none_or(|n| !n.trim().is_empty() && n.chars().count() <= MAX_NOTE_LEN);
        let consistent = note_fits
            && match s.status {
                ProposalStatus::Pending => {
                    s.decided_at.is_none()
                        && s.decided_by.is_none()
                        && s.response.is_none()
                        && s.note.is_none()
                }
                _ => {
                    s.decided_at.is_some()
                        && s.decided_by.is_some() == by_person
                        && s.response.is_some() == with_response
                        && (s.status == ProposalStatus::Rejected || s.note.is_none())
                }
            };
        if !consistent {
            return Err(ProposalError::Inconsistent);
        }
        proposal.version = s.version;
        proposal.stamps = s.stamps;
        proposal.status = s.status;
        proposal.decided_by = s.decided_by;
        proposal.decided_at = s.decided_at;
        proposal.note = s.note;
        proposal.response = s.response;
        proposal.refreshes = s.refreshes;
        proposal.events = Recorder::default();
        Ok(proposal)
    }
}

/// Plain-data form of a `Proposal` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalSnapshot {
    pub id: ProposalId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub agent: AgentId,
    pub operation: OperationId,
    pub request: ProposalRequest,
    pub idempotency_key: Option<IdempotencyKey>,
    pub preview: Value,
    pub digest: String,
    pub scopes: Vec<Scope>,
    pub status: ProposalStatus,
    pub decided_by: Option<UserId>,
    pub decided_at: Option<Timestamp>,
    pub note: Option<String>,
    pub response: Option<ProposalResponse>,
    pub expires_at: Timestamp,
    pub refreshes: u32,
}

impl Recorded for Proposal {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for Proposal {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Proposal {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn id() -> ProposalId {
        ProposalId::from_uuid(Uuid::from_u128(1))
    }

    fn agent() -> AgentId {
        AgentId::from_uuid(Uuid::from_u128(2))
    }

    fn user() -> UserId {
        UserId::from_uuid(Uuid::from_u128(3))
    }

    fn at(micros: i64) -> Timestamp {
        Timestamp::from_unix_micros(micros)
    }

    fn request() -> ProposalRequest {
        let mut params = Map::new();
        params.insert("id".to_string(), json!("prod_1"));
        let mut query = Map::new();
        query.insert("notify".to_string(), json!(true));
        ProposalRequest {
            params,
            query,
            body: Some(json!({"title": "Mug"})),
        }
    }

    fn new() -> NewProposal {
        NewProposal {
            agent: agent(),
            operation: OperationId::new("admin.products.update").unwrap(),
            request: request(),
            idempotency_key: Some(IdempotencyKey::new("k1").unwrap()),
            preview: json!({"complete": true}),
            digest: "sha256:abc".to_string(),
            scopes: vec![
                Scope::InventoryWrite,
                Scope::ProductsWrite,
                Scope::InventoryWrite,
            ],
            expires_at: at(100),
        }
    }

    fn pending() -> Proposal {
        let mut p = Proposal::new(id(), new()).unwrap();
        p.take_events();
        p
    }

    fn response() -> ProposalResponse {
        ProposalResponse::new(200, json!({"id": "prod_1"})).unwrap()
    }

    fn data(p: &mut Proposal) -> Vec<(&'static str, serde_json::Value)> {
        p.take_events()
            .into_iter()
            .map(|e| (e.event_type, e.data))
            .collect()
    }

    #[test]
    fn statuses_parse_their_own_strings_only() {
        for status in ProposalStatus::ALL {
            assert_eq!(ProposalStatus::parse(status.as_str()), Ok(status));
        }
        assert_eq!(
            ProposalStatus::parse("approved"),
            Err("approved".to_string())
        );
    }

    #[test]
    fn a_request_round_trips_through_json() {
        let r = request();
        assert_eq!(
            r.to_json(),
            json!({"params": {"id": "prod_1"}, "query": {"notify": true}, "body": {"title": "Mug"}})
        );
        assert_eq!(ProposalRequest::from_json(r.to_json()), Some(r));
        let bodiless = ProposalRequest::default();
        assert_eq!(
            ProposalRequest::from_json(bodiless.to_json()),
            Some(bodiless)
        );
        assert_eq!(ProposalRequest::from_json(json!({"params": {}})), None);
        assert_eq!(ProposalRequest::from_json(json!([])), None);
    }

    #[test]
    fn a_new_proposal_is_pending_with_its_scopes_in_order_once() {
        let mut p = Proposal::new(id(), new()).unwrap();
        assert_eq!(p.status(), ProposalStatus::Pending);
        assert_eq!(p.scopes(), [Scope::ProductsWrite, Scope::InventoryWrite]);
        assert_eq!(
            data(&mut p),
            [(
                "proposal.created",
                json!({
                    "proposal": id().to_string(),
                    "agent": agent().to_string(),
                    "operation": "admin.products.update",
                    "scopes": ["products:write", "inventory:write"]
                })
            )]
        );
    }

    #[test]
    fn path_parameters_are_text() {
        let mut n = new();
        n.request.params.insert("n".to_string(), json!(1));
        assert_eq!(
            Proposal::new(id(), n),
            Err(ProposalError::InvalidParam("n".to_string()))
        );
    }

    #[test]
    fn the_digest_is_visible_ascii_and_short() {
        let longest = "a".repeat(MAX_DIGEST_LEN);
        let with = |digest: &str| NewProposal {
            digest: digest.to_string(),
            ..new()
        };
        assert!(Proposal::new(id(), with(&longest)).is_ok());
        for bad in [String::new(), format!("{longest}a"), "sha 256".to_string()] {
            assert_eq!(
                Proposal::new(id(), with(&bad)),
                Err(ProposalError::InvalidDigest(bad.clone())),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn previews_and_requests_are_bounded() {
        let big = NewProposal {
            preview: json!("x".repeat(MAX_PREVIEW_BYTES)),
            ..new()
        };
        assert_eq!(
            Proposal::new(id(), big),
            Err(ProposalError::TooLarge {
                what: "preview",
                max: MAX_PREVIEW_BYTES
            })
        );
        let mut n = new();
        n.request.body = Some(json!("x".repeat(MAX_REQUEST_BYTES)));
        assert_eq!(
            Proposal::new(id(), n),
            Err(ProposalError::TooLarge {
                what: "request",
                max: MAX_REQUEST_BYTES
            })
        );
    }

    #[test]
    fn a_response_has_an_http_status() {
        assert!(ProposalResponse::new(100, json!(null)).is_ok());
        assert!(ProposalResponse::new(599, json!(null)).is_ok());
        assert_eq!(
            ProposalResponse::new(99, json!(null)),
            Err(ProposalError::InvalidStatus(99))
        );
        assert_eq!(
            ProposalResponse::new(600, json!(null)),
            Err(ProposalError::InvalidStatus(600))
        );
    }

    /// Pending, then approved at 10: its write running.
    fn applying() -> Proposal {
        let mut p = pending();
        p.start_applying(user(), at(10)).unwrap();
        p
    }

    #[test]
    fn approving_holds_it_until_the_answer_or_the_lease() {
        let mut p = applying();
        assert_eq!(p.status(), ProposalStatus::Applying);
        assert_eq!(p.decided_by(), Some(user()));
        assert!(data(&mut p).is_empty(), "nothing decided yet");
        let lapses = at(10).plus(APPLYING_LEASE);
        let held = Err(ProposalError::Applying);
        assert_eq!(p.open(lapses.minus(Duration::from_micros(1))), held);
        assert_eq!(p.clone().reject(user(), at(11), None), held);
        assert_eq!(p.clone().start_applying(user(), at(11)), held);
        assert_eq!(p.open(lapses), Ok(()), "the lease passed");
        p.release().unwrap();
        assert_eq!(
            (p.status(), p.decided_by(), p.decided_at()),
            (ProposalStatus::Pending, None, None)
        );
        assert_eq!(
            p.release(),
            Err(ProposalError::NotApplying(ProposalStatus::Pending))
        );
    }

    #[test]
    fn applying_records_who_when_and_the_response() {
        let mut p = applying();
        p.applied(user(), at(50), response()).unwrap();
        assert_eq!(p.status(), ProposalStatus::Applied);
        assert_eq!(p.decided_by(), Some(user()));
        assert_eq!(p.decided_at(), Some(at(50)));
        assert_eq!(p.response(), Some(&response()));
        assert_eq!(
            data(&mut p),
            [(
                "proposal.decided",
                json!({
                    "proposal": id().to_string(),
                    "agent": agent().to_string(),
                    "status": "applied",
                    "decided_by": user().to_string()
                })
            )]
        );
    }

    #[test]
    fn applying_may_finish_after_the_lifetime() {
        let mut p = applying();
        p.applied(user(), at(100), response()).unwrap();
        let mut p = applying();
        p.failed(user(), at(101), response()).unwrap();
        assert_eq!(p.status(), ProposalStatus::Failed);
    }

    #[test]
    fn deciding_is_open_until_the_lifetime_ends() {
        let p = pending();
        assert_eq!(p.check_decidable(at(99)), Ok(()));
        assert_eq!(p.check_decidable(at(100)), Err(ProposalError::Expired));
        let mut late = pending();
        assert_eq!(
            late.reject(user(), at(100), None),
            Err(ProposalError::Expired)
        );
    }

    #[test]
    fn rejecting_keeps_a_trimmed_note() {
        let mut p = pending();
        p.reject(user(), at(10), Some("  not this price ".to_string()))
            .unwrap();
        assert_eq!(p.status(), ProposalStatus::Rejected);
        assert_eq!(p.note(), Some("not this price"));
        assert_eq!(p.decided_by(), Some(user()));
        let events = data(&mut p);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["status"], "rejected");
        let mut blank = pending();
        blank.reject(user(), at(10), Some(" ".to_string())).unwrap();
        assert_eq!(blank.note(), None);
    }

    #[test]
    fn a_note_is_at_most_max_note_len_characters() {
        let mut p = pending();
        assert!(
            p.clone()
                .reject(user(), at(1), Some("é".repeat(MAX_NOTE_LEN)))
                .is_ok()
        );
        assert_eq!(
            p.reject(user(), at(1), Some("é".repeat(MAX_NOTE_LEN + 1))),
            Err(ProposalError::NoteTooLong { max: MAX_NOTE_LEN })
        );
    }

    #[test]
    fn expiring_waits_for_the_lifetime_and_names_no_one() {
        let mut p = pending();
        assert_eq!(p.expire(at(99)), Err(ProposalError::NotDue));
        p.expire(at(100)).unwrap();
        assert_eq!(p.status(), ProposalStatus::Expired);
        assert_eq!(p.decided_by(), None);
        assert_eq!(data(&mut p)[0].1["decided_by"], json!(null));
    }

    #[test]
    fn only_pending_moves() {
        let mut decided = applying();
        decided.applied(user(), at(1), response()).unwrap();
        let done = Err(ProposalError::Decided(ProposalStatus::Applied));
        let not_applying = Err(ProposalError::NotApplying(ProposalStatus::Applied));
        assert_eq!(
            decided.clone().applied(user(), at(2), response()),
            not_applying
        );
        assert_eq!(
            decided.clone().failed(user(), at(2), response()),
            not_applying
        );
        assert_eq!(
            pending().applied(user(), at(2), response()),
            Err(ProposalError::NotApplying(ProposalStatus::Pending)),
            "only an approval records an answer"
        );
        assert_eq!(decided.clone().reject(user(), at(2), None), done);
        assert_eq!(decided.clone().expire(at(200)), done);
        assert_eq!(decided.check_decidable(at(2)), done);
    }

    #[test]
    fn restore_round_trips_every_status() {
        let mut applied = applying();
        applied.applied(user(), at(5), response()).unwrap();
        let mut rejected = pending();
        rejected
            .reject(user(), at(5), Some("no".to_string()))
            .unwrap();
        let mut expired = pending();
        expired.expire(at(100)).unwrap();
        let mut failed = applying();
        failed.failed(user(), at(5), response()).unwrap();
        for mut p in [pending(), applying(), applied, rejected, expired, failed] {
            p.take_events();
            p.set_version(Version::new(2));
            assert_eq!(Proposal::restore(p.snapshot()), Ok(p));
        }
    }

    #[test]
    fn restore_refuses_a_decision_that_doesnt_match_the_status() {
        let mut s = pending().snapshot();
        s.decided_at = Some(at(1));
        assert_eq!(Proposal::restore(s), Err(ProposalError::Inconsistent));
        let mut s = pending().snapshot();
        s.status = ProposalStatus::Applied;
        s.decided_at = Some(at(1));
        s.decided_by = Some(user());
        assert_eq!(
            Proposal::restore(s.clone()),
            Err(ProposalError::Inconsistent),
            "applied without a response"
        );
        s.status = ProposalStatus::Expired;
        assert_eq!(
            Proposal::restore(s),
            Err(ProposalError::Inconsistent),
            "expired by someone"
        );
    }

    #[test]
    fn restore_refuses_a_blank_or_overlong_note() {
        for note in ["  ".to_string(), "x".repeat(MAX_NOTE_LEN + 1)] {
            let mut s = pending().snapshot();
            s.note = Some(note);
            assert_eq!(Proposal::restore(s), Err(ProposalError::Inconsistent));
        }
    }

    #[test]
    fn a_preview_may_weigh_exactly_its_bound() {
        // A JSON string's quotes count.
        let full = NewProposal {
            preview: json!("x".repeat(MAX_PREVIEW_BYTES - 2)),
            ..new()
        };
        assert!(Proposal::new(id(), full).is_ok());
    }

    #[test]
    fn the_bounds_are_a_megabyte_four_and_five_minutes() {
        assert_eq!(MAX_REQUEST_BYTES, 1 << 20);
        assert_eq!(MAX_PREVIEW_BYTES, 4 << 20);
        assert_eq!(APPLYING_LEASE, Duration::from_secs(300));
    }

    #[test]
    fn a_stale_proposal_is_previewed_again_and_is_pending_again() {
        let mut p = applying();
        p.refresh(
            user(),
            json!({"complete": true, "n": 2}),
            "sha256:def".to_string(),
        )
        .unwrap();
        assert_eq!(p.status(), ProposalStatus::Pending);
        assert_eq!(p.preview(), &json!({"complete": true, "n": 2}));
        assert_eq!(p.digest(), "sha256:def");
        assert_eq!(p.refreshes(), 1);
        let events = data(&mut p);
        assert_eq!(events[0].0, "proposal.updated");
        assert_eq!(events[0].1["refreshes"], json!(1));
        p.start_applying(user(), at(11)).unwrap();
        p.refresh(user(), json!({}), "sha256:ghi".to_string())
            .unwrap();
        assert_eq!(p.refreshes(), 2);
        let mut again = p.clone();
        again.start_applying(user(), at(12)).unwrap();
        assert_eq!(
            again.refresh(user(), json!({}), String::new()),
            Err(ProposalError::InvalidDigest(String::new()))
        );
        p.take_events();
        assert_eq!(Proposal::restore(p.snapshot()), Ok(p.clone()));
        p.start_applying(user(), at(12)).unwrap();
        p.applied(user(), at(13), response()).unwrap();
        assert_eq!(
            p.refresh(user(), json!({}), "sha256:x".to_string()),
            Err(ProposalError::NotApplying(ProposalStatus::Applied))
        );
    }
}
