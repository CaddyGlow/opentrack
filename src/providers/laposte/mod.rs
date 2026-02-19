use async_trait::async_trait;
use chrono::Utc;

use crate::providers::Provider;
use crate::tracking::{TrackOptions, TrackingEvent, TrackingInfo, TrackingStatus};
use crate::{Error, Result};

pub mod api;
pub mod models;

pub struct LaposteProvider {
    client: wreq::Client,
    default_lang: String,
}

impl LaposteProvider {
    pub fn new(client: wreq::Client, default_lang: String) -> Self {
        Self {
            client,
            default_lang,
        }
    }

    pub fn detect_id(parcel_id: &str) -> bool {
        if parcel_id.len() != 13 {
            return false;
        }

        let mut chars = parcel_id.chars();
        let prefix_ok = chars
            .by_ref()
            .take(2)
            .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase());
        let rest_ok = chars.all(|c| c.is_ascii_digit());

        prefix_ok && rest_ok
    }

    fn map_status(group: &str, is_final: bool) -> TrackingStatus {
        match group {
            "EXPANN" => TrackingStatus::PreShipment,
            "ACHNAT" | "DISARR" => TrackingStatus::InTransit,
            "DISTOU" => TrackingStatus::OutForDelivery,
            "DESBAL" | "DESTIN" if is_final => TrackingStatus::Delivered,
            "DESBAL" | "DESTIN" => TrackingStatus::Delivered,
            "RETOUR" => TrackingStatus::Exception,
            _ => TrackingStatus::Unknown,
        }
    }

    fn is_success_return_code(code: u32) -> bool {
        matches!(code, 0 | 200)
    }

    fn map_events(events: &[models::LaPosteEvent]) -> Vec<TrackingEvent> {
        let mut sorted = events.to_vec();
        sorted.sort_by(|a, b| b.order.cmp(&a.order));

        sorted
            .into_iter()
            .map(|event| TrackingEvent {
                timestamp: event.date.with_timezone(&Utc),
                description: event.label,
                location: (!event.country.is_empty()).then_some(event.country),
                raw_code: Some(format!("{}/{}", event.group, event.code)),
            })
            .collect()
    }

    pub(crate) fn map_response(
        parcel_id: &str,
        response: &models::LaPosteResponse,
    ) -> TrackingInfo {
        let latest = response
            .shipment
            .event
            .iter()
            .max_by_key(|event| event.order);
        let status = latest
            .map(|event| Self::map_status(&event.group, response.shipment.is_final))
            .unwrap_or(TrackingStatus::Unknown);

        TrackingInfo {
            parcel_id: parcel_id.to_string(),
            provider: "laposte".to_string(),
            status,
            events: Self::map_events(&response.shipment.event),
            estimated_delivery: response
                .shipment
                .estim_date
                .map(|date| date.with_timezone(&Utc)),
            destination_postcode: None,
        }
    }
}

#[async_trait]
impl Provider for LaposteProvider {
    fn id(&self) -> &'static str {
        "laposte"
    }

    fn name(&self) -> &'static str {
        "La Poste / Colissimo"
    }

    fn detect(&self, parcel_id: &str) -> bool {
        Self::detect_id(parcel_id)
    }

    async fn track(&self, parcel_id: &str, opts: &TrackOptions) -> Result<TrackingInfo> {
        let lang = opts.lang.as_deref().unwrap_or(&self.default_lang);
        let data = api::fetch_tracking(&self.client, parcel_id, lang).await?;
        let Some(first) = data.first() else {
            return Err(Error::ProviderError {
                code: 0,
                message: "laposte returned an empty response".to_string(),
            });
        };

        if !Self::is_success_return_code(first.return_code) {
            return Err(Error::ProviderError {
                code: first.return_code,
                message: first.return_message.clone(),
            });
        }

        Ok(Self::map_response(parcel_id, first))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_fixture_to_delivered_status() {
        let fixture = include_str!("../../../tests/fixtures/laposte/tracking_delivered.json");
        let parsed: Vec<models::LaPosteResponse> =
            serde_json::from_str(fixture).expect("valid fixture");
        let info = LaposteProvider::map_response("AB12345678901", &parsed[0]);

        assert_eq!(info.status, TrackingStatus::Delivered);
        assert!(!info.events.is_empty());
    }

    #[test]
    fn return_code_200_is_treated_as_success() {
        assert!(LaposteProvider::is_success_return_code(200));
        assert!(LaposteProvider::is_success_return_code(0));
        assert!(!LaposteProvider::is_success_return_code(500));
    }
}
