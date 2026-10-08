use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

// This genuinely owns a LOCAL fixture lease, not a native AgentControl reservation.
struct LocalSlot {
    snapshot: Arc<PreparedChildSnapshot>,
    live: Arc<AtomicUsize>,
}

impl ReservedSlot for LocalSlot {
    fn snapshot(&self) -> &Arc<PreparedChildSnapshot> {
        &self.snapshot
    }
}

impl Drop for LocalSlot {
    fn drop(&mut self) {
        assert_eq!(self.live.fetch_sub(1, Ordering::SeqCst), 1);
    }
}

struct Fixture {
    now: Instant,
    evidence: HostEvidence,
    permit: OwnedPermit,
    live: Arc<AtomicUsize>,
}

fn fixture(change: impl FnOnce(&mut PreparedChildSnapshot)) -> Fixture {
    let now = Instant::now();
    let expiry = now + Duration::from_secs(10);
    let mut snapshot = PreparedChildSnapshot {
        owner_handle: 1,
        policy_revision: 1,
        provider: DedicatedProviderIdentity {
            account_handle: 1,
            credential_generation: 1,
        },
        turn: PreparedTurn::FreshInitial,
        body: b"synthetic payload, not a validated DTO"
            .to_vec()
            .into_boxed_slice(),
        tools: vec![PreparedTool {
            name: "bounded_reader".into(),
            kind: ToolKind::Function,
            authority: ToolAuthority::UpperBoundReadOnly,
        }]
        .into_boxed_slice(),
    };
    change(&mut snapshot);
    let snapshot = Arc::new(snapshot);
    let evidence = HostEvidence {
        snapshot: Arc::clone(&snapshot),
        context: ContextProof {
            snapshot: Arc::clone(&snapshot),
            valid_until: expiry,
            provenance: ContextProvenance::ChildFresh,
            modality: InputModality::Text,
            sensitivity: ContextSensitivity::NonSensitive,
            isolation: IsolationEvidence::IsolationVerified,
            budget: BoundedContextBudget::from_measured_tokens(1024).expect("fixture bound"),
            contains_parent_reasoning: false,
        },
        catalogue: CatalogueProof {
            snapshot: Arc::clone(&snapshot),
            valid_until: expiry,
            complete_serialized_catalogue: true,
            bounded_readers_enforced: true,
        },
        quota: QuotaProof {
            snapshot: Arc::clone(&snapshot),
            valid_until: expiry,
            enrollment: EnrollmentEvidence::VerifiedCodingPlan,
            billing: BillingBoundary::CodingSubscriptionOnly,
            credential_kind: CredentialKind::VerifiedCodingPlan,
            operational: OperationalGate::Ready,
            quota: QuotaAdmission::Available,
            circuit: CircuitState::Closed,
        },
        task: TaskProof {
            snapshot: Arc::clone(&snapshot),
            valid_until: expiry,
            category: TaskCategory::RepoSearch,
            risk: TaskRisk::Low,
            model_directive: ModelDirective::InheritParent,
        },
    };
    let live = Arc::new(AtomicUsize::new(1));
    let permit = OwnedPermit {
        reservation: Box::new(LocalSlot {
            snapshot,
            live: Arc::clone(&live),
        }),
    };
    Fixture {
        now,
        evidence,
        permit,
        live,
    }
}

fn candidate(mode: RoutingMode) -> RoutingDecision {
    RoutingDecision {
        target: RoutingTarget::ZaiGlm53Low,
        disposition: RoutingDisposition::CandidateZai,
        reason: if mode == RoutingMode::ExplicitZai {
            RoutingReason::ExplicitLowRiskFreshReadOnly
        } else {
            RoutingReason::AutoLowRiskFreshReadOnly
        },
    }
}

fn reject(f: Fixture, mode: RoutingMode, expected: AdmissionError) {
    let result = admit(mode, candidate(mode), f.evidence, f.permit, f.now);
    assert_eq!(result.err(), Some(expected));
    assert_eq!(f.live.load(Ordering::SeqCst), 0);
}

#[test]
fn admission_owns_the_lease_and_expiry_does_not_release_the_owned_slot() {
    let mut f = fixture(|_| {});
    f.evidence.quota.valid_until = f.now + Duration::from_secs(2);
    let admitted = admit(
        RoutingMode::Auto,
        candidate(RoutingMode::Auto),
        f.evidence,
        f.permit,
        f.now,
    )
    .expect("synthetic proofs only");
    assert_eq!(f.live.load(Ordering::SeqCst), 1);
    assert_eq!(admitted.revalidate(f.now), Ok(()));
    assert_eq!(
        admitted.revalidate(f.now + Duration::from_secs(2)),
        Err(AdmissionError::AdmissionExpired)
    );
    assert_eq!(f.live.load(Ordering::SeqCst), 1);
    drop(admitted);
    assert_eq!(f.live.load(Ordering::SeqCst), 0);
}

#[test]
fn an_r1_candidate_cannot_bypass_host_context_or_change_explicit_refusal() {
    for mode in [RoutingMode::Auto, RoutingMode::ExplicitZai] {
        let mut f = fixture(|_| {});
        f.evidence.context.provenance = ContextProvenance::FullHistory;
        let disposition = if mode == RoutingMode::ExplicitZai {
            RoutingDisposition::RejectExplicitZai
        } else {
            RoutingDisposition::KeepParent
        };
        reject(
            f,
            mode,
            AdmissionError::RoutingRejected(disposition, RoutingReason::FullHistory),
        );
    }
}

#[test]
fn modality_privacy_isolation_and_parent_reasoning_are_not_reduced() {
    for case in 0..4 {
        let mut f = fixture(|_| {});
        let expected = match case {
            0 => {
                f.evidence.context.modality = InputModality::Other;
                AdmissionError::RoutingRejected(
                    RoutingDisposition::KeepParent,
                    RoutingReason::UnsupportedModality,
                )
            }
            1 => {
                f.evidence.context.sensitivity = ContextSensitivity::SecretPotential;
                AdmissionError::RoutingRejected(
                    RoutingDisposition::KeepParent,
                    RoutingReason::PotentialSecret,
                )
            }
            2 => {
                f.evidence.context.isolation = IsolationEvidence::Unverified;
                AdmissionError::RoutingRejected(
                    RoutingDisposition::KeepParent,
                    RoutingReason::IsolationUnverified,
                )
            }
            3 => {
                f.evidence.context.contains_parent_reasoning = true;
                AdmissionError::ParentReasoning
            }
            _ => unreachable!("fixture matrix"),
        };
        reject(f, RoutingMode::Auto, expected);
    }
}

#[test]
fn every_proof_expires_at_its_boundary() {
    for case in 0..4 {
        let mut f = fixture(|_| {});
        let expected = match case {
            0 => {
                f.evidence.context.valid_until = f.now;
                AdmissionError::ContextExpired
            }
            1 => {
                f.evidence.catalogue.valid_until = f.now;
                AdmissionError::CatalogueExpired
            }
            2 => {
                f.evidence.quota.valid_until = f.now;
                AdmissionError::QuotaExpired
            }
            3 => {
                f.evidence.task.valid_until = f.now;
                AdmissionError::TaskExpired
            }
            _ => unreachable!("fixture matrix"),
        };
        reject(f, RoutingMode::Auto, expected);
    }
}

#[test]
fn unsupported_or_unbounded_catalogue_refuses_the_whole_request() {
    for kind in [
        ToolKind::Image,
        ToolKind::ComputerUse,
        ToolKind::Namespace,
        ToolKind::Unsupported,
    ] {
        reject(
            fixture(|s| s.tools[0].kind = kind),
            RoutingMode::Auto,
            AdmissionError::UnsupportedCatalogue,
        );
    }
    reject(
        fixture(|s| s.tools[0].authority = ToolAuthority::Unrestricted),
        RoutingMode::Auto,
        AdmissionError::UnsupportedCatalogue,
    );
    reject(
        fixture(|s| {
            s.tools = (0..33)
                .map(|n| PreparedTool {
                    name: format!("reader_{n}"),
                    kind: ToolKind::Function,
                    authority: ToolAuthority::UpperBoundReadOnly,
                })
                .collect()
        }),
        RoutingMode::Auto,
        AdmissionError::UnsupportedCatalogue,
    );
    for complete in [false, true] {
        let mut f = fixture(|_| {});
        f.evidence.catalogue.complete_serialized_catalogue = complete;
        f.evidence.catalogue.bounded_readers_enforced = !complete;
        reject(f, RoutingMode::Auto, AdmissionError::CatalogueUnverified);
    }
}

#[test]
fn invalid_and_duplicate_function_names_are_refused_without_filtering() {
    for name in [
        String::new(),
        "has space".into(),
        "namespaced.reader".into(),
        "x".repeat(65),
    ] {
        reject(
            fixture(|s| s.tools[0].name = name),
            RoutingMode::Auto,
            AdmissionError::UnsupportedCatalogue,
        );
    }
    reject(
        fixture(|s| {
            s.tools = (0..2)
                .map(|_| PreparedTool {
                    name: "same_reader".into(),
                    kind: ToolKind::Function,
                    authority: ToolAuthority::UpperBoundReadOnly,
                })
                .collect()
        }),
        RoutingMode::Auto,
        AdmissionError::UnsupportedCatalogue,
    );
}

#[test]
fn identical_but_separately_prepared_payload_or_account_proofs_do_not_bind() {
    let mut f = fixture(|_| {});
    let other = fixture(|_| {});
    f.evidence.quota.snapshot = Arc::clone(&other.evidence.snapshot);
    reject(f, RoutingMode::Auto, AdmissionError::SnapshotMismatch);
    drop(other);
    let f = fixture(|_| {});
    let other = fixture(|s| s.provider.account_handle = 2);
    let result = admit(
        RoutingMode::Auto,
        candidate(RoutingMode::Auto),
        f.evidence,
        other.permit,
        f.now,
    );
    assert_eq!(result.err(), Some(AdmissionError::PermitMismatch));
    assert_eq!(other.live.load(Ordering::SeqCst), 0);
    assert_eq!(f.live.load(Ordering::SeqCst), 1);
    drop(f.permit);
    assert_eq!(f.live.load(Ordering::SeqCst), 0);
}

#[test]
fn quota_unknown_or_exhausted_never_becomes_an_estimated_balance() {
    for quota in [QuotaAdmission::Unknown, QuotaAdmission::Exhausted] {
        let mut f = fixture(|_| {});
        f.evidence.quota.quota = quota;
        let reason = if quota == QuotaAdmission::Unknown {
            RoutingReason::QuotaUnknown
        } else {
            RoutingReason::QuotaExhausted
        };
        reject(
            f,
            RoutingMode::Auto,
            AdmissionError::RoutingRejected(RoutingDisposition::KeepParent, reason),
        );
    }
}

#[test]
fn changed_candidate_and_oversized_input_are_refused_without_dispatch() {
    let f = fixture(|_| {});
    let result = admit(
        RoutingMode::Auto,
        candidate(RoutingMode::ExplicitZai),
        f.evidence,
        f.permit,
        f.now,
    );
    assert_eq!(result.err(), Some(AdmissionError::CandidateMismatch));
    assert_eq!(f.live.load(Ordering::SeqCst), 0);
    reject(
        fixture(|s| s.body = vec![b'x'; MAX_REQUEST_BYTES + 1].into_boxed_slice()),
        RoutingMode::Auto,
        AdmissionError::InvalidBody,
    );
    for turn in [PreparedTurn::Continuation, PreparedTurn::Restart] {
        reject(
            fixture(|s| s.turn = turn),
            RoutingMode::Auto,
            AdmissionError::HistoryUnsupported,
        );
    }
}
