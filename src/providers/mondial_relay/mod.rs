use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::sync::Arc;

use crate::config::MondialRelayMode;
use crate::providers::Provider;
use crate::tracking::{TrackOptions, TrackingEvent, TrackingInfo, TrackingStatus};
use crate::{Error, Result};

pub mod api;
pub mod browser_cdp;
pub mod models;
pub mod token;
mod transport;

use transport::TrackingTransport;

/// Known Mondial Relay brand codes. The configured brand is tried first,
/// then the remaining alternatives are attempted on "no parcel found" errors.
const KNOWN_BRANDS: &[&str] = &["PP", "MR", "CC", "24R"];

pub struct MondialRelayProvider {
    transport: Box<dyn TrackingTransport>,
    brands: Vec<String>,
}

impl MondialRelayProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: wreq::Client,
        country: String,
        brand: String,
        mode: MondialRelayMode,
        cdp_browser: Arc<browser_cdp::SharedCdpBrowser>,
    ) -> Self {
        let transport =
            transport::build_tracking_transport(client, country, mode, cdp_browser);
        // Put the configured brand first, then append any other known brands.
        let mut brands = vec![brand.clone()];
        for &known in KNOWN_BRANDS {
            if !known.eq_ignore_ascii_case(&brand) {
                brands.push(known.to_string());
            }
        }
        Self { transport, brands }
    }

    pub fn detect_id(parcel_id: &str) -> bool {
        (7..=10).contains(&parcel_id.len()) && parcel_id.chars().all(|c| c.is_ascii_digit())
    }

    fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
        let input = value.trim();
        if input.is_empty() {
            return None;
        }

        if let Ok(parsed) = DateTime::parse_from_rfc3339(input) {
            return Some(parsed.with_timezone(&Utc));
        }

        for format in [
            "%Y-%m-%d %H:%M:%S%.f",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d %H:%M",
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%dT%H:%M",
            "%d/%m/%Y %H:%M:%S%.f",
            "%d/%m/%Y %H:%M:%S",
            "%d/%m/%Y %H:%M",
            "%d-%m-%Y %H:%M:%S%.f",
            "%d-%m-%Y %H:%M:%S",
            "%d-%m-%Y %H:%M",
        ] {
            if let Ok(parsed) = NaiveDateTime::parse_from_str(input, format) {
                return Some(parsed.and_utc());
            }
        }

        None
    }

    fn map_status_text(text: &str) -> Option<TrackingStatus> {
        let normalized = text.to_lowercase();

        let contains_any =
            |needles: &[&str]| needles.iter().any(|needle| normalized.contains(needle));

        if contains_any(&[
            "livre",
            "livré",
            "destinataire",
            "distrib",
            "retire",
            "retiré",
            "delivered",
        ]) {
            return Some(TrackingStatus::Delivered);
        }

        if contains_any(&[
            "exception",
            "incident",
            "impossible",
            "retour",
            "erreur",
            "echec",
            "échec",
            "refuse",
            "refusé",
        ]) {
            return Some(TrackingStatus::Exception);
        }

        if contains_any(&[
            "en livraison",
            "cours de livraison",
            "tournee",
            "tournée",
            "distribution",
            "out for delivery",
        ]) {
            return Some(TrackingStatus::OutForDelivery);
        }

        if contains_any(&[
            "transit",
            "agence",
            "achemin",
            "en route",
            "pris en charge",
            "remis",
            "expedie",
            "expédié",
        ]) {
            return Some(TrackingStatus::InTransit);
        }

        if contains_any(&["preparation", "préparation", "annonce", "etiquette creee"]) {
            return Some(TrackingStatus::PreShipment);
        }

        None
    }

    fn map_status(
        steps: &[models::MondialRelayStep],
        events: &[TrackingEvent],
        fallback_steps_count: usize,
    ) -> TrackingStatus {
        if let Some(active_step) = steps.iter().find(|step| step.is_active())
            && let Some(text) = active_step.as_text()
            && let Some(status) = Self::map_status_text(text)
        {
            return status;
        }

        if let Some((_, reached_step)) = steps
            .iter()
            .enumerate()
            .filter(|(_, step)| step.is_reached())
            .max_by_key(|(idx, step)| step.rank(*idx))
            && let Some(text) = reached_step.as_text()
            && let Some(status) = Self::map_status_text(text)
        {
            return status;
        }

        if let Some(event) = events.first()
            && let Some(status) = Self::map_status_text(&event.description)
        {
            return status;
        }

        if fallback_steps_count == 0 {
            TrackingStatus::Unknown
        } else if fallback_steps_count <= 1 {
            TrackingStatus::PreShipment
        } else {
            TrackingStatus::InTransit
        }
    }

    pub(crate) fn map_response(
        parcel_id: &str,
        postcode: Option<&str>,
        response: &models::MondialRelayResponse,
    ) -> TrackingInfo {
        let mut events: Vec<TrackingEvent> = response
            .events_recursive()
            .iter()
            .filter_map(|event| {
                let timestamp = event
                    .date
                    .as_deref()
                    .and_then(Self::parse_datetime)
                    .unwrap_or_else(Utc::now);

                let description = event.description.clone()?;
                let description = description.trim().to_string();
                if description.is_empty() {
                    return None;
                }

                let location = event.resolved_location();

                Some(TrackingEvent {
                    timestamp,
                    description,
                    location,
                    raw_code: event.code.clone(),
                })
            })
            .collect();

        events.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));

        let steps = response.steps_recursive();
        let status = Self::map_status(steps, &events, steps.len());

        let estimated_delivery = response
            .estimated_delivery_recursive()
            .and_then(Self::parse_datetime);

        let destination_postcode = response
            .destination_postcode_recursive()
            .map(std::string::ToString::to_string)
            .or_else(|| postcode.map(std::string::ToString::to_string));

        TrackingInfo {
            parcel_id: parcel_id.to_string(),
            provider: "mondial-relay".to_string(),
            status,
            events,
            estimated_delivery,
            destination_postcode,
        }
    }

    fn map_provider_response(
        parcel_id: &str,
        opts: &TrackOptions,
        response: models::MondialRelayResponse,
    ) -> Result<TrackingInfo> {
        if let Some(code) = response.code_retour_recursive().filter(|code| *code != 0) {
            let message = response
                .message_recursive()
                .unwrap_or("Mondial Relay returned an error")
                .to_string();
            tracing::warn!(
                provider = "mondial-relay",
                parcel_id = %parcel_id,
                code,
                message = %message,
                "Mondial Relay response returned non-zero CodeRetour"
            );
            return Err(Error::ProviderError { code, message });
        }

        if let Some(status_msg) = response.status_error_message() {
            tracing::warn!(
                provider = "mondial-relay",
                parcel_id = %parcel_id,
                message = %status_msg,
                "Mondial Relay response contained status error"
            );
            return Err(Error::ProviderError {
                code: 0,
                message: status_msg,
            });
        }

        Ok(Self::map_response(
            parcel_id,
            opts.postcode.as_deref(),
            &response,
        ))
    }
}

#[async_trait]
impl Provider for MondialRelayProvider {
    fn id(&self) -> &'static str {
        "mondial-relay"
    }

    fn name(&self) -> &'static str {
        "Mondial Relay"
    }

    fn detect(&self, parcel_id: &str) -> bool {
        Self::detect_id(parcel_id)
    }

    fn tracking_url(&self, parcel_id: &str, _opts: &TrackOptions) -> String {
        let brand = self.brands.first().map(String::as_str).unwrap_or("PP");
        format!(
            "https://www.mondialrelay.fr/suivi-de-colis?codeMarque={brand}&numeroExpedition={parcel_id}&pays=FR&language=fr"
        )
    }

    async fn track(&self, parcel_id: &str, opts: &TrackOptions) -> Result<TrackingInfo> {
        tracing::debug!(
            provider = "mondial-relay",
            parcel_id = %parcel_id,
            has_postcode = opts.postcode.is_some(),
            mode = ?self.transport.mode(),
            brands = ?self.brands,
            "starting Mondial Relay track request"
        );

        let mut last_error = None;

        for (idx, brand) in self.brands.iter().enumerate() {
            let response = self
                .transport
                .fetch_tracking_response(parcel_id, opts, brand)
                .await;

            match response {
                Ok(resp) => {
                    // Check for "no parcel found" status errors -- try next brand.
                    if resp.status_error_message().is_some() && idx + 1 < self.brands.len() {
                        tracing::debug!(
                            provider = "mondial-relay",
                            parcel_id = %parcel_id,
                            brand = %brand,
                            status_msg = %resp.status_error_message().unwrap_or_default(),
                            "brand returned status error, trying next brand"
                        );
                        last_error =
                            Some(Self::map_provider_response(parcel_id, opts, resp).unwrap_err());
                        continue;
                    }
                    return Self::map_provider_response(parcel_id, opts, resp);
                }
                Err(err) => {
                    // On transport errors, don't retry with other brands.
                    return Err(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| Error::ProviderError {
            code: 0,
            message: "no brand matched for this parcel".to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_request_verification_token_name_first() {
        let html = r#"<input name="__RequestVerificationToken" type="hidden" value="TOKEN123" />"#;
        let token = token::extract_request_verification_token(html);
        assert_eq!(token.as_deref(), Some("TOKEN123"));
    }

    #[test]
    fn extracts_request_verification_token_value_first() {
        let html = r#"<input type="hidden" value="TOKEN456" name="__RequestVerificationToken" />"#;
        let token = token::extract_request_verification_token(html);
        assert_eq!(token.as_deref(), Some("TOKEN456"));
    }

    #[test]
    fn maps_fixture_to_delivered_status() {
        let fixture = include_str!("../../../tests/fixtures/mondial_relay/tracking_delivered.json");
        let response: models::MondialRelayResponse =
            serde_json::from_str(fixture).expect("valid fixture");

        let info = MondialRelayProvider::map_response("12345678", Some("00000"), &response);
        assert_eq!(info.status, TrackingStatus::Delivered);
        assert!(!info.events.is_empty());
        assert_eq!(info.destination_postcode.as_deref(), Some("00000"));
    }

    #[test]
    fn parse_datetime_supports_fractional_seconds() {
        let parsed = MondialRelayProvider::parse_datetime("2025-10-23T13:09:13.476")
            .expect("should parse fractional seconds");
        assert_eq!(parsed.to_rfc3339(), "2025-10-23T13:09:13.476+00:00");
    }
}
