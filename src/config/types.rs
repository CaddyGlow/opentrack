use crate::notifications::event::NotificationTrigger;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub proxy: ProxyConfig,
    pub cdp: CdpConfig,
    pub providers: ProvidersConfig,
    pub notifications: Vec<NotificationConfig>,
    pub parcels: Vec<ParcelEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub output: OutputMode,
    pub cache_ttl: u64,
    pub watch_interval: u64,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            output: OutputMode::Pretty,
            cache_ttl: 300,
            watch_interval: 300,
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    #[default]
    Pretty,
    Json,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
pub struct ProxyConfig {
    pub url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CdpConfig {
    pub endpoint: Option<String>,
    pub show_browser: bool,
    pub browser_timeout_secs: u64,
}

impl Default for CdpConfig {
    fn default() -> Self {
        Self {
            endpoint: None,
            show_browser: false,
            browser_timeout_secs: 25,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
pub struct ProvidersConfig {
    pub mondial_relay: MondialRelayConfig,
    pub laposte: LaposteConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MondialRelayConfig {
    pub country: String,
    pub brand: String,
    pub mode: MondialRelayMode,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MondialRelayMode {
    Api,
    #[serde(alias = "cbp")]
    #[default]
    Cdp,
}

impl Default for MondialRelayConfig {
    fn default() -> Self {
        Self {
            country: "fr".to_string(),
            brand: "PP".to_string(),
            mode: MondialRelayMode::Cdp,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LaposteConfig {
    pub lang: String,
}

impl Default for LaposteConfig {
    fn default() -> Self {
        Self {
            lang: "fr".to_string(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ParcelEntry {
    pub id: String,
    pub provider: String,
    pub label: Option<String>,
    pub postcode: Option<String>,
    pub lang: Option<String>,
    pub notify: Option<bool>,
}

impl Default for ParcelEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            provider: String::new(),
            label: None,
            postcode: None,
            lang: None,
            notify: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotificationConfig {
    Desktop(DesktopConfig),
    Ntfy(NtfyConfig),
    Webhook(WebhookConfig),
    Command(CommandConfig),
    Matrix(MatrixConfig),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DesktopConfig {
    pub triggers: Vec<NotificationTrigger>,
    pub app_name: Option<String>,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            triggers: vec![NotificationTrigger::StatusChange],
            app_name: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct NtfyConfig {
    pub triggers: Vec<NotificationTrigger>,
    pub url: String,
    pub token: Option<String>,
    pub priority: Option<String>,
}

impl Default for NtfyConfig {
    fn default() -> Self {
        Self {
            triggers: vec![NotificationTrigger::StatusChange],
            url: String::new(),
            token: None,
            priority: Some("default".to_string()),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct WebhookConfig {
    pub triggers: Vec<NotificationTrigger>,
    pub url: String,
    pub headers: std::collections::BTreeMap<String, String>,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            triggers: vec![NotificationTrigger::Any],
            url: String::new(),
            headers: std::collections::BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CommandConfig {
    pub triggers: Vec<NotificationTrigger>,
    pub command: String,
}

impl Default for CommandConfig {
    fn default() -> Self {
        Self {
            triggers: vec![NotificationTrigger::Delivered],
            command: String::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MatrixConfig {
    pub triggers: Vec<NotificationTrigger>,
    pub homeserver: String,
    pub room_id: String,
    pub access_token: String,
}

impl Default for MatrixConfig {
    fn default() -> Self {
        Self {
            triggers: vec![NotificationTrigger::StatusChange],
            homeserver: String::new(),
            room_id: String::new(),
            access_token: String::new(),
        }
    }
}
