use chrono::{DateTime, FixedOffset};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaPosteResponse {
    pub lang: String,
    pub return_code: u32,
    pub return_message: String,
    pub shipment: LaPosteShipment,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaPosteShipment {
    pub id_ship: String,
    pub product: Option<String>,
    pub is_final: bool,
    pub delivery_date: Option<DateTime<FixedOffset>>,
    pub entry_date: Option<DateTime<FixedOffset>>,
    pub estim_date: Option<DateTime<FixedOffset>>,
    pub timeline: Vec<LaPosteTimelineStep>,
    pub event: Vec<LaPosteEvent>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaPosteTimelineStep {
    pub id: u32,
    pub short_label: String,
    pub date: Option<DateTime<FixedOffset>>,
    pub status: bool,
    pub code: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaPosteEvent {
    pub group: String,
    pub code: String,
    pub label: String,
    pub date: DateTime<FixedOffset>,
    pub country: String,
    pub category: Option<String>,
    pub order: u32,
}
