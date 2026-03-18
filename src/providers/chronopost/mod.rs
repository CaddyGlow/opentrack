use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};

use crate::providers::Provider;
use crate::tracking::{TrackOptions, TrackingEvent, TrackingInfo, TrackingStatus};
use crate::{Error, Result};

pub mod api;
pub mod models;

pub struct ChronopostProvider {
    client: wreq::Client,
    default_lang: String,
}

impl ChronopostProvider {
    pub fn new(client: wreq::Client, default_lang: String) -> Self {
        Self {
            client,
            default_lang,
        }
    }

    /// Chronopost / Shop2Shop skybill numbers:
    /// - Typically 13-15 chars, alphanumeric, often starting with XM/XW/XY/EE/EP
    ///   and ending with two uppercase letters (e.g. XM002774533TS)
    /// - Can also be purely numeric (e.g. 13-digit Chronopost numbers)
    pub fn detect_id(parcel_id: &str) -> bool {
        let len = parcel_id.len();
        if !(11..=15).contains(&len) {
            return false;
        }

        let all_alnum = parcel_id.chars().all(|c| c.is_ascii_alphanumeric());
        if !all_alnum {
            return false;
        }

        // Must have at least one letter to distinguish from Mondial Relay (all-digit IDs)
        let has_letter = parcel_id.chars().any(|c| c.is_ascii_alphabetic());
        if !has_letter {
            return false;
        }

        // Common prefixes for Chronopost/Shop2Shop skybills
        let upper = parcel_id.to_ascii_uppercase();
        let known_prefix = upper.starts_with("XM")
            || upper.starts_with("XW")
            || upper.starts_with("XY")
            || upper.starts_with("EE")
            || upper.starts_with("EP")
            || upper.starts_with("EV");

        // Also match if it ends with two letters (common Chronopost pattern)
        let ends_with_letters = parcel_id
            .chars()
            .rev()
            .take(2)
            .all(|c| c.is_ascii_alphabetic());

        known_prefix || ends_with_letters
    }

    /// Map Chronopost event codes to TrackingStatus.
    /// See: https://www.chronopost.fr event code documentation
    fn map_event_code(code: &str) -> TrackingStatus {
        match code.trim() {
            // Preparation / pre-shipment
            "DC" => TrackingStatus::PreShipment,

            // Deposit / pickup
            "DB" | "SD" => TrackingStatus::InTransit,

            // In transit / sorting / transfer
            "PC" | "TT" | "TS" | "EC" | "RG" | "TA" | "TC" | "ET" | "EP" | "AG" | "SC"
            | "SM" | "RB" | "RE" | "AA" => TrackingStatus::InTransit,

            // Out for delivery
            "MD" | "ML" | "MC" => TrackingStatus::OutForDelivery,

            // Delivered
            "DI" | "LV" | "LP" | "BL" | "LR" => TrackingStatus::Delivered,

            // Exception / return / issue
            "RS" | "RI" | "NA" | "NH" | "ND" | "AR" | "NI" => TrackingStatus::Exception,

            _ => TrackingStatus::Unknown,
        }
    }

    fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
        let input = value.trim();
        if input.is_empty() {
            return None;
        }

        if let Ok(parsed) = DateTime::<FixedOffset>::parse_from_rfc3339(input) {
            return Some(parsed.with_timezone(&Utc));
        }

        // Chronopost uses ISO 8601 with offset: 2026-01-07T11:14:50+01:00
        if let Ok(parsed) = DateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S%:z") {
            return Some(parsed.with_timezone(&Utc));
        }

        None
    }

    fn build_location(event: &models::ChronopostEvent) -> Option<String> {
        let office = event.office_label.trim();
        let zip = event.zip_code.trim();

        match (office.is_empty(), zip.is_empty()) {
            (true, true) => None,
            (false, true) => Some(office.to_string()),
            (true, false) => Some(zip.to_string()),
            (false, false) => Some(format!("{office} ({zip})")),
        }
    }

    pub(crate) fn map_response(
        parcel_id: &str,
        response: &models::ChronopostResponse,
    ) -> TrackingInfo {
        let mut events: Vec<TrackingEvent> = response
            .events
            .iter()
            .filter_map(|event| {
                let timestamp = Self::parse_datetime(&event.event_date)?;
                Some(TrackingEvent {
                    timestamp,
                    description: event.event_label.clone(),
                    location: Self::build_location(event),
                    raw_code: Some(event.code.clone()),
                })
            })
            .collect();

        // Most recent first
        events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        let status = response
            .events
            .last()
            .map(|event| Self::map_event_code(&event.code))
            .unwrap_or(TrackingStatus::Unknown);

        TrackingInfo {
            parcel_id: parcel_id.to_string(),
            provider: "chronopost".to_string(),
            status,
            events,
            estimated_delivery: None,
            destination_postcode: None,
        }
    }
}

#[async_trait]
impl Provider for ChronopostProvider {
    fn id(&self) -> &'static str {
        "chronopost"
    }

    fn name(&self) -> &'static str {
        "Chronopost / Shop2Shop"
    }

    fn detect(&self, parcel_id: &str) -> bool {
        Self::detect_id(parcel_id)
    }

    async fn track(&self, parcel_id: &str, opts: &TrackOptions) -> Result<TrackingInfo> {
        let lang = opts.lang.as_deref().unwrap_or(&self.default_lang);
        let xml = api::fetch_tracking(&self.client, parcel_id, lang).await?;
        let response = models::ChronopostResponse::parse(&xml)?;

        if response.error_code != 0 {
            return Err(Error::ProviderError {
                code: response.error_code,
                message: format!(
                    "Chronopost returned error code {}",
                    response.error_code
                ),
            });
        }

        if response.events.is_empty() {
            return Err(Error::ProviderError {
                code: 0,
                message: "Chronopost returned no events for this parcel".to_string(),
            });
        }

        Ok(Self::map_response(parcel_id, &response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_chronopost_ids() {
        assert!(ChronopostProvider::detect_id("XM002774533TS"));
        assert!(ChronopostProvider::detect_id("EE123456789FR"));
        assert!(ChronopostProvider::detect_id("EP987654321FR"));
        // Should not match Mondial Relay (all-digit, 7-10 chars)
        assert!(!ChronopostProvider::detect_id("12345678"));
        // Should not match La Poste (2 alphanum + 11 digits)
        assert!(!ChronopostProvider::detect_id("6A04819842585"));
    }

    #[test]
    fn maps_intransit_fixture() {
        let xml = include_str!("../../../tests/fixtures/chronopost/tracking_intransit.xml");
        let response = models::ChronopostResponse::parse(xml).expect("valid xml");
        let info = ChronopostProvider::map_response("XM002774533TS", &response);
        assert_eq!(info.status, TrackingStatus::InTransit);
        assert_eq!(info.events.len(), 6);
        assert_eq!(info.provider, "chronopost");
    }

    #[test]
    fn maps_delivered_fixture() {
        let xml = include_str!("../../../tests/fixtures/chronopost/tracking_delivered.xml");
        let response = models::ChronopostResponse::parse(xml).expect("valid xml");
        let info = ChronopostProvider::map_response("XM002774000TS", &response);
        assert_eq!(info.status, TrackingStatus::Delivered);
        assert!(!info.events.is_empty());
    }

    #[test]
    fn parse_datetime_handles_offset() {
        use chrono::Timelike;
        let dt = ChronopostProvider::parse_datetime("2026-01-07T11:14:50+01:00");
        assert!(dt.is_some());
        let dt = dt.unwrap();
        assert_eq!(dt.hour(), 10); // UTC = 11:14 CET - 1h
    }
}
