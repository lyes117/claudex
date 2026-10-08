//! Terminal delivery belongs either to the native agent tree or to its supervisor.
//! This attachment is captured before child startup and is never keyed by thread ID.
pub(crate) const LIVE_THREAD_REPORTING_MISMATCH: &str =
    "live thread completion reporting does not match the requested admission";

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) enum CompletionReporting {
    #[default]
    Automatic,
    SupervisorOwned,
}
