//! Conservative routing for fresh, bounded, read-only child tasks.
//!
//! Inputs are trusted supervisor evidence, never classifications claimed by a prompt or tool.
//! This module does not verify subscriptions, measure tokens, estimate remote quota, reserve
//! concurrency, perform inference, or install credentials. The caller must supply that evidence.
//! Both automatic and explicit routing refuse uncertain or unrestricted execution contexts.
//! Evidence must come from trusted host verifiers and a preauthorized bounded payload/readers,
//! not model output, an HTTP 200, prompt-length heuristics, or a claimed privacy scan.
//! The dispatcher must enforce that tool profile and revalidate expiring quota/slot evidence.
//! Selection never reserves or releases a concurrency permit, nor proves remote model eligibility.

pub const ZAI_CODING_ENDPOINT: &str = "https://api.z.ai/api/coding/paas/v4";
pub const MAX_ROUTED_CONTEXT_TOKENS: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingMode {
    Disabled,
    Auto,
    ExplicitZai,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCategory {
    Documentation,
    RepoSearch,
    BoundedTransform,
    TestAuthoring,
    Architecture,
    Security,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskRisk {
    Low,
    High,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextProvenance {
    ChildFresh,
    FullHistory,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputModality {
    Text,
    Other,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSensitivity {
    NonSensitive,
    SecretPotential,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAuthority {
    UpperBoundReadOnly,
    Unrestricted,
    Unknown,
}
/// Attestation of the bounded child payload/readers; freshness alone does not prove isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationEvidence {
    IsolationVerified,
    Unverified,
}
/// Verification must establish this account's GLM-5.3 coding-plan entitlement, not just any plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentEvidence {
    VerifiedCodingPlan,
    Unverified,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillingBoundary {
    CodingSubscriptionOnly,
    PayAsYouGoPossible,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    VerifiedCodingPlan,
    GeneralApiKey,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaEvidenceFreshness {
    Fresh,
    Stale,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelDirective {
    InheritParent,
    ExplicitOther,
}
/// `Ready` requires external verification of enrollment, credentials and supported transport.
/// It does not assert a precise remote quota or authorize a pay-as-you-go endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationalGate {
    ProviderUnavailable,
    SubscriptionUnverified,
    CredentialUnavailable,
    TransportUnverified,
    Ready,
}
/// Admission evidence from the caller; a local estimate must not be represented as remote truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaAdmission {
    Unknown,
    Available,
    Exhausted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    Unknown,
}
/// A granted slot must already have been reserved by the supervisor for this child task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConcurrencyAdmission {
    Granted,
    Unavailable,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectPolicy {
    Deny,
    Follow,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointError {
    RedirectsNotDenied,
    NonCanonicalSubscriptionEndpoint,
}

/// Evidence of an exact coding-subscription endpoint and a requested no-redirect policy.
/// The transport must enforce that policy; this value performs no network verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedSubscriptionEndpoint {
    _private: (),
}

pub fn validate_subscription_endpoint(
    endpoint: &str,
    redirects: RedirectPolicy,
) -> Result<ValidatedSubscriptionEndpoint, EndpointError> {
    if redirects != RedirectPolicy::Deny {
        return Err(EndpointError::RedirectsNotDenied);
    }
    if endpoint != ZAI_CODING_ENDPOINT {
        return Err(EndpointError::NonCanonicalSubscriptionEndpoint);
    }
    let parsed =
        url::Url::parse(endpoint).map_err(|_| EndpointError::NonCanonicalSubscriptionEndpoint)?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("api.z.ai")
        || parsed.path() != "/api/coding/paas/v4"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(EndpointError::NonCanonicalSubscriptionEndpoint);
    }
    Ok(ValidatedSubscriptionEndpoint { _private: () })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextBudgetError {
    Empty,
    ExceedsHardLimit,
}
/// A measured bound over the complete prepared child input, including instructions and tools.
/// A caller without a reliable count must use `None`, which refuses alternate-provider routing.
/// Count the actual GLM input and schemas; an OpenAI tokenizer estimate is not sufficient evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedContextBudget {
    measured_tokens: u32,
}

impl BoundedContextBudget {
    pub fn from_measured_tokens(measured_tokens: u32) -> Result<Self, ContextBudgetError> {
        if measured_tokens == 0 {
            return Err(ContextBudgetError::Empty);
        }
        if measured_tokens > MAX_ROUTED_CONTEXT_TOKENS {
            return Err(ContextBudgetError::ExceedsHardLimit);
        }
        Ok(Self { measured_tokens })
    }

    pub fn measured_tokens(self) -> u32 {
        self.measured_tokens
    }
}

/// Trusted host-only evidence resolved by the supervising runtime. No prompt text enters this selector.
/// Public enum construction is not an attestation mechanism. Do not expose these values as
/// model-controlled tool arguments; verify real isolation, entitlement and tool authority first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutingInput {
    pub mode: RoutingMode,
    pub category: TaskCategory,
    pub risk: TaskRisk,
    pub context: ContextProvenance,
    pub modality: InputModality,
    pub sensitivity: ContextSensitivity,
    pub tool_authority: ToolAuthority,
    pub isolation: IsolationEvidence,
    pub enrollment: EnrollmentEvidence,
    pub billing: BillingBoundary,
    pub credential_kind: CredentialKind,
    pub quota_freshness: QuotaEvidenceFreshness,
    pub model_directive: ModelDirective,
    pub operational: OperationalGate,
    pub quota: QuotaAdmission,
    pub circuit: CircuitState,
    pub concurrency: ConcurrencyAdmission,
    pub budget: Option<BoundedContextBudget>,
    pub endpoint: Option<ValidatedSubscriptionEndpoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingTarget {
    ParentCodex,
    ZaiGlm53Low,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingReason {
    Disabled,
    ExplicitOtherModel,
    HighRisk,
    UnknownRisk,
    IneligibleTask,
    FullHistory,
    UnknownContext,
    UnsupportedModality,
    UnknownModality,
    PotentialSecret,
    UnknownSensitivity,
    UnrestrictedTools,
    UnknownToolAuthority,
    IsolationUnverified,
    EnrollmentUnverified,
    PayAsYouGoPossible,
    UnknownBillingBoundary,
    GeneralApiCredential,
    UnknownCredentialKind,
    StaleQuotaEvidence,
    UnknownQuotaFreshness,
    ProviderUnavailable,
    SubscriptionUnverified,
    CredentialUnavailable,
    TransportUnverified,
    UnvalidatedEndpoint,
    QuotaUnknown,
    QuotaExhausted,
    CircuitOpen,
    CircuitUnknown,
    ConcurrencyUnavailable,
    ConcurrencyUnknown,
    ContextBudgetUnknown,
    AutoLowRiskFreshReadOnly,
    ExplicitLowRiskFreshReadOnly,
}

/// A candidate is admissible for further dispatch checks, not evidence of execution or privacy.
/// An explicit refusal must surface an error; it must never silently dispatch to the parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDisposition {
    KeepParent,
    CandidateZai,
    RejectExplicitZai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutingDecision {
    pub target: RoutingTarget,
    pub reason: RoutingReason,
    pub disposition: RoutingDisposition,
}

pub fn select_route(input: &RoutingInput) -> RoutingDecision {
    use RoutingReason as Reason;
    let retain = |reason| RoutingDecision {
        target: RoutingTarget::ParentCodex,
        reason,
        disposition: if input.mode == RoutingMode::ExplicitZai {
            RoutingDisposition::RejectExplicitZai
        } else {
            RoutingDisposition::KeepParent
        },
    };
    if input.mode == RoutingMode::Disabled {
        return retain(Reason::Disabled);
    }
    if input.model_directive == ModelDirective::ExplicitOther {
        return retain(Reason::ExplicitOtherModel);
    }
    match input.risk {
        TaskRisk::High => return retain(Reason::HighRisk),
        TaskRisk::Unknown => return retain(Reason::UnknownRisk),
        TaskRisk::Low => {}
    }
    match input.category {
        TaskCategory::Documentation
        | TaskCategory::RepoSearch
        | TaskCategory::BoundedTransform
        | TaskCategory::TestAuthoring => {}
        TaskCategory::Architecture | TaskCategory::Security | TaskCategory::Unknown => {
            return retain(Reason::IneligibleTask);
        }
    }
    match input.context {
        ContextProvenance::FullHistory => return retain(Reason::FullHistory),
        ContextProvenance::Unknown => return retain(Reason::UnknownContext),
        ContextProvenance::ChildFresh => {}
    }
    match input.modality {
        InputModality::Other => return retain(Reason::UnsupportedModality),
        InputModality::Unknown => return retain(Reason::UnknownModality),
        InputModality::Text => {}
    }
    match input.sensitivity {
        ContextSensitivity::SecretPotential => return retain(Reason::PotentialSecret),
        ContextSensitivity::Unknown => return retain(Reason::UnknownSensitivity),
        ContextSensitivity::NonSensitive => {}
    }
    match input.tool_authority {
        ToolAuthority::Unrestricted => return retain(Reason::UnrestrictedTools),
        ToolAuthority::Unknown => return retain(Reason::UnknownToolAuthority),
        ToolAuthority::UpperBoundReadOnly => {}
    }
    if input.isolation != IsolationEvidence::IsolationVerified {
        return retain(Reason::IsolationUnverified);
    }
    if input.enrollment != EnrollmentEvidence::VerifiedCodingPlan {
        return retain(Reason::EnrollmentUnverified);
    }
    match input.billing {
        BillingBoundary::PayAsYouGoPossible => return retain(Reason::PayAsYouGoPossible),
        BillingBoundary::Unknown => return retain(Reason::UnknownBillingBoundary),
        BillingBoundary::CodingSubscriptionOnly => {}
    }
    match input.credential_kind {
        CredentialKind::GeneralApiKey => return retain(Reason::GeneralApiCredential),
        CredentialKind::Unknown => return retain(Reason::UnknownCredentialKind),
        CredentialKind::VerifiedCodingPlan => {}
    }
    match input.quota_freshness {
        QuotaEvidenceFreshness::Stale => return retain(Reason::StaleQuotaEvidence),
        QuotaEvidenceFreshness::Unknown => return retain(Reason::UnknownQuotaFreshness),
        QuotaEvidenceFreshness::Fresh => {}
    }
    match input.operational {
        OperationalGate::ProviderUnavailable => return retain(Reason::ProviderUnavailable),
        OperationalGate::SubscriptionUnverified => return retain(Reason::SubscriptionUnverified),
        OperationalGate::CredentialUnavailable => return retain(Reason::CredentialUnavailable),
        OperationalGate::TransportUnverified => return retain(Reason::TransportUnverified),
        OperationalGate::Ready => {}
    }
    if input.endpoint.is_none() {
        return retain(Reason::UnvalidatedEndpoint);
    }
    match input.quota {
        QuotaAdmission::Unknown => return retain(Reason::QuotaUnknown),
        QuotaAdmission::Exhausted => return retain(Reason::QuotaExhausted),
        QuotaAdmission::Available => {}
    }
    match input.circuit {
        CircuitState::Open => return retain(Reason::CircuitOpen),
        CircuitState::Unknown => return retain(Reason::CircuitUnknown),
        CircuitState::Closed => {}
    }
    match input.concurrency {
        ConcurrencyAdmission::Unavailable => return retain(Reason::ConcurrencyUnavailable),
        ConcurrencyAdmission::Unknown => return retain(Reason::ConcurrencyUnknown),
        ConcurrencyAdmission::Granted => {}
    }
    if input.budget.is_none() {
        return retain(Reason::ContextBudgetUnknown);
    }
    let reason = match input.mode {
        RoutingMode::Auto => Reason::AutoLowRiskFreshReadOnly,
        RoutingMode::ExplicitZai => Reason::ExplicitLowRiskFreshReadOnly,
        RoutingMode::Disabled => return retain(Reason::Disabled),
    };
    RoutingDecision {
        target: RoutingTarget::ZaiGlm53Low,
        reason,
        disposition: RoutingDisposition::CandidateZai,
    }
}

#[cfg(test)]
#[path = "claudex_routing_tests.rs"]
mod tests;
