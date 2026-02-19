use std::path::PathBuf;

use tokio::process::Command;

use crate::{Error, Result};

pub mod types;

pub use types::*;

pub fn config_path() -> Result<PathBuf> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("opentrack")
        .map_err(|e| Error::Cache(format!("failed to create XDG directories: {e}")))?;
    Ok(xdg_dirs.get_config_file("config.toml"))
}

pub async fn load() -> Result<Config> {
    let path = config_path()?;
    if !tokio::fs::try_exists(&path).await? {
        return Ok(Config::default());
    }

    let content = tokio::fs::read_to_string(&path).await?;
    let cfg = toml::from_str::<Config>(&content)?;
    Ok(cfg)
}

pub async fn save(config: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let content = toml::to_string_pretty(config)?;
    tokio::fs::write(path, content).await?;
    Ok(())
}

pub async fn edit() -> Result<()> {
    let path = config_path()?;
    if !tokio::fs::try_exists(&path).await? {
        save(&Config::default()).await?;
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(editor).arg(path).status().await?;
    if !status.success() {
        return Err(Error::Cache(
            "editor exited with non-zero status".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_intervals() {
        let cfg = Config::default();
        assert_eq!(cfg.general.cache_ttl, 300);
        assert_eq!(cfg.general.watch_interval, 300);
    }

    #[test]
    fn mondial_relay_mode_defaults_are_stable() {
        let cfg = Config::default();
        assert_eq!(cfg.providers.mondial_relay.mode, MondialRelayMode::Cdp);
        assert_eq!(cfg.providers.mondial_relay.country, "fr");
        assert_eq!(cfg.providers.mondial_relay.brand, "PP");
    }

    #[test]
    fn cdp_defaults_are_stable() {
        let cfg = Config::default();
        assert_eq!(cfg.cdp.endpoint, None);
        assert!(!cfg.cdp.show_browser);
        assert_eq!(cfg.cdp.browser_timeout_secs, 25);
    }

    #[test]
    fn mondial_relay_mode_accepts_cbp_alias() {
        let cfg: Config = toml::from_str(
            r#"
            [providers.mondial_relay]
            mode = "cbp"
            "#,
        )
        .expect("valid config");
        assert_eq!(cfg.providers.mondial_relay.mode, MondialRelayMode::Cdp);
    }
}
