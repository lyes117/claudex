use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use crate::outgoing_message::OutgoingMessageSender;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::SkillsChangedNotification;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_file_watcher::FileWatcher;
use codex_file_watcher::FileWatcherSubscriber;
use codex_file_watcher::Receiver;
use codex_file_watcher::ThrottledWatchReceiver;
use codex_file_watcher::WatchPath;
use codex_file_watcher::WatchRegistration;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_skills::system_cache_root_dir;
use codex_skills_extension::HostSkillsLoadInput;
use codex_skills_extension::HostSkillsService;
use codex_utils_absolute_path::AbsolutePathBuf;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;
use tracing::warn;

#[cfg(not(test))]
const WATCHER_THROTTLE_INTERVAL: Duration = Duration::from_secs(10);
#[cfg(test)]
const WATCHER_THROTTLE_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) struct SkillsWatcher {
    subscriber: FileWatcherSubscriber,
    runtime_extra_roots_registration: Mutex<WatchRegistration>,
    shutdown_token: CancellationToken,
    _shutdown_drop_guard: DropGuard,
}

impl SkillsWatcher {
    pub(crate) fn new(
        skills_service: Arc<HostSkillsService>,
        codex_home: &AbsolutePathBuf,
        outgoing: Arc<OutgoingMessageSender>,
    ) -> Arc<Self> {
        let file_watcher = match FileWatcher::new() {
            Ok(file_watcher) => Arc::new(file_watcher),
            Err(err) => {
                warn!("failed to initialize skills file watcher: {err}");
                Arc::new(FileWatcher::noop())
            }
        };
        let (subscriber, rx) = file_watcher.add_subscriber();
        let shutdown_token = CancellationToken::new();
        let shutdown_drop_guard = shutdown_token.clone().drop_guard();
        let system_skills_root = system_cache_root_dir(codex_home);
        Self::spawn_event_loop(
            rx,
            skills_service,
            system_skills_root,
            outgoing,
            shutdown_token.child_token(),
        );
        Arc::new(Self {
            subscriber,
            runtime_extra_roots_registration: Mutex::new(WatchRegistration::default()),
            shutdown_token,
            _shutdown_drop_guard: shutdown_drop_guard,
        })
    }

    pub(crate) fn shutdown(&self) {
        self.shutdown_token.cancel();
    }

    pub(crate) fn register_runtime_extra_roots(&self, extra_roots: &[AbsolutePathBuf]) {
        let roots = extra_roots
            .iter()
            .map(|root| WatchPath {
                path: root.clone().into_path_buf(),
                recursive: true,
            })
            .collect();
        let registration = self.subscriber.register_paths(roots);
        let mut guard = self
            .runtime_extra_roots_registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = registration;
    }

    pub(crate) async fn register_thread_config(
        &self,
        config: &Config,
        thread_manager: &ThreadManager,
        environments: &[TurnEnvironmentSelection],
    ) -> WatchRegistration {
        let Some(environment_selection) = environments.first() else {
            return WatchRegistration::default();
        };
        let Some(environment) = thread_manager
            .environment_manager()
            .get_environment(&environment_selection.environment_id)
        else {
            warn!(
                "failed to register skills watcher for unknown environment `{}`",
                environment_selection.environment_id
            );
            return WatchRegistration::default();
        };
        if environment.is_remote() {
            return WatchRegistration::default();
        }

        let plugins_input = config.plugins_config_input();
        let plugins_manager = thread_manager.plugins_manager();
        let plugin_outcome = plugins_manager.plugins_for_config(&plugins_input).await;
        let legacy_selection = config.claude_plugin_selection(&plugin_outcome);
        let skills_input = HostSkillsLoadInput::new(
            config.cwd.clone(),
            plugin_outcome.effective_plugin_skill_roots(),
            config.config_layer_stack.clone(),
        )
        .with_legacy_plugin_selection(legacy_selection);
        let selected_roots = thread_manager
            .skills_service()
            .watchable_skill_root_paths(&skills_input, environment.get_filesystem())
            .await;
        // Retain fallback roots under surveillance so edits while excluded invalidate
        // their old cache entries before a later marker/config/thread exclusion restores them.
        let fallback_input = skills_input.with_legacy_plugin_selection(Default::default());
        let fallback_roots = thread_manager
            .skills_service()
            .watchable_skill_root_paths(&fallback_input, environment.get_filesystem())
            .await;
        let marker_parent = codex_config::claude::user_config_enabled(
            config.config_layer_stack.layers_low_to_high(),
        )
        .then(codex_config::claude::home)
        .flatten()
        .and_then(|home| home.parent().map(|parent| parent.join(".claudex/memory")))
        .filter(|directory| directory.is_dir());
        let roots = watch_paths_with_fallback(selected_roots, fallback_roots, marker_parent);
        self.subscriber.register_paths(roots)
    }

    fn spawn_event_loop(
        rx: Receiver,
        skills_service: Arc<HostSkillsService>,
        system_skills_root: AbsolutePathBuf,
        outgoing: Arc<OutgoingMessageSender>,
        shutdown_token: CancellationToken,
    ) {
        let mut rx = ThrottledWatchReceiver::new(rx, WATCHER_THROTTLE_INTERVAL);
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            warn!("skills watcher listener skipped: no Tokio runtime available");
            return;
        };
        handle.spawn(async move {
            loop {
                let event = tokio::select! {
                    _ = shutdown_token.cancelled() => break,
                    event = rx.recv() => event,
                };
                let Some(event) = event else {
                    break;
                };
                // The legacy user-skills root contains `.system` and is watched recursively.
                if event
                    .paths
                    .iter()
                    .all(|path| path.starts_with(system_skills_root.as_path()))
                {
                    continue;
                }
                skills_service.clear_cache();
                outgoing
                    .send_server_notification(ServerNotification::SkillsChanged(
                        SkillsChangedNotification {},
                    ))
                    .await;
            }
        });
    }
}

fn watch_paths_with_fallback(
    mut selected_roots: Vec<AbsolutePathBuf>,
    fallback_roots: Vec<AbsolutePathBuf>,
    marker_parent: Option<std::path::PathBuf>,
) -> Vec<WatchPath> {
    selected_roots.extend(fallback_roots);
    selected_roots.sort();
    selected_roots.dedup();
    let mut paths: Vec<_> = selected_roots
        .into_iter()
        .map(|path| WatchPath {
            path: path.into_path_buf(),
            recursive: true,
        })
        .collect();
    if let Some(path) = marker_parent {
        paths.push(WatchPath {
            path,
            recursive: false,
        });
    }
    paths
}

#[cfg(test)]
#[path = "skills_watcher_tests.rs"]
mod tests;
