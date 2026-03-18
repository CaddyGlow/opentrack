use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::config::Config;
use crate::tracking::{TrackOptions, TrackingInfo};
use crate::{Error, Result};

pub mod chronopost;
pub mod laposte;
pub mod mondial_relay;

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn detect(&self, parcel_id: &str) -> bool;
    /// Return the public web tracking URL for the given parcel ID.
    fn tracking_url(&self, parcel_id: &str, opts: &TrackOptions) -> String;
    async fn track(&self, parcel_id: &str, opts: &TrackOptions) -> Result<TrackingInfo>;
}

pub struct ProviderRegistry {
    providers: Vec<Box<dyn Provider>>,
    shared_cdp_browser: Arc<mondial_relay::browser_cdp::SharedCdpBrowser>,
}

impl ProviderRegistry {
    pub fn new(client: wreq::Client, config: &Config) -> Self {
        let browser_proxy_server = resolved_browser_proxy_server(config);
        let browser_proxy_bypass_list = proxy_bypass_list_from_env();
        let shared_cdp_browser = Arc::new(mondial_relay::browser_cdp::SharedCdpBrowser::new(
            mondial_relay::browser_cdp::CdpRuntimeConfig {
                cdp_endpoint: config.cdp.endpoint.clone(),
                show_browser: config.cdp.show_browser,
                timeout: Duration::from_secs(config.cdp.browser_timeout_secs.max(1)),
                proxy_url: browser_proxy_server.clone(),
                proxy_bypass_list: browser_proxy_bypass_list.clone(),
            },
        ));
        let providers: Vec<Box<dyn Provider>> = vec![
            Box::new(laposte::LaposteProvider::new(
                client.clone(),
                config.providers.laposte.lang.clone(),
            )),
            Box::new(chronopost::ChronopostProvider::new(
                client.clone(),
                config.providers.chronopost.lang.clone(),
            )),
            Box::new(mondial_relay::MondialRelayProvider::new(
                client,
                config.providers.mondial_relay.country.clone(),
                config.providers.mondial_relay.brand.clone(),
                config.providers.mondial_relay.mode,
                shared_cdp_browser.clone(),
            )),
        ];

        Self {
            providers,
            shared_cdp_browser,
        }
    }

    pub async fn shutdown(&self) {
        self.shared_cdp_browser.shutdown().await;
    }

    pub fn get_by_id(&self, provider_id: &str) -> Result<&dyn Provider> {
        self.providers
            .iter()
            .find(|provider| provider.id() == provider_id)
            .map(std::ops::Deref::deref)
            .ok_or_else(|| Error::ProviderNotFound(provider_id.to_string()))
    }

    pub fn tracking_url(&self, provider_id: &str, parcel_id: &str) -> Result<String> {
        let provider = self.get_by_id(provider_id)?;
        Ok(provider.tracking_url(parcel_id, &TrackOptions::default()))
    }

    pub fn auto_detect(&self, parcel_id: &str) -> Result<&dyn Provider> {
        let matches: Vec<&dyn Provider> = self
            .providers
            .iter()
            .filter(|provider| provider.detect(parcel_id))
            .map(std::ops::Deref::deref)
            .collect();

        match matches.as_slice() {
            [] => Err(Error::AutoDetectFailed {
                provider: "all".to_string(),
                id: parcel_id.to_string(),
            }),
            [single] => Ok(*single),
            _ => Err(Error::AutoDetectAmbiguous(parcel_id.to_string())),
        }
    }
}

pub fn build_http_client(config: &Config) -> Result<wreq::Client> {
    let mut builder = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Firefox136)
        .cookie_store(true)
        .connect_timeout(Duration::from_secs(30));

    if let Some(proxy_url) = resolved_http_proxy_url(config) {
        builder = builder.proxy(wreq::Proxy::all(&proxy_url)?);
    }

    Ok(builder.build()?)
}

fn configured_proxy_url(config: &Config) -> Option<String> {
    config
        .proxy
        .url
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(std::string::ToString::to_string)
}

fn env_proxy_value(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn resolved_http_proxy_url(config: &Config) -> Option<String> {
    configured_proxy_url(config).or_else(|| {
        env_proxy_value(&[
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ])
    })
}

fn resolved_browser_proxy_server(config: &Config) -> Option<String> {
    if let Some(proxy_url) = configured_proxy_url(config) {
        return Some(proxy_url);
    }

    let http = env_proxy_value(&["HTTP_PROXY", "http_proxy"]);
    let https = env_proxy_value(&["HTTPS_PROXY", "https_proxy"]);
    let all = env_proxy_value(&["ALL_PROXY", "all_proxy"]);

    let http = http.or_else(|| all.clone());
    let https = https.or(all);

    match (http, https) {
        (Some(http), Some(https)) if http == https => Some(http),
        (Some(http), Some(https)) => Some(format!("http={http};https={https}")),
        (Some(http), None) => Some(http),
        (None, Some(https)) => Some(https),
        (None, None) => None,
    }
}

fn proxy_bypass_list_from_env() -> Option<String> {
    env_proxy_value(&["NO_PROXY", "no_proxy"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laposte_detection_works() {
        assert!(laposte::LaposteProvider::detect_id("AB12345678901"));
        assert!(!laposte::LaposteProvider::detect_id("12345678"));
    }

    #[test]
    fn mondial_detection_works() {
        assert!(mondial_relay::MondialRelayProvider::detect_id("12345678"));
        assert!(!mondial_relay::MondialRelayProvider::detect_id(
            "AB12345678901"
        ));
    }

    #[test]
    fn chronopost_detection_works() {
        assert!(chronopost::ChronopostProvider::detect_id("XM002774533TS"));
        assert!(!chronopost::ChronopostProvider::detect_id("12345678"));
    }
}
