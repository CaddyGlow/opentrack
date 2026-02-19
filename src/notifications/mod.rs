use async_trait::async_trait;
use futures::future::join_all;

use crate::tracking::{TrackingInfo, TrackingStatus};
use crate::{Error, Result};

pub mod event;

pub use event::{NotificationEvent, NotificationTrigger};

#[async_trait]
pub trait Notifier: Send + Sync {
    fn type_id(&self) -> &'static str;
    async fn notify(&self, event: &NotificationEvent) -> Result<()>;
}

pub async fn dispatch(
    notifiers: &[(Vec<NotificationTrigger>, Box<dyn Notifier>)],
    event: &NotificationEvent,
) {
    let futures = notifiers
        .iter()
        .filter(|(triggers, _)| triggers.iter().any(|t| t == &event.trigger))
        .map(|(_, notifier)| notifier.notify(event));

    for result in join_all(futures).await {
        if let Err(err) = result {
            tracing::warn!(error = %err, "failed to send notification");
        }
    }
}

pub fn evaluate_triggers(
    old: Option<&TrackingInfo>,
    new: &TrackingInfo,
) -> Vec<NotificationTrigger> {
    let mut triggers = Vec::new();

    let old_status = old.map(|info| &info.status);
    if old_status != Some(&new.status) {
        triggers.push(NotificationTrigger::StatusChange);
    }

    let old_count = old.map_or(0, |info| info.events.len());
    let new_count = new.events.len();
    let old_latest = old
        .and_then(|info| info.events.first())
        .map(|event| event.timestamp);
    let new_latest = new.events.first().map(|event| event.timestamp);
    if new_count > old_count || old_latest != new_latest {
        triggers.push(NotificationTrigger::Any);
    }

    if new.status == TrackingStatus::Delivered && old_status != Some(&TrackingStatus::Delivered) {
        triggers.push(NotificationTrigger::Delivered);
    }

    if new.status == TrackingStatus::Exception && old_status != Some(&TrackingStatus::Exception) {
        triggers.push(NotificationTrigger::Exception);
    }

    triggers
}

pub fn build_event(
    trigger: NotificationTrigger,
    label: Option<String>,
    old: Option<&TrackingInfo>,
    new: &TrackingInfo,
) -> NotificationEvent {
    NotificationEvent {
        parcel_id: new.parcel_id.clone(),
        provider: new.provider.clone(),
        label,
        trigger,
        old_status: old.map(|info| info.status.clone()),
        new_status: new.status.clone(),
        latest_event: new.events.first().cloned(),
    }
}

pub fn build_notifiers(
    _cfgs: &[crate::config::NotificationConfig],
    _http_client: &wreq::Client,
) -> Vec<(Vec<NotificationTrigger>, Box<dyn Notifier>)> {
    Vec::new()
}

#[derive(Debug, thiserror::Error)]
#[error("notifier not configured")]
struct NotConfigured;

pub struct NoopNotifier;

#[async_trait]
impl Notifier for NoopNotifier {
    fn type_id(&self) -> &'static str {
        "noop"
    }

    async fn notify(&self, _event: &NotificationEvent) -> Result<()> {
        Err(Error::Notification {
            notifier: self.type_id().to_string(),
            source: Box::new(NotConfigured),
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::*;
    use crate::tracking::{TrackingEvent, TrackingInfo};

    fn make_info(status: TrackingStatus, ts: i64) -> TrackingInfo {
        TrackingInfo {
            parcel_id: "123".to_string(),
            provider: "laposte".to_string(),
            status,
            events: vec![TrackingEvent {
                timestamp: Utc::now() + Duration::seconds(ts),
                description: "event".to_string(),
                location: None,
                raw_code: None,
            }],
            estimated_delivery: None,
            destination_postcode: None,
        }
    }

    #[test]
    fn delivered_transition_emits_expected_triggers() {
        let old = make_info(TrackingStatus::InTransit, 0);
        let new = make_info(TrackingStatus::Delivered, 1);
        let triggers = evaluate_triggers(Some(&old), &new);

        assert!(triggers.contains(&NotificationTrigger::Any));
        assert!(triggers.contains(&NotificationTrigger::StatusChange));
        assert!(triggers.contains(&NotificationTrigger::Delivered));
        assert!(!triggers.contains(&NotificationTrigger::Exception));
    }
}
