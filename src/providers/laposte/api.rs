use crate::Result;

use super::models::LaPosteResponse;

pub async fn fetch_tracking(
    client: &wreq::Client,
    parcel_id: &str,
    lang: &str,
) -> Result<Vec<LaPosteResponse>> {
    let url = format!("https://www.laposte.fr/ssu/sun/back/suivi-unifie/{parcel_id}?lang={lang}");
    let referer = format!("https://www.laposte.fr/outils/suivre-vos-envois?code={parcel_id}");

    let response = client.get(url).header("Referer", referer).send().await?;
    let parsed = response.json::<Vec<LaPosteResponse>>().await?;
    Ok(parsed)
}
