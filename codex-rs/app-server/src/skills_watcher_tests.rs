use super::*;
use crate::outgoing_message::OutgoingEnvelope;
use crate::outgoing_message::OutgoingMessage;
use codex_config::ConfigLayerEntry;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_config::claude::LegacyPluginSelection;
use codex_exec_server::LOCAL_FS;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn fallback_watch_invalidates_metadata_before_reusing_the_cached_catalog() {
    let directory = tempfile::tempdir().unwrap();
    let home = AbsolutePathBuf::from_absolute_path(directory.path()).unwrap();
    let skill_root = directory.path().join("skills/memory");
    std::fs::create_dir_all(&skill_root).unwrap();
    let file = skill_root.join("SKILL.md");
    let v1 = "---\nname: memory-fixture\ndescription: version-one\n---\nbody v1\n";
    let v2 = "---\nname: memory-fixture\ndescription: version-two\n---\nbody v2\n";
    std::fs::write(&file, v1).unwrap();
    let stack = ConfigLayerStack::new(
        vec![ConfigLayerEntry::new(
            ConfigLayerSource::User {
                file: AbsolutePathBuf::from_absolute_path(directory.path().join("config.toml"))
                    .unwrap(),
                profile: None,
            },
            toml::Value::Table(Default::default()),
        )],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let input = HostSkillsLoadInput::new(home.clone(), Vec::new(), stack)
        .with_legacy_plugin_selection(LegacyPluginSelection::KeepAll);
    let service = Arc::new(HostSkillsService::new(home.clone(), false));
    let first = service
        .snapshot_for_config(&input, Some(Arc::clone(&LOCAL_FS)))
        .await;
    let skill = first
        .outcome()
        .skills
        .iter()
        .find(|s| s.name == "memory-fixture")
        .unwrap();
    assert_eq!(skill.description, "version-one");
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let outgoing = Arc::new(OutgoingMessageSender::new(
        tx,
        codex_analytics::AnalyticsEventsClient::disabled(),
    ));
    let watcher = SkillsWatcher::new(Arc::clone(&service), &home, outgoing);
    // The selected catalogue may exclude this root. Its fallback watcher must still invalidate it.
    let registration = watcher.subscriber.register_paths(watch_paths_with_fallback(
        Vec::new(),
        vec![AbsolutePathBuf::from_absolute_path(&skill_root).unwrap()],
        None,
    ));
    std::fs::write(&file, v2).unwrap();
    let envelope = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        envelope,
        OutgoingEnvelope::Broadcast {
            message: OutgoingMessage::AppServerNotification(_),
        }
    ));
    let restored = service
        .snapshot_for_config(&input, Some(Arc::clone(&LOCAL_FS)))
        .await;
    let skill = restored
        .outcome()
        .skills
        .iter()
        .find(|s| s.name == "memory-fixture")
        .unwrap();
    assert_eq!(skill.description, "version-two");
    assert_eq!(
        std::fs::read_to_string(skill.path_to_skills_md.as_path()).unwrap(),
        v2
    );
    assert!(!std::ptr::eq(first.outcome(), restored.outcome()));
    watcher.shutdown();
    drop(registration);
}
