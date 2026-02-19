use chrono::{DateTime, Utc};

#[derive(Debug, Default, Clone)]
pub struct TrackOptions {
    pub postcode: Option<String>,
    pub lang: Option<String>,
    pub no_cache: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrackingInfo {
    pub parcel_id: String,
    pub provider: String,
    pub status: TrackingStatus,
    pub events: Vec<TrackingEvent>,
    pub estimated_delivery: Option<DateTime<Utc>>,
    pub destination_postcode: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrackingEvent {
    pub timestamp: DateTime<Utc>,
    pub description: String,
    pub location: Option<String>,
    pub raw_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TrackingStatus {
    PreShipment,
    InTransit,
    OutForDelivery,
    Delivered,
    Exception,
    Unknown,
}

impl std::fmt::Display for TrackingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreShipment => write!(f, "PreShipment"),
            Self::InTransit => write!(f, "InTransit"),
            Self::OutForDelivery => write!(f, "OutForDelivery"),
            Self::Delivered => write!(f, "Delivered"),
            Self::Exception => write!(f, "Exception"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}
