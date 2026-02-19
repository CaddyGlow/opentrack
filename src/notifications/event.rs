use crate::tracking::{TrackingEvent, TrackingStatus};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationTrigger {
    Any,
    StatusChange,
    Delivered,
    Exception,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotificationEvent {
    pub parcel_id: String,
    pub provider: String,
    pub label: Option<String>,
    pub trigger: NotificationTrigger,
    pub old_status: Option<TrackingStatus>,
    pub new_status: TrackingStatus,
    pub latest_event: Option<TrackingEvent>,
}
