use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::tracking::TrackingInfo;
use crate::{Error, Result};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheEntry {
    pub cached_at: DateTime<Utc>,
    pub info: TrackingInfo,
}

fn cache_root() -> Result<PathBuf> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("opentrack")
        .map_err(|e| Error::Cache(format!("failed to create XDG directories: {e}")))?;
    Ok(xdg_dirs.get_cache_home())
}

fn entry_path(provider: &str, parcel_id: &str) -> Result<PathBuf> {
    Ok(cache_root()?
        .join(provider)
        .join(format!("{parcel_id}.json")))
}

pub async fn read(provider: &str, parcel_id: &str) -> Result<Option<CacheEntry>> {
    let path = entry_path(provider, parcel_id)?;
    if !tokio::fs::try_exists(&path).await? {
        return Ok(None);
    }

    let bytes = tokio::fs::read(path).await?;
    let entry = serde_json::from_slice::<CacheEntry>(&bytes)?;
    Ok(Some(entry))
}

pub async fn write(provider: &str, parcel_id: &str, info: &TrackingInfo) -> Result<()> {
    let path = entry_path(provider, parcel_id)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let entry = CacheEntry {
        cached_at: Utc::now(),
        info: info.clone(),
    };

    let content = serde_json::to_vec_pretty(&entry)?;
    tokio::fs::write(path, content).await?;
    Ok(())
}

pub async fn clear(provider: Option<&str>, parcel_id: Option<&str>) -> Result<u32> {
    let root = cache_root()?;
    if !tokio::fs::try_exists(&root).await? {
        return Ok(0);
    }

    match (provider, parcel_id) {
        (Some(p), Some(id)) => clear_single(&root, p, id).await,
        (Some(p), None) => clear_provider(&root, p).await,
        (None, Some(id)) => clear_id_glob(&root, id).await,
        (None, None) => clear_all(&root).await,
    }
}

async fn clear_single(root: &Path, provider: &str, parcel_id: &str) -> Result<u32> {
    let path = root.join(provider).join(format!("{parcel_id}.json"));
    if tokio::fs::try_exists(&path).await? {
        tokio::fs::remove_file(path).await?;
        return Ok(1);
    }
    Ok(0)
}

async fn clear_provider(root: &Path, provider: &str) -> Result<u32> {
    let path = root.join(provider);
    if !tokio::fs::try_exists(&path).await? {
        return Ok(0);
    }

    let count = count_json_files(&path).await?;
    tokio::fs::remove_dir_all(path).await?;
    Ok(count)
}

async fn clear_id_glob(root: &Path, parcel_id: &str) -> Result<u32> {
    let mut deleted = 0;
    let mut rd = tokio::fs::read_dir(root).await?;
    while let Some(entry) = rd.next_entry().await? {
        let provider_dir = entry.path();
        let metadata = entry.metadata().await?;
        if !metadata.is_dir() {
            continue;
        }

        let file = provider_dir.join(format!("{parcel_id}.json"));
        if tokio::fs::try_exists(&file).await? {
            tokio::fs::remove_file(file).await?;
            deleted += 1;
        }
    }

    Ok(deleted)
}

async fn clear_all(root: &Path) -> Result<u32> {
    let count = count_json_files(root).await?;
    tokio::fs::remove_dir_all(root).await?;
    Ok(count)
}

async fn count_json_files(root: &Path) -> Result<u32> {
    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        let mut rd = tokio::fs::read_dir(path).await?;
        while let Some(entry) = rd.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }

            if path.extension().and_then(std::ffi::OsStr::to_str) == Some("json") {
                count += 1;
            }
        }
    }

    Ok(count)
}
