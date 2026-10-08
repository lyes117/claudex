use super::*;
use pretty_assertions::assert_eq;

fn ready_input(mode: RoutingMode) -> RoutingInput {
    RoutingInput {
        mode,
        category: TaskCategory::Documentation,
        risk: TaskRisk::Low,
        context: ContextProvenance::ChildFresh,
        modality: InputModality::Text,
        sensitivity: ContextSensitivity::NonSensitive,
        tool_authority: ToolAuthority::UpperBoundReadOnly,
        isolation: IsolationEvidence::IsolationVerified,
        enrollment: EnrollmentEvidence::VerifiedCodingPlan,
        billing: BillingBoundary::CodingSubscriptionOnly,
        credential_kind: CredentialKind::VerifiedCodingPlan,
        quota_freshness: QuotaEvidenceFreshness::Fresh,
        model_directive: ModelDirective::InheritParent,
        operational: OperationalGate::Ready,
        quota: QuotaAdmission::Available,
        circuit: CircuitState::Closed,
        concurrency: ConcurrencyAdmission::Granted,
        budget: Some(BoundedContextBudget::from_measured_tokens(1024).expect("measured context")),
        endpoint: Some(
            validate_subscription_endpoint(ZAI_CODING_ENDPOINT, RedirectPolicy::Deny)
                .expect("endpoint"),
        ),
    }
}

#[test]
fn eligibility_matrix_retains_parent_for_any_non_low_risk_or_ineligible_task() {
    for mode in [
        RoutingMode::Disabled,
        RoutingMode::Auto,
        RoutingMode::ExplicitZai,
    ] {
        for category in [
            TaskCategory::Documentation,
            TaskCategory::RepoSearch,
            TaskCategory::BoundedTransform,
            TaskCategory::TestAuthoring,
            TaskCategory::Architecture,
            TaskCategory::Security,
            TaskCategory::Unknown,
        ] {
            for risk in [TaskRisk::Low, TaskRisk::High, TaskRisk::Unknown] {
                let mut input = ready_input(mode);
                input.category = category;
                input.risk = risk;
                let reason = match (mode, risk, category) {
                    (RoutingMode::Disabled, _, _) => RoutingReason::Disabled,
                    (_, TaskRisk::High, _) => RoutingReason::HighRisk,
                    (_, TaskRisk::Unknown, _) => RoutingReason::UnknownRisk,
                    (
                        _,
                        TaskRisk::Low,
                        TaskCategory::Architecture | TaskCategory::Security | TaskCategory::Unknown,
                    ) => RoutingReason::IneligibleTask,
                    (RoutingMode::Auto, TaskRisk::Low, _) => {
                        RoutingReason::AutoLowRiskFreshReadOnly
                    }
                    (RoutingMode::ExplicitZai, TaskRisk::Low, _) => {
                        RoutingReason::ExplicitLowRiskFreshReadOnly
                    }
                };
                let target = if matches!(
                    reason,
                    RoutingReason::AutoLowRiskFreshReadOnly
                        | RoutingReason::ExplicitLowRiskFreshReadOnly
                ) {
                    RoutingTarget::ZaiGlm53Low
                } else {
                    RoutingTarget::ParentCodex
                };
                let disposition = if target == RoutingTarget::ZaiGlm53Low {
                    RoutingDisposition::CandidateZai
                } else if mode == RoutingMode::ExplicitZai {
                    RoutingDisposition::RejectExplicitZai
                } else {
                    RoutingDisposition::KeepParent
                };
                assert_eq!(
                    select_route(&input),
                    RoutingDecision {
                        target,
                        reason,
                        disposition
                    }
                );
            }
        }
    }
}

#[test]
fn every_missing_operational_or_context_evidence_refuses_even_explicit_zai() {
    for mode in [RoutingMode::Auto, RoutingMode::ExplicitZai] {
        let ready = ready_input(mode);
        let mut cases = Vec::new();
        macro_rules! refuses {
            ($field:ident, $value:expr, $reason:ident) => {{
                let mut input = ready;
                input.$field = $value;
                cases.push((input, RoutingReason::$reason));
            }};
        }
        refuses!(
            model_directive,
            ModelDirective::ExplicitOther,
            ExplicitOtherModel
        );
        refuses!(context, ContextProvenance::FullHistory, FullHistory);
        refuses!(context, ContextProvenance::Unknown, UnknownContext);
        refuses!(modality, InputModality::Other, UnsupportedModality);
        refuses!(modality, InputModality::Unknown, UnknownModality);
        refuses!(
            sensitivity,
            ContextSensitivity::SecretPotential,
            PotentialSecret
        );
        refuses!(sensitivity, ContextSensitivity::Unknown, UnknownSensitivity);
        refuses!(
            tool_authority,
            ToolAuthority::Unrestricted,
            UnrestrictedTools
        );
        refuses!(tool_authority, ToolAuthority::Unknown, UnknownToolAuthority);
        refuses!(
            isolation,
            IsolationEvidence::Unverified,
            IsolationUnverified
        );
        refuses!(
            enrollment,
            EnrollmentEvidence::Unverified,
            EnrollmentUnverified
        );
        refuses!(
            billing,
            BillingBoundary::PayAsYouGoPossible,
            PayAsYouGoPossible
        );
        refuses!(billing, BillingBoundary::Unknown, UnknownBillingBoundary);
        refuses!(
            credential_kind,
            CredentialKind::GeneralApiKey,
            GeneralApiCredential
        );
        refuses!(
            credential_kind,
            CredentialKind::Unknown,
            UnknownCredentialKind
        );
        refuses!(
            quota_freshness,
            QuotaEvidenceFreshness::Stale,
            StaleQuotaEvidence
        );
        refuses!(
            quota_freshness,
            QuotaEvidenceFreshness::Unknown,
            UnknownQuotaFreshness
        );
        refuses!(
            operational,
            OperationalGate::ProviderUnavailable,
            ProviderUnavailable
        );
        refuses!(
            operational,
            OperationalGate::SubscriptionUnverified,
            SubscriptionUnverified
        );
        refuses!(
            operational,
            OperationalGate::CredentialUnavailable,
            CredentialUnavailable
        );
        refuses!(
            operational,
            OperationalGate::TransportUnverified,
            TransportUnverified
        );
        refuses!(endpoint, None, UnvalidatedEndpoint);
        refuses!(quota, QuotaAdmission::Unknown, QuotaUnknown);
        refuses!(quota, QuotaAdmission::Exhausted, QuotaExhausted);
        refuses!(circuit, CircuitState::Open, CircuitOpen);
        refuses!(circuit, CircuitState::Unknown, CircuitUnknown);
        refuses!(
            concurrency,
            ConcurrencyAdmission::Unavailable,
            ConcurrencyUnavailable
        );
        refuses!(
            concurrency,
            ConcurrencyAdmission::Unknown,
            ConcurrencyUnknown
        );
        refuses!(budget, None, ContextBudgetUnknown);
        for (input, reason) in cases {
            let disposition = if mode == RoutingMode::ExplicitZai {
                RoutingDisposition::RejectExplicitZai
            } else {
                RoutingDisposition::KeepParent
            };
            assert_eq!(
                select_route(&input),
                RoutingDecision {
                    target: RoutingTarget::ParentCodex,
                    reason,
                    disposition
                }
            );
        }
    }
}

#[test]
fn disabled_preserves_existing_model_before_other_evidence_is_examined() {
    let mut input = ready_input(RoutingMode::Disabled);
    input.model_directive = ModelDirective::ExplicitOther;
    input.operational = OperationalGate::SubscriptionUnverified;
    input.quota = QuotaAdmission::Unknown;
    input.endpoint = None;
    input.budget = None;
    assert_eq!(
        select_route(&input),
        RoutingDecision {
            target: RoutingTarget::ParentCodex,
            reason: RoutingReason::Disabled,
            disposition: RoutingDisposition::KeepParent
        }
    );
}

#[test]
fn operational_ready_cannot_replace_subscription_isolation_or_billing_attestation() {
    let mut input = ready_input(RoutingMode::ExplicitZai);
    input.enrollment = EnrollmentEvidence::Unverified;
    assert_eq!(
        select_route(&input),
        RoutingDecision {
            target: RoutingTarget::ParentCodex,
            reason: RoutingReason::EnrollmentUnverified,
            disposition: RoutingDisposition::RejectExplicitZai,
        }
    );
    input.enrollment = EnrollmentEvidence::VerifiedCodingPlan;
    input.billing = BillingBoundary::PayAsYouGoPossible;
    assert_eq!(
        select_route(&input),
        RoutingDecision {
            target: RoutingTarget::ParentCodex,
            reason: RoutingReason::PayAsYouGoPossible,
            disposition: RoutingDisposition::RejectExplicitZai,
        }
    );
    input.billing = BillingBoundary::CodingSubscriptionOnly;
    input.isolation = IsolationEvidence::Unverified;
    assert_eq!(
        select_route(&input),
        RoutingDecision {
            target: RoutingTarget::ParentCodex,
            reason: RoutingReason::IsolationUnverified,
            disposition: RoutingDisposition::RejectExplicitZai,
        }
    );
}

#[test]
fn measured_budget_accepts_exact_bound_but_cannot_construct_unknown_empty_or_overflow() {
    for tokens in [1, MAX_ROUTED_CONTEXT_TOKENS] {
        let budget = BoundedContextBudget::from_measured_tokens(tokens).expect("bounded");
        assert_eq!(budget.measured_tokens(), tokens);
        let mut input = ready_input(RoutingMode::Auto);
        input.budget = Some(budget);
        assert_eq!(select_route(&input).target, RoutingTarget::ZaiGlm53Low);
    }
    assert_eq!(
        BoundedContextBudget::from_measured_tokens(0),
        Err(ContextBudgetError::Empty)
    );
    for tokens in [MAX_ROUTED_CONTEXT_TOKENS + 1, u32::MAX] {
        assert_eq!(
            BoundedContextBudget::from_measured_tokens(tokens),
            Err(ContextBudgetError::ExceedsHardLimit)
        );
    }
}

#[test]
fn endpoint_rejects_normalization_credentials_paygo_and_redirect_escape_routes() {
    validate_subscription_endpoint(ZAI_CODING_ENDPOINT, RedirectPolicy::Deny)
        .expect("canonical endpoint");
    for policy in [RedirectPolicy::Follow, RedirectPolicy::Unknown] {
        assert_eq!(
            validate_subscription_endpoint(ZAI_CODING_ENDPOINT, policy),
            Err(EndpointError::RedirectsNotDenied)
        );
    }
    for url in [
        "http://api.z.ai/api/coding/paas/v4",
        "https://api.z.ai/api/paas/v4",
        "https://api.z.ai/api/coding/paas/v4/",
        "https://api.z.ai:443/api/coding/paas/v4",
        "https://API.Z.AI/api/coding/paas/v4",
        "https://api.z.ai./api/coding/paas/v4",
        "https://api.z.ai.evil.test/api/coding/paas/v4",
        "https://api.z.ai@evil.test/api/coding/paas/v4",
        "https://user@api.z.ai/api/coding/paas/v4",
        "https://user:fixture@api.z.ai/api/coding/paas/v4",
        "https://api.z.ai/api/coding/paas/v4?mode=subscription",
        "https://api.z.ai/api/coding/paas/v4#coding",
        "https://api.z.ai/api/coding/../coding/paas/v4",
        "https://api.z.ai/api/%63oding/paas/v4",
        "https://api.z.a\u{0456}/api/coding/paas/v4",
        "https://api.z.ai\\evil.test/api/coding/paas/v4",
        " https://api.z.ai/api/coding/paas/v4",
        "https://api.z.ai/api/coding/paas/v4\n",
    ] {
        assert_eq!(
            validate_subscription_endpoint(url, RedirectPolicy::Deny),
            Err(EndpointError::NonCanonicalSubscriptionEndpoint)
        );
    }
}
