use crate::{Error, Result};

use super::models::MondialRelayResponse;

pub async fn fetch_tracking(
    client: &wreq::Client,
    shipment: &str,
    postcode: Option<&str>,
    brand: &str,
    country: &str,
    request_verification_token: &str,
) -> Result<MondialRelayResponse> {
    let postcode_q = postcode.unwrap_or_default();
    let referer_country = country.to_uppercase();
    let country_q = country.to_lowercase();
    let url = format!(
        "https://www.mondialrelay.fr/api/tracking?shipment={shipment}&postcode={postcode_q}&brand={brand}&codePays={country_q}"
    );
    let referer = format!(
        "https://www.mondialrelay.fr/suivi-de-colis?codeMarque={brand}&numeroExpedition={shipment}&pays={referer_country}&language=fr"
    );
    tracing::debug!(
        provider = "mondial-relay",
        shipment = %shipment,
        has_postcode = postcode.is_some(),
        brand = %brand,
        country = %country_q,
        url = %url,
        "sending Mondial Relay tracking API request"
    );

    let (status, body) =
        send_tracking_request(client, &url, &referer, request_verification_token).await?;

    if !status.is_success() {
        let mut message = format!(
            "Mondial Relay API returned HTTP {} with body: {}",
            status,
            preview(&body, 180)
        );

        let body_lower = body.to_lowercase();
        if status.as_u16() == 401 && body_lower.contains("autorisation a") {
            message = format!(
                "{}. This usually means the anti-bot/CSRF session was rejected by Mondial Relay (Cloudflare or invalid session).",
                message
            );
        }
        tracing::warn!(
            provider = "mondial-relay",
            shipment = %shipment,
            status = %status,
            body_preview = %preview(&body, 180),
            "Mondial Relay tracking API returned non-success status"
        );

        return Err(Error::ProviderError {
            code: status.as_u16() as u32,
            message,
        });
    }

    parse_tracking_response(&body, postcode.is_some())
}

pub async fn fetch_tracking_page(
    client: &wreq::Client,
    shipment: &str,
    country: &str,
    brand: &str,
) -> Result<String> {
    let country_upper = country.to_uppercase();
    let url = format!(
        "https://www.mondialrelay.fr/suivi-de-colis?codeMarque={brand}&numeroExpedition={shipment}&pays={country_upper}&language=fr"
    );

    let response = client.get(url).send().await?;
    let status = response.status();
    let html = response.text().await?;
    tracing::debug!(
        provider = "mondial-relay",
        shipment = %shipment,
        status = %status,
        html_len = html.len(),
        "fetched Mondial Relay tracking page"
    );

    if looks_like_cloudflare_block(&html) {
        tracing::warn!(
            provider = "mondial-relay",
            shipment = %shipment,
            "Mondial Relay tracking page appears blocked by Cloudflare"
        );
        return Err(Error::ProviderError {
            code: 403,
            message:
                "Mondial Relay tracking page is blocked by Cloudflare from this network. Try another network or a proxy."
                    .to_string(),
        });
    }

    Ok(html)
}

pub(crate) fn parse_tracking_response(
    body: &str,
    postcode_supplied: bool,
) -> Result<MondialRelayResponse> {
    maybe_dump_raw_response(body);

    let trimmed = body.trim();
    if trimmed.is_empty() {
        tracing::warn!(
            provider = "mondial-relay",
            postcode_supplied,
            "Mondial Relay returned an empty tracking response body"
        );
        let hint = if postcode_supplied {
            "Mondial Relay returned an empty response body."
        } else {
            "Mondial Relay returned an empty response body; this shipment may require `--postcode`."
        };
        return Err(Error::ProviderError {
            code: 0,
            message: hint.to_string(),
        });
    }

    if trimmed.starts_with('<') {
        tracing::warn!(
            provider = "mondial-relay",
            postcode_supplied,
            body_preview = %preview(trimmed, 180),
            "Mondial Relay returned HTML instead of JSON"
        );
        if looks_like_cloudflare_block(trimmed) {
            return Err(Error::ProviderError {
                code: 403,
                message:
                    "Mondial Relay returned a Cloudflare block page instead of JSON. Try another network or a proxy."
                        .to_string(),
            });
        }

        return Err(Error::ProviderError {
            code: 0,
            message: format!(
                "Mondial Relay returned HTML instead of JSON. Body preview: {}",
                preview(trimmed, 180)
            ),
        });
    }

    serde_json::from_str::<MondialRelayResponse>(trimmed).map_err(|err| {
        tracing::warn!(
            provider = "mondial-relay",
            error = %err,
            body_preview = %preview(trimmed, 180),
            "failed to parse Mondial Relay JSON body"
        );
        Error::ProviderError {
            code: 0,
            message: format!(
                "failed to parse Mondial Relay JSON: {err}; body preview: {}",
                preview(trimmed, 180)
            ),
        }
    })
}

async fn send_tracking_request(
    client: &wreq::Client,
    url: &str,
    referer: &str,
    request_verification_token: &str,
) -> Result<(wreq::StatusCode, String)> {
    tracing::debug!(
        provider = "mondial-relay",
        method = "GET",
        requestverificationtoken = %request_verification_token,
        url = %url,
        referer = %referer,
        "dispatching Mondial Relay API HTTP request"
    );
    let builder = client
        .get(url)
        .header("requestverificationtoken", request_verification_token)
        .header("x-requested-with", "XMLHttpRequest")
        .header("accept", "application/json, text/plain, */*")
        .header("accept-language", "en-US,en;q=0.9,fr;q=0.8")
        .header("cache-control", "no-cache")
        .header("pragma", "no-cache")
        .header("origin", "https://www.mondialrelay.fr")
        .header("referer", referer)
        .header("sec-fetch-dest", "empty")
        .header("sec-fetch-mode", "cors")
        .header("sec-fetch-site", "same-origin");

    let response = builder.send().await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("<missing>")
        .to_string();
    let body = response.text().await?;
    tracing::debug!(
        provider = "mondial-relay",
        method = "GET",
        status = %status,
        content_type = %content_type,
        body_len = body.len(),
        body_starts_with_html = body.trim_start().starts_with('<'),
        "received Mondial Relay API HTTP response"
    );
    Ok((status, body))
}

fn preview(input: &str, max_chars: usize) -> String {
    let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = compact.chars().take(max_chars).collect::<String>();
    if compact.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn looks_like_cloudflare_block(input: &str) -> bool {
    let lower = input.to_lowercase();
    lower.contains("attention required! | cloudflare")
        || lower.contains("cf-error-details")
        || lower.contains("sorry, you have been blocked")
}

fn maybe_dump_raw_response(body: &str) {
    if std::env::var_os("OPENTRACK_MONDIAL_DUMP_RESPONSE").is_none() {
        return;
    }

    eprintln!("--- MONDIAL_RELAY_RAW_RESPONSE_BEGIN ---");
    eprintln!("{body}");
    eprintln!("--- MONDIAL_RELAY_RAW_RESPONSE_END ---");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_body_without_postcode_has_helpful_hint() {
        let err = parse_tracking_response("", false).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("--postcode"));
    }

    #[test]
    fn html_body_returns_specific_error() {
        let err = parse_tracking_response("<html>blocked</html>", true).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("HTML"));
    }

    #[test]
    fn valid_json_parses() {
        let parsed = parse_tracking_response(r#"{"CodeRetour":0,"Message":"OK"}"#, true)
            .expect("valid json");
        assert_eq!(parsed.code_retour_recursive(), Some(0));
    }

    #[test]
    fn cloudflare_block_detection_works() {
        assert!(looks_like_cloudflare_block(
            "<title>Attention Required! | Cloudflare</title>"
        ));
        assert!(!looks_like_cloudflare_block("{\"CodeRetour\":0}"));
    }
}
