/// Represents all meaningful lifecycle transitions for a GitLab Merge Request.
///
/// Each variant maps to a distinct scenario the application may react to.
/// The policy that governs what happens for each scenario is defined by
/// [`MrEventPolicy`] — keeping domain rules out of the UI layer.
#[derive(Debug, Clone, PartialEq)]
pub enum MrLifecycleEvent {
    /// The user requested tracking a new MR by ID.
    Added,
    /// The user removed a MR from the tracking list.
    Deleted,
    /// A periodic refresh returned updated data from GitLab.
    Refreshed,
    /// The MR was merged (state transition detected in a refresh).
    Merged,
    /// The MR was closed without being merged (state transition detected in a refresh).
    Closed,
    /// A GitLab API fetch failed for this MR.
    FetchFailed,
}

/// Governs the application's reaction to a [`MrLifecycleEvent`].
///
/// Implement this trait to define domain rules without coupling them to any
/// UI event loop. The default implementation ([`DefaultMrEventPolicy`]) covers
/// the standard GitLab tracker behaviour.
pub trait MrEventPolicy: Send + Sync {
    /// Whether the event should trigger a new API call to GitLab.
    fn needs_refetch(&self, event: &MrLifecycleEvent) -> bool;

    /// Whether the event should cause the MR to be removed from the tracked list.
    fn should_remove(&self, event: &MrLifecycleEvent) -> bool;

    /// Whether the event should be surfaced as a desktop notification.
    fn should_notify(&self, event: &MrLifecycleEvent) -> bool;

    /// Whether the event requires persisting state to disk.
    fn needs_persist(&self, event: &MrLifecycleEvent) -> bool;
}

/// Standard policy for the GitLab tracker.
///
/// Rules are documented per-method. Override by implementing [`MrEventPolicy`]
/// on a custom struct and wiring it through dependency injection.
#[derive(Default)]
pub struct DefaultMrEventPolicy;

impl MrEventPolicy for DefaultMrEventPolicy {
    fn needs_refetch(&self, event: &MrLifecycleEvent) -> bool {
        matches!(event, MrLifecycleEvent::Added)
    }

    fn should_remove(&self, event: &MrLifecycleEvent) -> bool {
        matches!(event, MrLifecycleEvent::Deleted)
    }

    fn should_notify(&self, event: &MrLifecycleEvent) -> bool {
        matches!(
            event,
            MrLifecycleEvent::Merged | MrLifecycleEvent::Closed | MrLifecycleEvent::FetchFailed
        )
    }

    fn needs_persist(&self, event: &MrLifecycleEvent) -> bool {
        matches!(
            event,
            MrLifecycleEvent::Added
                | MrLifecycleEvent::Deleted
                | MrLifecycleEvent::Merged
                | MrLifecycleEvent::Closed
                | MrLifecycleEvent::Refreshed
        )
    }
}
