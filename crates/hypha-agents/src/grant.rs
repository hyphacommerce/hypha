//! What staff let an agent do: read scopes, write scopes each with a mode, personal data,
//! and optionally a list of the only operations it may call. And what that comes to at a
//! request, once the deployment's cap and (for a connected agent) its person's role have
//! had their say: `effective`.

use std::fmt;

use hypha_core::{ErrorCode, Scope};
use thiserror::Error;

use crate::{OperationId, OperationIdError};

/// How an agent may use a write scope: `Propose` previews the write and keeps it for a
/// person to approve; `Apply` writes at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WriteMode {
    Propose,
    Apply,
}

impl WriteMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WriteMode::Propose => "propose",
            WriteMode::Apply => "apply",
        }
    }

    /// The mode `as_str` spells, or the text that isn't one.
    pub fn parse(s: &str) -> Result<Self, String> {
        [WriteMode::Propose, WriteMode::Apply]
            .into_iter()
            .find(|m| m.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

impl fmt::Display for WriteMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Scopes no agent is granted in any mode: with them it could widen its own reach or mint
/// credentials (staff, keys, apps, agents), or act as a shopper (`checkout`: agentic
/// shopping has its own adapter). Fixed in code, not a setting.
pub const NEVER_GRANTED: [Scope; 5] = [
    Scope::Checkout,
    Scope::UsersWrite,
    Scope::ApiKeysWrite,
    Scope::AppsWrite,
    Scope::AgentsWrite,
];

/// Write scopes an agent may only propose: a person approves every use.
pub const PROPOSE_ONLY: [Scope; 1] = [Scope::CustomersErase];

/// The most operations an allowlist names: more than the admin surface has.
pub const MAX_OPERATIONS: usize = 1000;

/// Which list of a grant a problem is in: where a form shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantField {
    Read,
    Write,
    CustomerData,
    Operations,
}

impl GrantField {
    pub fn as_str(self) -> &'static str {
        match self {
            GrantField::Read => "read",
            GrantField::Write => "write",
            GrantField::CustomerData => "customer_data",
            GrantField::Operations => "operations",
        }
    }

    /// The list a scope of this kind belongs in.
    pub fn of(scope: Scope) -> Self {
        match kind(scope) {
            Kind::Read => GrantField::Read,
            Kind::Write => GrantField::Write,
            Kind::CustomerData => GrantField::CustomerData,
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum GrantError {
    /// A name in `field` that isn't a scope.
    #[error("{1:?} is not a scope")]
    UnknownScope(GrantField, String),
    #[error("{mode:?} is not a write mode (\"propose\" or \"apply\")")]
    UnknownWriteMode { scope: String, mode: String },
    /// Listed in `field`.
    #[error("{1} is never granted to an agent")]
    NotGrantable(GrantField, Scope),
    #[error("{0} is granted to an agent to propose only")]
    ProposeOnly(Scope),
    #[error("{0} is not a read scope")]
    NotARead(Scope),
    #[error("{0} is not a write scope")]
    NotAWrite(Scope),
    #[error("{0} is not a customer_data scope")]
    NotCustomerData(Scope),
    #[error("{0} is listed twice in the write scopes")]
    DuplicateWrite(Scope),
    #[error(transparent)]
    Operation(#[from] OperationIdError),
    #[error("an allowlist names at most {max} operations")]
    TooManyOperations { max: usize },
}

impl ErrorCode for GrantError {
    fn code(&self) -> &'static str {
        match self {
            GrantError::UnknownScope(..) => "unknown_scope",
            GrantError::UnknownWriteMode { .. } => "invalid_write_mode",
            GrantError::NotGrantable(..) | GrantError::ProposeOnly(_) => {
                "agent_scope_not_grantable"
            }
            GrantError::NotARead(_)
            | GrantError::NotAWrite(_)
            | GrantError::NotCustomerData(_)
            | GrantError::DuplicateWrite(_) => "invalid_grant",
            GrantError::Operation(e) => e.code(),
            GrantError::TooManyOperations { .. } => "too_many_operations",
        }
    }
}

/// Which list of a grant a scope belongs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Read,
    Write,
    CustomerData,
}

fn kind(scope: Scope) -> Kind {
    if scope.is_personal_data() {
        Kind::CustomerData
    } else if scope.implied_by().is_some() {
        Kind::Read
    } else {
        Kind::Write
    }
}

fn position(scope: &Scope) -> usize {
    Scope::ALL
        .iter()
        .position(|s| s == scope)
        .unwrap_or(Scope::ALL.len())
}

/// A grant as a client sends it: scope names as text, checked by `Grant::new`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrantInput {
    pub read: Vec<String>,
    /// Each write scope with its mode: `("products:write", "propose")`.
    pub write: Vec<(String, String)>,
    pub customer_data: Vec<String>,
    /// `None`: every operation the scopes allow.
    pub operations: Option<Vec<String>>,
}

/// What staff granted an agent. Each list in `Scope::ALL` order, each scope once; the
/// allowlist sorted, each operation once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grant {
    read: Vec<Scope>,
    write: Vec<(Scope, WriteMode)>,
    customer_data: Vec<Scope>,
    operations: Option<Vec<OperationId>>,
}

fn parse_scope(field: GrantField, name: &str) -> Result<Scope, GrantError> {
    Scope::parse(name).ok_or_else(|| GrantError::UnknownScope(field, name.to_string()))
}

impl GrantError {
    /// Where the problem is, for a form: the list, and the scope or operation it names.
    pub fn field(&self) -> GrantField {
        match self {
            GrantError::UnknownScope(field, _) | GrantError::NotGrantable(field, _) => *field,
            GrantError::UnknownWriteMode { .. }
            | GrantError::ProposeOnly(_)
            | GrantError::NotAWrite(_)
            | GrantError::DuplicateWrite(_) => GrantField::Write,
            GrantError::NotARead(_) => GrantField::Read,
            GrantError::NotCustomerData(_) => GrantField::CustomerData,
            GrantError::Operation(_) | GrantError::TooManyOperations { .. } => {
                GrantField::Operations
            }
        }
    }

    /// The scope it's about, as named (known or not).
    pub fn scope(&self) -> Option<String> {
        match self {
            GrantError::UnknownScope(_, name) => Some(name.clone()),
            GrantError::UnknownWriteMode { scope, .. } => Some(scope.clone()),
            GrantError::NotGrantable(_, s)
            | GrantError::ProposeOnly(s)
            | GrantError::NotARead(s)
            | GrantError::NotAWrite(s)
            | GrantError::NotCustomerData(s)
            | GrantError::DuplicateWrite(s) => Some(s.as_str().to_string()),
            GrantError::Operation(_) | GrantError::TooManyOperations { .. } => None,
        }
    }

    /// The operation it's about, as sent.
    pub fn operation(&self) -> Option<&str> {
        match self {
            GrantError::Operation(e) => Some(&e.0),
            _ => None,
        }
    }
}

fn sorted_once(mut scopes: Vec<Scope>) -> Vec<Scope> {
    scopes.sort_by_key(position);
    scopes.dedup();
    scopes
}

impl Grant {
    /// Checks a grant from text: every name a scope, in the right list, grantable to an
    /// agent in its mode; the operations shaped like operation IDs. Repeated read or
    /// customer data scopes count once; a write scope named twice is an error (which mode
    /// would it be?).
    pub fn new(input: GrantInput) -> Result<Self, GrantError> {
        let read = input
            .read
            .iter()
            .map(|s| parse_scope(GrantField::Read, s))
            .collect::<Result<Vec<_>, _>>()?;
        let write = input
            .write
            .iter()
            .map(|(s, m)| {
                let scope = parse_scope(GrantField::Write, s)?;
                let mode = WriteMode::parse(m).map_err(|mode| GrantError::UnknownWriteMode {
                    scope: s.clone(),
                    mode,
                })?;
                Ok((scope, mode))
            })
            .collect::<Result<Vec<_>, GrantError>>()?;
        let customer_data = input
            .customer_data
            .iter()
            .map(|s| parse_scope(GrantField::CustomerData, s))
            .collect::<Result<Vec<_>, _>>()?;
        let operations = input
            .operations
            .map(|ops| {
                ops.iter()
                    .map(OperationId::new)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        Self::from_parts(read, write, customer_data, operations)
    }

    /// The same rules as `new`, on typed values (and how storage restores one).
    pub fn from_parts(
        read: Vec<Scope>,
        write: Vec<(Scope, WriteMode)>,
        customer_data: Vec<Scope>,
        operations: Option<Vec<OperationId>>,
    ) -> Result<Self, GrantError> {
        let all = read
            .iter()
            .map(|s| (GrantField::Read, s))
            .chain(write.iter().map(|(s, _)| (GrantField::Write, s)))
            .chain(customer_data.iter().map(|s| (GrantField::CustomerData, s)));
        if let Some((field, never)) = all.into_iter().find(|(_, s)| NEVER_GRANTED.contains(s)) {
            return Err(GrantError::NotGrantable(field, *never));
        }
        if let Some(s) = read.iter().find(|s| kind(**s) != Kind::Read) {
            return Err(GrantError::NotARead(*s));
        }
        if let Some((s, _)) = write.iter().find(|(s, _)| kind(*s) != Kind::Write) {
            return Err(GrantError::NotAWrite(*s));
        }
        if let Some(s) = customer_data
            .iter()
            .find(|s| kind(**s) != Kind::CustomerData)
        {
            return Err(GrantError::NotCustomerData(*s));
        }
        if let Some((s, _)) = write
            .iter()
            .find(|(s, m)| *m == WriteMode::Apply && PROPOSE_ONLY.contains(s))
        {
            return Err(GrantError::ProposeOnly(*s));
        }
        let mut write = write;
        write.sort_by_key(|(s, _)| position(s));
        if let Some(pair) = write.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(GrantError::DuplicateWrite(pair[0].0));
        }
        let operations = match operations {
            Some(mut ops) => {
                ops.sort();
                ops.dedup();
                if ops.len() > MAX_OPERATIONS {
                    return Err(GrantError::TooManyOperations {
                        max: MAX_OPERATIONS,
                    });
                }
                Some(ops)
            }
            None => None,
        };
        Ok(Self {
            read: sorted_once(read),
            write,
            customer_data: sorted_once(customer_data),
            operations,
        })
    }

    pub fn read(&self) -> &[Scope] {
        &self.read
    }

    pub fn write(&self) -> &[(Scope, WriteMode)] {
        &self.write
    }

    pub fn customer_data(&self) -> &[Scope] {
        &self.customer_data
    }

    /// The only operations the agent may call; `None` for every one its scopes allow.
    pub fn operations(&self) -> Option<&[OperationId]> {
        self.operations.as_deref()
    }

    /// The write scopes in `mode`, in `Scope::ALL` order.
    pub fn writes_in(&self, mode: WriteMode) -> Vec<Scope> {
        self.write
            .iter()
            .filter(|(_, m)| *m == mode)
            .map(|(s, _)| *s)
            .collect()
    }

    /// Every scope the grant names, in any list, in `Scope::ALL` order: what whoever grants
    /// it must hold.
    pub fn scopes(&self) -> Vec<Scope> {
        sorted_once(
            self.read
                .iter()
                .chain(self.write.iter().map(|(s, _)| s))
                .chain(self.customer_data.iter())
                .copied()
                .collect(),
        )
    }

    /// The same grant with another allowlist (checked like `from_parts`).
    pub fn with_operations(self, operations: Option<Vec<OperationId>>) -> Result<Self, GrantError> {
        Self::from_parts(self.read, self.write, self.customer_data, operations)
    }
}

/// The grant a consent page offers a person connecting a client, for them to change: every
/// scope they `hold` that an agent may have, reads as reads and writes to `propose`, no
/// personal data and no allowlist. A client that asked for scopes (`requested`, non-empty)
/// is offered only those, with the reads its writes imply.
pub fn suggested_grant(holds: impl Fn(Scope) -> bool, requested: &[Scope]) -> Grant {
    let wanted = |s: Scope| {
        requested.is_empty()
            || requested.contains(&s)
            || requested.iter().any(|r| s.implied_by() == Some(*r))
    };
    let offered = || {
        Scope::ALL
            .into_iter()
            .filter(|s| !NEVER_GRANTED.contains(s) && holds(*s) && wanted(*s))
    };
    let read = offered().filter(|s| kind(*s) == Kind::Read).collect();
    let write = offered()
        .filter(|s| kind(*s) == Kind::Write)
        .map(|s| (s, WriteMode::Propose))
        .collect();
    // Every part was filtered by the rules `from_parts` checks.
    Grant::from_parts(read, write, Vec::new(), None).unwrap_or_default()
}

/// The operator's cap on what any agent may write (`agents.writes`), applied at every
/// request after the grant: `Propose` turns every `Apply` into `Propose`, `Off` leaves
/// agents read-only. The grant stays as staff set it, so lifting the cap restores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WritesCap {
    /// Grants decide.
    #[default]
    Apply,
    Propose,
    Off,
}

impl WritesCap {
    pub fn as_str(self) -> &'static str {
        match self {
            WritesCap::Apply => "apply",
            WritesCap::Propose => "propose",
            WritesCap::Off => "off",
        }
    }

    /// The cap `as_str` spells, or the text that isn't one.
    pub fn parse(s: &str) -> Result<Self, String> {
        [WritesCap::Apply, WritesCap::Propose, WritesCap::Off]
            .into_iter()
            .find(|c| c.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

/// What an agent may do with one scope at a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Access {
    Read,
    Propose,
    Apply,
}

/// What an agent may do at one request: its grant, under the deployment's cap, within its
/// person's role for a connected agent. Read scopes include those its write scopes imply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permissions {
    read: Vec<Scope>,
    write: Vec<(Scope, WriteMode)>,
    customer_data: Vec<Scope>,
    operations: Option<Vec<OperationId>>,
}

/// Whether a role's scopes (as granted, a write scope standing for its read scope too)
/// hold `scope`, like `Ctx::has_scope`.
fn role_holds(role: &[Scope], scope: Scope) -> bool {
    role.contains(&scope) || scope.implied_by().is_some_and(|w| role.contains(&w))
}

/// What `grant` comes to under `cap`, for an agent acting for a staff member whose role
/// has `role` (`None` for an autonomous agent, which has no person behind it). A pure
/// function of its inputs, run at every request, so a narrowed role or a lowered cap
/// applies at once.
pub fn effective(grant: &Grant, cap: WritesCap, role: Option<&[Scope]>) -> Permissions {
    let allowed = |s: Scope| role.is_none_or(|r| role_holds(r, s));
    let implied = grant.write.iter().filter_map(|(s, _)| {
        Scope::ALL
            .into_iter()
            .find(|read| read.implied_by() == Some(*s))
    });
    let read = sorted_once(
        grant
            .read
            .iter()
            .copied()
            .chain(implied)
            .filter(|s| allowed(*s))
            .collect(),
    );
    let write = match cap {
        WritesCap::Off => Vec::new(),
        WritesCap::Apply | WritesCap::Propose => grant
            .write
            .iter()
            .filter(|(s, _)| allowed(*s))
            .map(|(s, m)| match cap {
                WritesCap::Propose => (*s, WriteMode::Propose),
                _ => (*s, *m),
            })
            .collect(),
    };
    let customer_data = grant
        .customer_data
        .iter()
        .copied()
        .filter(|s| allowed(*s))
        .collect();
    Permissions {
        read,
        write,
        customer_data,
        operations: grant.operations.clone(),
    }
}

impl Permissions {
    /// What the agent may do with `scope`: read it, or write in a mode. `None` without it.
    /// A personal data scope is `Read` when held.
    pub fn access(&self, scope: Scope) -> Option<Access> {
        if let Some(mode) = self.write_mode(scope) {
            return Some(match mode {
                WriteMode::Propose => Access::Propose,
                WriteMode::Apply => Access::Apply,
            });
        }
        (self.read.contains(&scope) || self.customer_data.contains(&scope)).then_some(Access::Read)
    }

    pub fn write_mode(&self, scope: Scope) -> Option<WriteMode> {
        self.write
            .iter()
            .find(|(s, _)| *s == scope)
            .map(|(_, m)| *m)
    }

    /// How a write needing `scopes` may go: `Propose` when the agent holds any of its
    /// write scopes only to propose, `Apply` otherwise. Decided by the write scopes held
    /// alone: a scope the agent lacks makes the route refuse the write, never turns a
    /// proposal into a direct write.
    pub fn mode_for(&self, scopes: &[Scope]) -> WriteMode {
        let proposes = scopes
            .iter()
            .any(|s| self.write_mode(*s) == Some(WriteMode::Propose));
        match proposes {
            true => WriteMode::Propose,
            false => WriteMode::Apply,
        }
    }

    /// Whether the agent holds every one of `scopes`, a write scope in either mode.
    pub fn holds_all(&self, scopes: &[Scope]) -> bool {
        scopes.iter().all(|s| {
            !matches!(
                (kind(*s), self.access(*s)),
                (_, None) | (Kind::Write, Some(Access::Read))
            )
        })
    }

    pub fn read(&self) -> &[Scope] {
        &self.read
    }

    pub fn write(&self) -> &[(Scope, WriteMode)] {
        &self.write
    }

    pub fn customer_data(&self) -> &[Scope] {
        &self.customer_data
    }

    /// Whether the agent may call `operation`: always, without an allowlist.
    pub fn allows_operation(&self, operation: &OperationId) -> bool {
        self.operations
            .as_ref()
            .is_none_or(|ops| ops.binary_search(operation).is_ok())
    }

    pub fn operations(&self) -> Option<&[OperationId]> {
        self.operations.as_deref()
    }

    /// Every scope the agent holds in any mode, in `Scope::ALL` order: the scopes of the
    /// `Ctx` a request runs with when nothing it does is kept without a person (a preview,
    /// a proposal's preview, an approved proposal's write). A write in `Propose` mode is
    /// held here, since previewing and proposing it run the command.
    pub fn scopes(&self) -> Vec<Scope> {
        self.held(|_| true)
    }

    /// The scopes the agent may use on its own, in `Scope::ALL` order: every one it holds
    /// but the write scopes it only proposes. The scopes of the `Ctx` of anything it reads
    /// or writes directly, so a write it may only propose can't commit there, whatever
    /// else the operation names.
    pub fn applied_scopes(&self) -> Vec<Scope> {
        self.held(|mode| mode == WriteMode::Apply)
    }

    fn held(&self, writes: impl Fn(WriteMode) -> bool) -> Vec<Scope> {
        sorted_once(
            self.read
                .iter()
                .chain(
                    self.write
                        .iter()
                        .filter(|(_, m)| writes(*m))
                        .map(|(s, _)| s),
                )
                .chain(self.customer_data.iter())
                .copied()
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(read: &[&str], write: &[(&str, &str)], customer_data: &[&str]) -> GrantInput {
        GrantInput {
            read: read.iter().map(|s| s.to_string()).collect(),
            write: write
                .iter()
                .map(|(s, m)| (s.to_string(), m.to_string()))
                .collect(),
            customer_data: customer_data.iter().map(|s| s.to_string()).collect(),
            operations: None,
        }
    }

    fn grant(read: &[&str], write: &[(&str, &str)], customer_data: &[&str]) -> Grant {
        Grant::new(input(read, write, customer_data)).unwrap()
    }

    fn op(id: &str) -> OperationId {
        OperationId::new(id).unwrap()
    }

    #[test]
    fn write_modes_parse_their_own_strings_only() {
        for mode in [WriteMode::Propose, WriteMode::Apply] {
            assert_eq!(WriteMode::parse(mode.as_str()), Ok(mode));
        }
        assert_eq!(WriteMode::parse("Apply"), Err("Apply".to_string()));
    }

    #[test]
    fn caps_parse_their_own_strings_only() {
        for cap in [WritesCap::Apply, WritesCap::Propose, WritesCap::Off] {
            assert_eq!(WritesCap::parse(cap.as_str()), Ok(cap));
        }
        assert_eq!(WritesCap::parse("none"), Err("none".to_string()));
        assert_eq!(WritesCap::default(), WritesCap::Apply);
    }

    #[test]
    fn a_grant_keeps_each_list_in_scope_order_once() {
        let g = grant(
            &["orders:read", "products:read", "orders:read"],
            &[("inventory:write", "apply"), ("products:write", "propose")],
            &[
                "customer_data:email",
                "customer_data:name",
                "customer_data:email",
            ],
        );
        assert_eq!(g.read(), [Scope::ProductsRead, Scope::OrdersRead]);
        assert_eq!(
            g.write(),
            [
                (Scope::ProductsWrite, WriteMode::Propose),
                (Scope::InventoryWrite, WriteMode::Apply)
            ]
        );
        assert_eq!(
            g.customer_data(),
            [Scope::CustomerDataName, Scope::CustomerDataEmail]
        );
        assert_eq!(g.writes_in(WriteMode::Apply), [Scope::InventoryWrite]);
        assert_eq!(
            g.scopes(),
            [
                Scope::ProductsRead,
                Scope::ProductsWrite,
                Scope::InventoryWrite,
                Scope::OrdersRead,
                Scope::CustomerDataName,
                Scope::CustomerDataEmail
            ]
        );
    }

    #[test]
    fn unknown_names_are_refused_by_name() {
        assert_eq!(
            Grant::new(input(&["products:reed"], &[], &[])),
            Err(GrantError::UnknownScope(
                GrantField::Read,
                "products:reed".to_string()
            ))
        );
        let mode = Grant::new(input(&[], &[("products:write", "always")], &[]));
        assert_eq!(
            mode,
            Err(GrantError::UnknownWriteMode {
                scope: "products:write".to_string(),
                mode: "always".to_string()
            })
        );
        let mode = mode.unwrap_err();
        assert_eq!(
            (mode.field(), mode.scope().as_deref()),
            (GrantField::Write, Some("products:write"))
        );
    }

    #[test]
    fn never_granted_scopes_are_refused_in_every_list_and_mode() {
        for scope in NEVER_GRANTED {
            let name = scope.as_str();
            let refused = |field| Err(GrantError::NotGrantable(field, scope));
            assert_eq!(
                Grant::new(input(&[name], &[], &[])),
                refused(GrantField::Read),
                "{name}"
            );
            for mode in ["propose", "apply"] {
                assert_eq!(
                    Grant::new(input(&[], &[(name, mode)], &[])),
                    refused(GrantField::Write),
                    "{name} {mode}"
                );
            }
            assert_eq!(
                Grant::new(input(&[], &[], &[name])),
                refused(GrantField::CustomerData),
                "{name}"
            );
        }
    }

    #[test]
    fn the_read_side_of_never_granted_scopes_is_grantable() {
        let g = grant(
            &["users:read", "api_keys:read", "apps:read", "agents:read"],
            &[],
            &[],
        );
        assert_eq!(g.read().len(), 4);
    }

    #[test]
    fn erasing_customers_is_propose_only() {
        assert_eq!(
            Grant::new(input(&[], &[("customers:erase", "apply")], &[])),
            Err(GrantError::ProposeOnly(Scope::CustomersErase))
        );
        let g = grant(&[], &[("customers:erase", "propose")], &[]);
        assert_eq!(g.write(), [(Scope::CustomersErase, WriteMode::Propose)]);
    }

    #[test]
    fn each_scope_goes_in_its_own_list() {
        assert_eq!(
            Grant::new(input(&["products:write"], &[], &[])),
            Err(GrantError::NotARead(Scope::ProductsWrite))
        );
        assert_eq!(
            Grant::new(input(&["customer_data:email"], &[], &[])),
            Err(GrantError::NotARead(Scope::CustomerDataEmail))
        );
        assert_eq!(
            Grant::new(input(&[], &[("products:read", "apply")], &[])),
            Err(GrantError::NotAWrite(Scope::ProductsRead))
        );
        assert_eq!(
            Grant::new(input(&[], &[("customer_data:name", "propose")], &[])),
            Err(GrantError::NotAWrite(Scope::CustomerDataName))
        );
        assert_eq!(
            Grant::new(input(&[], &[], &["customers:read"])),
            Err(GrantError::NotCustomerData(Scope::CustomersRead))
        );
    }

    #[test]
    fn a_write_scope_has_one_mode() {
        assert_eq!(
            Grant::new(input(
                &[],
                &[("orders:write", "propose"), ("orders:write", "apply")],
                &[]
            )),
            Err(GrantError::DuplicateWrite(Scope::OrdersWrite))
        );
        assert_eq!(
            Grant::new(input(
                &[],
                &[("orders:write", "apply"), ("orders:write", "apply")],
                &[]
            )),
            Err(GrantError::DuplicateWrite(Scope::OrdersWrite))
        );
    }

    #[test]
    fn the_allowlist_is_checked_sorted_and_bounded() {
        let mut i = input(&["products:read"], &[], &[]);
        i.operations = Some(vec![
            "admin.products.list".to_string(),
            "admin.products.fetch".to_string(),
            "admin.products.list".to_string(),
        ]);
        let g = Grant::new(i.clone()).unwrap();
        assert_eq!(
            g.operations(),
            Some(&[op("admin.products.fetch"), op("admin.products.list")][..])
        );
        i.operations = Some(vec!["products".to_string()]);
        assert_eq!(
            Grant::new(i.clone()),
            Err(GrantError::Operation(OperationIdError(
                "products".to_string()
            )))
        );
        let many = |n: usize| (0..n).map(|k| op(&format!("admin.ops.op{k}"))).collect();
        assert!(
            Grant::default()
                .with_operations(Some(many(MAX_OPERATIONS)))
                .is_ok()
        );
        assert_eq!(
            Grant::default().with_operations(Some(many(MAX_OPERATIONS + 1))),
            Err(GrantError::TooManyOperations {
                max: MAX_OPERATIONS
            })
        );
        assert_eq!(Grant::default().operations(), None);
    }

    fn example() -> Grant {
        grant(
            &["orders:read"],
            &[
                ("products:write", "propose"),
                ("inventory:write", "apply"),
                ("customers:erase", "propose"),
            ],
            &["customer_data:email"],
        )
    }

    #[test]
    fn under_apply_the_grant_decides_and_writes_imply_reads() {
        let p = effective(&example(), WritesCap::Apply, None);
        assert_eq!(
            p.read(),
            [Scope::ProductsRead, Scope::InventoryRead, Scope::OrdersRead]
        );
        assert_eq!(p.access(Scope::ProductsWrite), Some(Access::Propose));
        assert_eq!(p.access(Scope::InventoryWrite), Some(Access::Apply));
        assert_eq!(p.access(Scope::InventoryRead), Some(Access::Read));
        assert_eq!(p.access(Scope::CustomerDataEmail), Some(Access::Read));
        assert_eq!(p.access(Scope::CustomerDataName), None);
        assert_eq!(p.access(Scope::OrdersWrite), None);
    }

    #[test]
    fn under_propose_nothing_applies() {
        let p = effective(&example(), WritesCap::Propose, None);
        assert_eq!(p.access(Scope::InventoryWrite), Some(Access::Propose));
        assert!(p.write().iter().all(|(_, m)| *m == WriteMode::Propose));
    }

    #[test]
    fn under_off_agents_only_read() {
        let p = effective(&example(), WritesCap::Off, None);
        assert_eq!(p.write(), []);
        assert_eq!(p.access(Scope::InventoryWrite), None);
        assert_eq!(
            p.access(Scope::InventoryRead),
            Some(Access::Read),
            "reads stay"
        );
        assert_eq!(p.access(Scope::CustomerDataEmail), Some(Access::Read));
        assert!(!p.holds_all(&[Scope::InventoryWrite]));
    }

    #[test]
    fn a_connected_agent_is_capped_by_its_persons_role() {
        // The role writes inventory, reads products (not writing them) and holds no
        // personal data.
        let role = [
            Scope::InventoryWrite,
            Scope::ProductsRead,
            Scope::OrdersRead,
        ];
        let p = effective(&example(), WritesCap::Apply, Some(&role));
        assert_eq!(p.write(), [(Scope::InventoryWrite, WriteMode::Apply)]);
        assert_eq!(
            p.read(),
            [Scope::ProductsRead, Scope::InventoryRead, Scope::OrdersRead]
        );
        assert_eq!(p.customer_data(), []);
        assert_eq!(p.access(Scope::CustomersErase), None);
        let none = effective(&example(), WritesCap::Apply, Some(&[]));
        assert_eq!(none.scopes(), Vec::<Scope>::new());
    }

    #[test]
    fn mode_for_a_write_is_its_weakest_scope_held() {
        let p = effective(&example(), WritesCap::Apply, None);
        assert_eq!(p.mode_for(&[Scope::InventoryWrite]), WriteMode::Apply);
        assert_eq!(
            p.mode_for(&[Scope::InventoryWrite, Scope::ProductsWrite]),
            WriteMode::Propose
        );
        assert_eq!(
            p.mode_for(&[Scope::InventoryWrite, Scope::OrdersRead]),
            WriteMode::Apply
        );
        assert_eq!(p.mode_for(&[]), WriteMode::Apply);
    }

    #[test]
    fn a_missing_scope_never_turns_a_proposal_into_a_write() {
        let p = effective(&example(), WritesCap::Apply, None);
        // products:write is only proposed; customers:read isn't held at all.
        let scopes = [Scope::ProductsWrite, Scope::CustomersRead];
        assert_eq!(p.mode_for(&scopes), WriteMode::Propose);
        assert!(!p.holds_all(&scopes));
        let capped = effective(&example(), WritesCap::Propose, None);
        let scopes = [Scope::InventoryWrite, Scope::CustomersRead];
        assert_eq!(capped.mode_for(&scopes), WriteMode::Propose, "the cap");
    }

    #[test]
    fn holding_a_write_is_holding_it_in_either_mode() {
        let p = effective(&example(), WritesCap::Apply, None);
        assert!(p.holds_all(&[Scope::InventoryWrite, Scope::ProductsWrite]));
        assert!(p.holds_all(&[Scope::InventoryWrite, Scope::OrdersRead]));
        assert!(!p.holds_all(&[Scope::InventoryWrite, Scope::OrdersWrite]));
        assert!(!p.holds_all(&[Scope::CustomersRead]));
        assert!(p.holds_all(&[]));
    }

    #[test]
    fn applied_scopes_leave_out_the_writes_only_proposed() {
        let p = effective(&example(), WritesCap::Apply, None);
        let applied = p.applied_scopes();
        assert!(applied.contains(&Scope::InventoryWrite));
        assert!(
            applied.contains(&Scope::ProductsRead),
            "a proposed write's read"
        );
        assert!(!applied.contains(&Scope::ProductsWrite));
        assert!(!applied.contains(&Scope::CustomersErase));
        let capped = effective(&example(), WritesCap::Propose, None);
        assert!(!capped.applied_scopes().contains(&Scope::InventoryWrite));
    }

    #[test]
    fn scopes_are_everything_held_in_any_mode() {
        let p = effective(&example(), WritesCap::Apply, None);
        assert_eq!(
            p.scopes(),
            [
                Scope::ProductsRead,
                Scope::ProductsWrite,
                Scope::InventoryRead,
                Scope::InventoryWrite,
                Scope::CustomersErase,
                Scope::OrdersRead,
                Scope::CustomerDataEmail
            ]
        );
    }

    #[test]
    fn the_allowlist_limits_operations() {
        let open = effective(&Grant::default(), WritesCap::Apply, None);
        assert!(open.allows_operation(&op("admin.orders.list")));
        let g = Grant::default()
            .with_operations(Some(vec![op("admin.orders.list")]))
            .unwrap();
        let p = effective(&g, WritesCap::Apply, None);
        assert!(p.allows_operation(&op("admin.orders.list")));
        assert!(!p.allows_operation(&op("admin.orders.fetch")));
    }

    #[test]
    fn every_scope_is_never_granted_read_write_or_customer_data() {
        // A new scope lands in exactly one of a grant's lists (or nowhere).
        for scope in Scope::ALL {
            let lists = [
                Grant::from_parts(vec![scope], vec![], vec![], None).is_ok(),
                Grant::from_parts(vec![], vec![(scope, WriteMode::Propose)], vec![], None).is_ok(),
                Grant::from_parts(vec![], vec![], vec![scope], None).is_ok(),
            ];
            let fits = lists.iter().filter(|ok| **ok).count();
            assert_eq!(
                fits,
                usize::from(!NEVER_GRANTED.contains(&scope)),
                "{scope}"
            );
        }
    }

    #[test]
    fn a_suggested_grant_reads_and_proposes_what_the_person_holds() {
        let held = [
            Scope::OrdersRead,
            Scope::OrdersWrite,
            Scope::ProductsRead,
            Scope::UsersWrite,
            Scope::CustomerDataEmail,
            Scope::CustomersErase,
        ];
        let holds = |s: Scope| held.contains(&s);
        let grant = suggested_grant(holds, &[]);
        assert_eq!(grant.read(), [Scope::ProductsRead, Scope::OrdersRead]);
        assert_eq!(
            grant.write(),
            [
                (Scope::CustomersErase, WriteMode::Propose),
                (Scope::OrdersWrite, WriteMode::Propose)
            ]
        );
        assert!(grant.customer_data().is_empty(), "no personal data");
        assert!(
            !grant.scopes().contains(&Scope::UsersWrite),
            "never granted"
        );

        let asked = suggested_grant(holds, &[Scope::OrdersWrite, Scope::InventoryRead]);
        assert_eq!(asked.read(), [Scope::OrdersRead]);
        assert_eq!(asked.write(), [(Scope::OrdersWrite, WriteMode::Propose)]);
    }
}
