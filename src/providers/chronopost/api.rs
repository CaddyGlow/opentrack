use crate::Result;

/// Chronopost SOAP tracking API. Returns raw XML response body.
pub async fn fetch_tracking(
    client: &wreq::Client,
    skybill_number: &str,
    lang: &str,
) -> Result<String> {
    let url = format!(
        "https://www.chronopost.fr/tracking-cxf/TrackingServiceWS/trackSkybill?language={lang}&skybillNumber={skybill_number}"
    );
    let referer = format!(
        "https://www.chronopost.fr/tracking-no-cms/suivi-page?listeNumerosLT={skybill_number}"
    );

    tracing::debug!(
        provider = "chronopost",
        skybill_number = %skybill_number,
        lang = %lang,
        url = %url,
        "sending Chronopost tracking API request"
    );

    let response = client.get(&url).header("Referer", referer).send().await?;

    let status = response.status();
    let body = response.text().await?;

    tracing::debug!(
        provider = "chronopost",
        skybill_number = %skybill_number,
        status = %status,
        body_len = body.len(),
        "received Chronopost tracking response"
    );

    if !status.is_success() {
        return Err(crate::Error::ProviderError {
            code: status.as_u16() as u32,
            message: format!("Chronopost API returned HTTP {status}"),
        });
    }

    Ok(body)
}
