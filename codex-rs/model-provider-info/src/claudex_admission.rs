//! Private preparatory host contract. No production proof constructors or reservation adapter.
//! R1 candidates, credentials present and successful HTTP responses cannot mint these proofs.
//! A future trusted verifier must attest the complete serialized request and enforced readers.
//! Nothing here dispatches, persists, verifies an account remotely or changes parent Responses.

use crate::claudex_routing::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

const MAX_FUNCTIONS: usize = 32;
const MAX_REQUEST_BYTES: usize = 64 * 1024;

// Handles are issued by the host, never account names, keys, prompt values or serialized config.
// Deliberately no Debug, Clone, serde, caller-selected URL/model or AuthManager fields.
struct DedicatedProviderIdentity {
    account_handle: u128,
    credential_generation: u64,
}

impl DedicatedProviderIdentity {
    fn endpoint(&self) -> &'static str {
        "https://api.z.ai/api/coding/paas/v4/chat/completions"
    }

    fn model(&self) -> &'static str {
        "glm-5.3"
    }

    fn reasoning_effort(&self) -> &'static str {
        "low"
    }
}

enum ToolKind {
    Function,
    Image,
    ComputerUse,
    Namespace,
    Unsupported,
}

struct PreparedTool {
    name: String,
    kind: ToolKind,
    authority: ToolAuthority,
}

enum PreparedTurn {
    FreshInitial,
    Continuation,
    Restart,
}

// The actual immutable request bytes, including all instructions and schemas, are retained.
// Host verifiers must derive the catalogue and token bound from precisely these bytes. This
// module does not parse or measure them and has no production constructor for this snapshot.
struct PreparedChildSnapshot {
    owner_handle: u128,
    policy_revision: u64,
    provider: DedicatedProviderIdentity,
    turn: PreparedTurn,
    body: Box<[u8]>,
    tools: Box<[PreparedTool]>,
}

struct ContextProof {
    snapshot: Arc<PreparedChildSnapshot>,
    valid_until: Instant,
    provenance: ContextProvenance,
    modality: InputModality,
    sensitivity: ContextSensitivity,
    isolation: IsolationEvidence,
    budget: BoundedContextBudget,
    contains_parent_reasoning: bool,
}

struct CatalogueProof {
    snapshot: Arc<PreparedChildSnapshot>,
    valid_until: Instant,
    complete_serialized_catalogue: bool,
    bounded_readers_enforced: bool,
}

struct QuotaProof {
    snapshot: Arc<PreparedChildSnapshot>,
    valid_until: Instant,
    enrollment: EnrollmentEvidence,
    billing: BillingBoundary,
    credential_kind: CredentialKind,
    operational: OperationalGate,
    quota: QuotaAdmission,
    circuit: CircuitState,
}

struct TaskProof {
    snapshot: Arc<PreparedChildSnapshot>,
    valid_until: Instant,
    category: TaskCategory,
    risk: TaskRisk,
    model_directive: ModelDirective,
}

// No public enum or model/tool input can create this opaque set. Constructors will belong to
// trusted host verifiers, with the minimum expiry of every constituent observation.
struct HostEvidence {
    snapshot: Arc<PreparedChildSnapshot>,
    context: ContextProof,
    catalogue: CatalogueProof,
    quota: QuotaProof,
    task: TaskProof,
}

/// Private, currently implemented only by tests. A future adapter must transfer an already
/// reserved native guard, scoped to this exact snapshot, not merely report `Granted`. Its Drop
/// must release that reservation. The lifecycle owner must retain it until the child has stopped.
trait ReservedSlot: Send {
    fn snapshot(&self) -> &Arc<PreparedChildSnapshot>;
}

struct OwnedPermit {
    reservation: Box<dyn ReservedSlot>,
}

#[derive(Debug, PartialEq, Eq)]
enum AdmissionError {
    SnapshotMismatch,
    PermitMismatch,
    ContextExpired,
    CatalogueExpired,
    QuotaExpired,
    TaskExpired,
    InvalidIdentity,
    InvalidBody,
    HistoryUnsupported,
    ParentReasoning,
    CatalogueUnverified,
    UnsupportedCatalogue,
    RoutingRejected(RoutingDisposition, RoutingReason),
    CandidateMismatch,
    AdmissionExpired,
}

// Non-Clone and non-serializable; the permit cannot be extracted or released independently.
// No spawn/reload/credential methods exist. Dropping this value is safe before dispatch; after
// dispatch, a future supervisor must own it alongside the child's cancellation/join lifecycle.
struct HostAdmittedChild {
    snapshot: Arc<PreparedChildSnapshot>,
    valid_until: Instant,
    _permit: OwnedPermit,
}

impl HostAdmittedChild {
    fn revalidate(&self, now: Instant) -> Result<(), AdmissionError> {
        if now >= self.valid_until {
            return Err(AdmissionError::AdmissionExpired);
        }
        Ok(())
    }
}

fn admit(
    mode: RoutingMode,
    candidate: RoutingDecision,
    evidence: HostEvidence,
    permit: OwnedPermit,
    now: Instant,
) -> Result<HostAdmittedChild, AdmissionError> {
    let snapshot = &evidence.snapshot;
    for bound in [
        &evidence.context.snapshot,
        &evidence.catalogue.snapshot,
        &evidence.quota.snapshot,
        &evidence.task.snapshot,
    ] {
        if !Arc::ptr_eq(snapshot, bound) {
            return Err(AdmissionError::SnapshotMismatch);
        }
    }
    if !Arc::ptr_eq(snapshot, permit.reservation.snapshot()) {
        return Err(AdmissionError::PermitMismatch);
    }
    for (expiry, error) in [
        (evidence.context.valid_until, AdmissionError::ContextExpired),
        (
            evidence.catalogue.valid_until,
            AdmissionError::CatalogueExpired,
        ),
        (evidence.quota.valid_until, AdmissionError::QuotaExpired),
        (evidence.task.valid_until, AdmissionError::TaskExpired),
    ] {
        if now >= expiry {
            return Err(error);
        }
    }
    if snapshot.owner_handle == 0
        || snapshot.policy_revision == 0
        || snapshot.provider.account_handle == 0
        || snapshot.provider.credential_generation == 0
    {
        return Err(AdmissionError::InvalidIdentity);
    }
    if snapshot.body.is_empty() || snapshot.body.len() > MAX_REQUEST_BYTES {
        return Err(AdmissionError::InvalidBody);
    }
    if !matches!(snapshot.turn, PreparedTurn::FreshInitial) {
        return Err(AdmissionError::HistoryUnsupported);
    }
    if evidence.context.contains_parent_reasoning {
        return Err(AdmissionError::ParentReasoning);
    }
    if !evidence.catalogue.complete_serialized_catalogue
        || !evidence.catalogue.bounded_readers_enforced
    {
        return Err(AdmissionError::CatalogueUnverified);
    }
    let mut names = HashSet::new();
    if snapshot.tools.len() > MAX_FUNCTIONS
        || snapshot.tools.iter().any(|tool| {
            !matches!(tool.kind, ToolKind::Function)
                || tool.authority != ToolAuthority::UpperBoundReadOnly
                || tool.name.is_empty()
                || tool.name.len() > 64
                || !tool
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                || !names.insert(tool.name.as_str())
        })
    {
        return Err(AdmissionError::UnsupportedCatalogue);
    }
    let route = select_route(&RoutingInput {
        mode,
        category: evidence.task.category,
        risk: evidence.task.risk,
        context: evidence.context.provenance,
        modality: evidence.context.modality,
        sensitivity: evidence.context.sensitivity,
        isolation: evidence.context.isolation,
        tool_authority: ToolAuthority::UpperBoundReadOnly,
        enrollment: evidence.quota.enrollment,
        billing: evidence.quota.billing,
        credential_kind: evidence.quota.credential_kind,
        quota_freshness: QuotaEvidenceFreshness::Fresh,
        model_directive: evidence.task.model_directive,
        operational: evidence.quota.operational,
        quota: evidence.quota.quota,
        circuit: evidence.quota.circuit,
        concurrency: ConcurrencyAdmission::Granted,
        budget: Some(evidence.context.budget),
        endpoint: Some(
            validate_subscription_endpoint(ZAI_CODING_ENDPOINT, RedirectPolicy::Deny)
                .map_err(|_| AdmissionError::InvalidIdentity)?,
        ),
    });
    if route.disposition != RoutingDisposition::CandidateZai {
        return Err(AdmissionError::RoutingRejected(
            route.disposition,
            route.reason,
        ));
    }
    if route != candidate {
        return Err(AdmissionError::CandidateMismatch);
    }
    let valid_until = [
        evidence.context.valid_until,
        evidence.catalogue.valid_until,
        evidence.quota.valid_until,
        evidence.task.valid_until,
    ]
    .into_iter()
    .min()
    .ok_or(AdmissionError::AdmissionExpired)?;
    Ok(HostAdmittedChild {
        snapshot: evidence.snapshot,
        valid_until,
        _permit: permit,
    })
}

#[cfg(test)]
#[path = "claudex_admission_tests.rs"]
mod tests;
