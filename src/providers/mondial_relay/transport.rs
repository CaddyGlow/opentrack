use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::config::MondialRelayMode;
use crate::tracking::TrackOptions;
use crate::{Error, Result};

use super::{api, browser_cdp, models, token};

pub(crate) fn build_tracking_transport(
    client: wreq::Client,
    country: String,
    mode: MondialRelayMode,
    cdp_browser: Arc<browser_cdp::SharedCdpBrowser>,
) -> Box<dyn TrackingTransport> {
    match mode {
        MondialRelayMode::Api => Box::new(ApiTransport::new(client, country)),
        MondialRelayMode::Cdp => Box::new(CdpTransport::new(country, cdp_browser)),
    }
}

#[async_trait]
pub(crate) trait TrackingTransport: Send + Sync {
    fn mode(&self) -> MondialRelayMode;

    async fn fetch_tracking_response(
        &self,
        parcel_id: &str,
        opts: &TrackOptions,
        brand: &str,
    ) -> Result<models::MondialRelayResponse>;
}

struct ApiTransport {
    client: wreq::Client,
    country: String,
    request_verification_token: Arc<RwLock<Option<String>>>,
}

impl ApiTransport {
    fn new(client: wreq::Client, country: String) -> Self {
        Self {
            client,
            country,
            request_verification_token: Arc::new(RwLock::new(None)),
        }
    }

    async fn get_request_verification_token(&self, parcel_id: &str, brand: &str) -> Result<String> {
        if let Some(token) = self.request_verification_token.read().await.clone() {
            tracing::debug!(
                provider = "mondial-relay",
                parcel_id = %parcel_id,
                token_len = token.len(),
                "using cached requestverificationtoken"
            );
            return Ok(token);
        }

        tracing::debug!(
            provider = "mondial-relay",
            parcel_id = %parcel_id,
            country = %self.country,
            brand = %brand,
            "fetching Mondial Relay tracking page to extract requestverificationtoken"
        );
        let html = api::fetch_tracking_page(&self.client, parcel_id, &self.country, brand).await?;
        let token = token::extract_request_verification_token(&html).ok_or_else(|| {
            tracing::warn!(
                provider = "mondial-relay",
                parcel_id = %parcel_id,
                html_len = html.len(),
                has_token_marker = html.contains(token::REQUEST_VERIFICATION_TOKEN_MARKER),
                "failed to extract requestverificationtoken from Mondial Relay page"
            );
            Error::ProviderError {
                code: 0,
                message: "failed to extract Mondial Relay requestverificationtoken".to_string(),
            }
        })?;

        tracing::debug!(
            provider = "mondial-relay",
            parcel_id = %parcel_id,
            token_len = token.len(),
            "extracted requestverificationtoken from Mondial Relay page"
        );
        *self.request_verification_token.write().await = Some(token.clone());
        Ok(token)
    }

    async fn invalidate_request_verification_token(&self) {
        tracing::debug!(
            provider = "mondial-relay",
            "invalidating cached requestverificationtoken"
        );
        *self.request_verification_token.write().await = None;
    }
}

#[async_trait]
impl TrackingTransport for ApiTransport {
    fn mode(&self) -> MondialRelayMode {
        MondialRelayMode::Api
    }

    async fn fetch_tracking_response(
        &self,
        parcel_id: &str,
        opts: &TrackOptions,
        brand: &str,
    ) -> Result<models::MondialRelayResponse> {
        let token = self
            .get_request_verification_token(parcel_id, brand)
            .await?;
        let mut response = api::fetch_tracking(
            &self.client,
            parcel_id,
            opts.postcode.as_deref(),
            brand,
            &self.country,
            &token,
        )
        .await;

        if matches!(&response, Err(Error::ProviderError { code: 401, .. })) {
            tracing::warn!(
                provider = "mondial-relay",
                parcel_id = %parcel_id,
                "Mondial Relay returned 401, refreshing token and retrying once"
            );
            self.invalidate_request_verification_token().await;
            let retry_token = self
                .get_request_verification_token(parcel_id, brand)
                .await?;
            response = api::fetch_tracking(
                &self.client,
                parcel_id,
                opts.postcode.as_deref(),
                brand,
                &self.country,
                &retry_token,
            )
            .await;
        }

        response
    }
}

struct CdpTransport {
    country: String,
    browser: Arc<browser_cdp::SharedCdpBrowser>,
}

impl CdpTransport {
    fn new(country: String, browser: Arc<browser_cdp::SharedCdpBrowser>) -> Self {
        Self { country, browser }
    }
}

#[async_trait]
impl TrackingTransport for CdpTransport {
    fn mode(&self) -> MondialRelayMode {
        MondialRelayMode::Cdp
    }

    async fn fetch_tracking_response(
        &self,
        parcel_id: &str,
        opts: &TrackOptions,
        brand: &str,
    ) -> Result<models::MondialRelayResponse> {
        let body = self
            .browser
            .fetch_tracking_response_body(parcel_id, opts.postcode.as_deref(), brand, &self.country)
            .await?;
        api::parse_tracking_response(&body, opts.postcode.is_some())
    }
}
