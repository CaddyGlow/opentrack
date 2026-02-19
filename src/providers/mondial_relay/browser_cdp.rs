use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::{Error, Result};

use super::token;

#[derive(Debug, Clone)]
pub struct CdpRuntimeConfig {
    pub cdp_endpoint: Option<String>,
    pub show_browser: bool,
    pub timeout: Duration,
    pub proxy_url: Option<String>,
    pub proxy_bypass_list: Option<String>,
}

pub struct SharedCdpBrowser {
    runtime: CdpRuntimeConfig,
    session: Mutex<Option<CdpBrowserSession>>,
}

impl SharedCdpBrowser {
    pub fn new(runtime: CdpRuntimeConfig) -> Self {
        Self {
            runtime,
            session: Mutex::new(None),
        }
    }

    pub async fn shutdown(&self) {
        let mut session_guard = self.session.lock().await;
        if let Some(session) = session_guard.take() {
            session.close().await;
        }
    }

    pub async fn fetch_tracking_response_body(
        &self,
        shipment: &str,
        postcode: Option<&str>,
        brand: &str,
        country: &str,
    ) -> Result<String> {
        let started = Instant::now();
        let runner =
            CdpTrackingRunner::new(shipment, postcode, brand, country, self.runtime.timeout);

        let mut session_guard = self.session.lock().await;
        if session_guard.is_none() {
            *session_guard = Some(CdpBrowserSession::connect_or_launch(&self.runtime).await?);
        }

        let (launched_locally, endpoint_for_logs) = session_guard
            .as_ref()
            .map(|session| (session.launched_locally, session.endpoint_for_logs.clone()))
            .expect("CDP session must exist after initialization");

        tracing::debug!(
            provider = "mondial-relay",
            shipment = %shipment,
            cdp_endpoint = %endpoint_for_logs,
            launched_locally,
            show_browser = self.runtime.show_browser,
            has_proxy = self.runtime.proxy_url.is_some(),
            has_proxy_bypass = self.runtime.proxy_bypass_list.is_some(),
            timeout_secs = self.runtime.timeout.as_secs(),
            page_url = %runner.page_url,
            api_url = %runner.api_url,
            "running Mondial Relay CDP request"
        );

        let outcome = {
            let session = session_guard
                .as_mut()
                .expect("CDP session must exist after initialization");
            tokio::time::timeout(self.runtime.timeout, runner.run(session)).await
        };

        let response = match outcome {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => {
                tracing::warn!(
                    provider = "mondial-relay",
                    shipment = %shipment,
                    error = %err,
                    "CDP request failed; resetting shared browser session"
                );
                if let Some(session) = session_guard.take() {
                    session.close().await;
                }
                return Err(err);
            }
            Err(_) => {
                tracing::warn!(
                    provider = "mondial-relay",
                    shipment = %shipment,
                    "CDP request timed out; resetting shared browser session"
                );
                if let Some(session) = session_guard.take() {
                    session.close().await;
                }
                return Err(provider_error(format!(
                    "CDP mode timed out after {}s while fetching Mondial Relay tracking",
                    self.runtime.timeout.as_secs()
                )));
            }
        };

        if !response.ok {
            let message = format!(
                "Mondial Relay CDP API returned HTTP {} with body: {}",
                response.status,
                preview_text(&response.body, 180)
            );
            return Err(Error::ProviderError {
                code: response.status as u32,
                message,
            });
        }

        tracing::debug!(
            provider = "mondial-relay",
            shipment = %shipment,
            elapsed_ms = started.elapsed().as_millis() as u64,
            body_len = response.body.len(),
            source = response.source.as_deref().unwrap_or("<unknown>"),
            response_url = response.url.as_deref().unwrap_or("<unknown>"),
            request_headers = ?response.request_headers,
            token_len = response.token_len,
            launched_locally,
            "finished Mondial Relay CDP request"
        );

        Ok(response.body)
    }
}

#[derive(Debug, Deserialize)]
struct BrowserTrackingResponse {
    status: u16,
    ok: bool,
    body: String,
    content_type: Option<String>,
    token_len: usize,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    request_headers: Option<BTreeMap<String, String>>,
}

struct CdpBrowserSession {
    browser: Browser,
    handler_task: JoinHandle<()>,
    endpoint_for_logs: String,
    launched_locally: bool,
    stealth_mode: bool,
}

impl CdpBrowserSession {
    async fn connect_or_launch(runtime: &CdpRuntimeConfig) -> Result<Self> {
        if let Some(endpoint) = runtime.cdp_endpoint.as_deref() {
            if runtime.proxy_url.is_some() {
                tracing::debug!(
                    provider = "mondial-relay",
                    cdp_endpoint = %endpoint,
                    "proxy.url is ignored when cdp_endpoint is set; configure proxy on the external browser process"
                );
            }
            let (browser, handler) =
                tokio::time::timeout(runtime.timeout, Browser::connect(endpoint))
                    .await
                    .map_err(|_| {
                        provider_error(format!(
                            "browser CDP connect timed out after {}s at {}",
                            runtime.timeout.as_secs(),
                            endpoint
                        ))
                    })?
                    .map_err(|err| {
                        provider_error(format!(
                            "failed to connect to browser CDP endpoint {endpoint}: {err}"
                        ))
                    })?;

            let handler_task = spawn_handler_task(handler);
            return Ok(Self {
                browser,
                handler_task,
                endpoint_for_logs: endpoint.to_string(),
                launched_locally: false,
                stealth_mode: false,
            });
        }

        let mut config_builder = BrowserConfig::builder()
            .no_sandbox()
            .launch_timeout(runtime.timeout)
            .request_timeout(runtime.timeout);
        let stealth_mode = !runtime.show_browser;
        config_builder = if runtime.show_browser {
            config_builder.with_head()
        } else {
            config_builder
                .new_headless_mode()
                .window_size(1366, 768)
                .arg("--disable-blink-features=AutomationControlled")
        };
        if let Some(proxy) = runtime
            .proxy_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            config_builder = config_builder.arg(format!("--proxy-server={proxy}"));
        }
        if let Some(bypass) = runtime
            .proxy_bypass_list
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            config_builder = config_builder.arg(format!("--proxy-bypass-list={bypass}"));
        }

        let config = config_builder.build().map_err(|err| {
            provider_error(format!(
                "failed to build browser config for CDP mode: {err}"
            ))
        })?;

        let (browser, handler) = tokio::time::timeout(runtime.timeout, Browser::launch(config))
            .await
            .map_err(|_| {
                provider_error(format!(
                    "launching local browser for CDP mode timed out after {}s",
                    runtime.timeout.as_secs()
                ))
            })?
            .map_err(|err| {
                provider_error(format!(
                    "failed to launch local browser for CDP mode: {err}"
                ))
            })?;

        let endpoint_for_logs = websocket_to_http_endpoint(browser.websocket_address())
            .unwrap_or_else(|| browser.websocket_address().clone());

        tracing::debug!(
            provider = "mondial-relay",
            cdp_endpoint = %endpoint_for_logs,
            show_browser = runtime.show_browser,
            has_proxy = runtime.proxy_url.is_some(),
            has_proxy_bypass = runtime.proxy_bypass_list.is_some(),
            "launched local browser for Mondial Relay CDP mode"
        );

        let handler_task = spawn_handler_task(handler);
        Ok(Self {
            browser,
            handler_task,
            endpoint_for_logs,
            launched_locally: true,
            stealth_mode,
        })
    }

    async fn close(mut self) {
        if self.launched_locally {
            match self.browser.close().await {
                Ok(_) => {
                    let _ = tokio::time::timeout(Duration::from_secs(3), self.browser.wait()).await;
                }
                Err(err) => {
                    tracing::warn!(
                        provider = "mondial-relay",
                        error = %err,
                        "failed to close locally-launched CDP browser"
                    );
                }
            }
        }

        self.handler_task.abort();
    }
}

struct CdpTrackingRunner<'a> {
    shipment: &'a str,
    page_url: String,
    api_url: String,
    timeout: Duration,
}

impl<'a> CdpTrackingRunner<'a> {
    fn new(
        shipment: &'a str,
        postcode: Option<&str>,
        brand: &str,
        country: &str,
        timeout: Duration,
    ) -> Self {
        Self {
            shipment,
            page_url: tracking_page_url(shipment, country, brand),
            api_url: tracking_api_url(shipment, postcode, brand, country),
            timeout,
        }
    }

    async fn run(&self, session: &mut CdpBrowserSession) -> Result<BrowserTrackingResponse> {
        let page = self.open_page_and_install_hooks(session).await?;
        self.navigate_and_wait_token(&page).await?;
        self.capture_or_fallback_fetch(&page).await
    }

    async fn open_page_and_install_hooks(&self, session: &mut CdpBrowserSession) -> Result<Page> {
        let page = session
            .browser
            .new_page("about:blank")
            .await
            .map_err(|err| {
                provider_error(format!("failed to open blank page in CDP mode: {err}"))
            })?;
        if session.stealth_mode {
            enable_headless_stealth_mode(&page).await?;
        }
        install_network_capture_hooks(&page).await?;
        Ok(page)
    }

    async fn navigate_and_wait_token(&self, page: &Page) -> Result<()> {
        page.goto(self.page_url.as_str()).await.map_err(|err| {
            provider_error(format!(
                "failed to navigate Mondial Relay page in CDP mode: {err}"
            ))
        })?;
        wait_for_request_verification_token(page, self.timeout).await
    }

    async fn capture_or_fallback_fetch(&self, page: &Page) -> Result<BrowserTrackingResponse> {
        if let Some(captured) =
            wait_for_tracking_response_from_page(page, self.shipment, self.timeout).await?
        {
            return Ok(captured);
        }

        tracing::warn!(
            provider = "mondial-relay",
            shipment = %self.shipment,
            "did not observe in-page tracking request, falling back to explicit browser fetch"
        );
        fetch_tracking_from_page(page, &self.api_url).await
    }
}

fn spawn_handler_task(mut handler: chromiumoxide::Handler) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if let Err(err) = event {
                tracing::debug!(
                    provider = "mondial-relay",
                    error = %err,
                    "browser CDP handler exited with error"
                );
                break;
            }
        }
    })
}

fn websocket_to_http_endpoint(websocket_url: &str) -> Option<String> {
    if let Some(rest) = websocket_url.strip_prefix("ws://") {
        let host = rest.split('/').next()?;
        return Some(format!("http://{host}"));
    }

    if let Some(rest) = websocket_url.strip_prefix("wss://") {
        let host = rest.split('/').next()?;
        return Some(format!("https://{host}"));
    }

    None
}

async fn enable_headless_stealth_mode(page: &Page) -> Result<()> {
    let user_agent = page.user_agent().await.map_err(|err| {
        provider_error(format!(
            "failed to read browser user-agent before stealth setup: {err}"
        ))
    })?;
    let stealth_user_agent = sanitize_headless_user_agent(&user_agent);

    page.enable_stealth_mode_with_agent(&stealth_user_agent)
        .await
        .map_err(|err| {
            provider_error(format!(
                "failed to enable headless stealth mode for Mondial Relay page: {err}"
            ))
        })?;

    Ok(())
}

fn sanitize_headless_user_agent(user_agent: &str) -> String {
    user_agent.replace("HeadlessChrome/", "Chrome/")
}

async fn wait_for_request_verification_token(page: &Page, timeout: Duration) -> Result<()> {
    let start = Instant::now();

    loop {
        let has_token: bool = page
            .evaluate(token::REQUEST_VERIFICATION_TOKEN_PRESENCE_JS)
            .await
            .map_err(|err| {
                provider_error(format!(
                    "failed to evaluate token presence on Mondial Relay page: {err}"
                ))
            })?
            .into_value()
            .map_err(|err| {
                provider_error(format!(
                    "failed to decode token presence evaluation result: {err}"
                ))
            })?;

        if has_token {
            return Ok(());
        }

        if start.elapsed() >= timeout {
            return Err(provider_error(format!(
                "timed out waiting for __RequestVerificationToken on Mondial Relay page after {}s",
                timeout.as_secs()
            )));
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn install_network_capture_hooks(page: &Page) -> Result<()> {
    let hook_script = r#"
(() => {
  if (window.__opentrackCaptureInstalled) return;
  window.__opentrackCaptureInstalled = true;
  window.__opentrackCaptured = [];

  const pushCapture = (entry) => {
    try {
      window.__opentrackCaptured.push(entry);
      if (window.__opentrackCaptured.length > 30) {
        window.__opentrackCaptured.shift();
      }
    } catch (_) {}
  };

  const origFetch = window.fetch.bind(window);
  window.fetch = async (...args) => {
    const req = args[0];
    const init = args[1] || {};
    const url = typeof req === 'string' ? req : (req && req.url) || '';
    const method = String(init.method || (req && req.method) || 'GET').toUpperCase();

    const headersObj = {};
    try {
      const hdrs = new Headers((req && req.headers) || init.headers || {});
      for (const [k, v] of hdrs.entries()) headersObj[String(k).toLowerCase()] = String(v);
    } catch (_) {}

    const response = await origFetch(...args);
    const body = await response.clone().text().catch(() => '');
    pushCapture({
      source: 'fetch',
      url,
      method,
      status: response.status,
      ok: response.ok,
      body,
      content_type: response.headers.get('content-type'),
      request_headers: headersObj
    });
    return response;
  };

  const xhrOpen = XMLHttpRequest.prototype.open;
  const xhrSetHeader = XMLHttpRequest.prototype.setRequestHeader;
  const xhrSend = XMLHttpRequest.prototype.send;

  XMLHttpRequest.prototype.open = function(method, url, ...rest) {
    this.__opentrackMethod = String(method || 'GET').toUpperCase();
    this.__opentrackUrl = String(url || '');
    this.__opentrackHeaders = {};
    return xhrOpen.call(this, method, url, ...rest);
  };

  XMLHttpRequest.prototype.setRequestHeader = function(name, value) {
    try {
      if (!this.__opentrackHeaders) this.__opentrackHeaders = {};
      this.__opentrackHeaders[String(name).toLowerCase()] = String(value);
    } catch (_) {}
    return xhrSetHeader.call(this, name, value);
  };

  XMLHttpRequest.prototype.send = function(...args) {
    this.addEventListener('loadend', () => {
      try {
        pushCapture({
          source: 'xhr',
          url: this.__opentrackUrl || '',
          method: this.__opentrackMethod || 'GET',
          status: this.status || 0,
          ok: this.status >= 200 && this.status < 300,
          body: typeof this.responseText === 'string' ? this.responseText : '',
          content_type: this.getResponseHeader('content-type'),
          request_headers: this.__opentrackHeaders || {}
        });
      } catch (_) {}
    });
    return xhrSend.apply(this, args);
  };
})();
"#;

    page.evaluate_on_new_document(hook_script)
        .await
        .map_err(|err| provider_error(format!("failed to install network capture hooks: {err}")))?;
    Ok(())
}

async fn wait_for_tracking_response_from_page(
    page: &Page,
    shipment: &str,
    timeout: Duration,
) -> Result<Option<BrowserTrackingResponse>> {
    let start = Instant::now();
    let shipment_json = serde_json::to_string(shipment).map_err(|err| {
        provider_error(format!(
            "failed to serialize shipment id for capture query: {err}"
        ))
    })?;
    let token_selector_json = serde_json::to_string(token::REQUEST_VERIFICATION_TOKEN_SELECTOR)
        .map_err(|err| {
            provider_error(format!(
                "failed to serialize token selector for capture query: {err}"
            ))
        })?;

    loop {
        let query_script = format!(
            "function() {{
  try {{
  const shipment = {shipment};
  const tokenSelector = {token_selector};
  const captures = window.__opentrackCaptured || [];
  const encoded = encodeURIComponent(shipment);
  for (let i = captures.length - 1; i >= 0; i--) {{
    const entry = captures[i] || {{}};
    const url = String(entry.url || '');
    if (url.indexOf('/api/tracking') === -1) continue;
    if (url.indexOf('shipment=' + shipment) === -1 && url.indexOf('shipment=' + encoded) === -1) continue;
    const tokenValue = document.querySelector(tokenSelector)?.value || '';
    const payload = {{
      status: Number(entry.status || 0),
      ok: Boolean(entry.ok),
      body: String(entry.body || ''),
      content_type: entry.content_type || null,
      token_len: tokenValue.length,
      source: entry.source || null,
      url,
      request_headers: entry.request_headers || {{}}
    }};
    return JSON.stringify(payload);
  }}
  return 'null';
  }} catch (err) {{
    return JSON.stringify({{ __opentrack_error: String(err) }});
  }}
}}",
            shipment = shipment_json,
            token_selector = token_selector_json,
        );

        let raw: String = page
            .evaluate_function(query_script)
            .await
            .map_err(|err| {
                provider_error(format!(
                    "failed to query captured page tracking request: {err}"
                ))
            })?
            .into_value()
            .map_err(|err| {
                provider_error(format!("failed to decode capture query result: {err}"))
            })?;

        if raw == "null" {
            if start.elapsed() >= timeout {
                return Ok(None);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }

        let value: serde_json::Value = serde_json::from_str(&raw).map_err(|err| {
            provider_error(format!(
                "failed to parse capture query json: {err}; raw={}",
                preview_text(&raw, 240)
            ))
        })?;

        if let Some(js_err) = value
            .get("__opentrack_error")
            .and_then(serde_json::Value::as_str)
        {
            tracing::debug!(
                provider = "mondial-relay",
                shipment = %shipment,
                error = %js_err,
                "capture query returned transient JS error, retrying"
            );
            if start.elapsed() >= timeout {
                return Ok(None);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }

        let parsed: BrowserTrackingResponse =
            serde_json::from_value(value.clone()).map_err(|err| {
                provider_error(format!(
                    "failed to decode captured in-page tracking response: {err}; payload={}",
                    preview_json(&value, 240)
                ))
            })?;
        return Ok(Some(parsed));
    }
}

async fn fetch_tracking_from_page(page: &Page, api_url: &str) -> Result<BrowserTrackingResponse> {
    let api_url_json = serde_json::to_string(api_url).map_err(|err| {
        provider_error(format!(
            "failed to serialize API URL for browser script: {err}"
        ))
    })?;
    let token_selector_json = serde_json::to_string(token::REQUEST_VERIFICATION_TOKEN_SELECTOR)
        .map_err(|err| {
            provider_error(format!(
                "failed to serialize token selector for browser script: {err}"
            ))
        })?;

    let script = format!(
        "async function() {{
  const apiUrl = {api_url_json};
  const tokenSelector = {token_selector};
  const tokenValue = document.querySelector(tokenSelector)?.value ?? '';
  const response = await fetch(apiUrl, {{
    method: 'GET',
    credentials: 'include',
    headers: {{
      'requestverificationtoken': tokenValue
    }}
  }});
  const body = await response.text();
  return {{
    status: response.status,
    ok: response.ok,
    body,
    content_type: response.headers.get('content-type'),
    token_len: tokenValue.length,
    source: 'direct-fetch',
    url: response.url,
    request_headers: {{ requestverificationtoken: tokenValue }}
  }};
}}",
        token_selector = token_selector_json,
    );

    let response_value: serde_json::Value = page
        .evaluate_function(script)
        .await
        .map_err(|err| {
            provider_error(format!(
                "failed to execute Mondial Relay browser fetch script: {err}"
            ))
        })?
        .into_value()
        .map_err(|err| {
            provider_error(format!(
                "failed to decode Mondial Relay browser fetch result: {err}"
            ))
        })?;

    let response: BrowserTrackingResponse = serde_json::from_value(response_value.clone())
        .map_err(|err| {
            provider_error(format!(
                "failed to decode Mondial Relay browser fetch payload: {err}; payload={}",
                preview_json(&response_value, 240)
            ))
        })?;

    tracing::debug!(
        provider = "mondial-relay",
        status = response.status,
        ok = response.ok,
        token_len = response.token_len,
        source = response.source.as_deref().unwrap_or("<unknown>"),
        response_url = response.url.as_deref().unwrap_or("<unknown>"),
        content_type = response.content_type.as_deref().unwrap_or("<missing>"),
        body_len = response.body.len(),
        "received Mondial Relay CDP API response"
    );

    Ok(response)
}

fn tracking_page_url(shipment: &str, country: &str, brand: &str) -> String {
    let country_upper = country.to_uppercase();
    format!(
        "https://www.mondialrelay.fr/suivi-de-colis?codeMarque={brand}&numeroExpedition={shipment}&pays={country_upper}&language=fr"
    )
}

fn tracking_api_url(shipment: &str, postcode: Option<&str>, brand: &str, country: &str) -> String {
    let postcode_q = postcode.unwrap_or_default();
    let country_q = country.to_lowercase();
    format!(
        "https://www.mondialrelay.fr/api/tracking?shipment={shipment}&postcode={postcode_q}&brand={brand}&codePays={country_q}"
    )
}

fn provider_error(message: String) -> Error {
    Error::ProviderError { code: 0, message }
}

fn preview_json(value: &serde_json::Value, max_chars: usize) -> String {
    let compact = value.to_string();
    let mut out = compact.chars().take(max_chars).collect::<String>();
    if compact.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn preview_text(input: &str, max_chars: usize) -> String {
    let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = compact.chars().take(max_chars).collect::<String>();
    if compact.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sanitize_headless_user_agent;

    #[test]
    fn sanitize_headless_user_agent_rewrites_headless_marker() {
        let input = "Mozilla/5.0 ... HeadlessChrome/133.0.0.0 Safari/537.36";
        let sanitized = sanitize_headless_user_agent(input);
        assert!(!sanitized.contains("HeadlessChrome/"));
        assert!(sanitized.contains("Chrome/133.0.0.0"));
    }

    #[test]
    fn sanitize_headless_user_agent_keeps_non_headless_ua() {
        let input = "Mozilla/5.0 ... Chrome/133.0.0.0 Safari/537.36";
        let sanitized = sanitize_headless_user_agent(input);
        assert_eq!(sanitized, input);
    }
}
