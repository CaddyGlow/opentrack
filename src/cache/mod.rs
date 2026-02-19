use chrono::{DateTime, Utc};

use crate::Result;
use crate::tracking::TrackingInfo;

pub mod store;

pub fn is_fresh(cached_at: DateTime<Utc>, ttl_seconds: u64, now: DateTime<Utc>) -> bool {
    if ttl_seconds == 0 {
        return false;
    }

    let age = now.signed_duration_since(cached_at);
    age.num_seconds() <= ttl_seconds as i64
}

pub async fn read_fresh(
    provider: &str,
    parcel_id: &str,
    ttl_seconds: u64,
) -> Result<Option<TrackingInfo>> {
    let Some(entry) = store::read(provider, parcel_id).await? else {
        return Ok(None);
    };

    if is_fresh(entry.cached_at, ttl_seconds, Utc::now()) {
        return Ok(Some(entry.info));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::*;

    #[test]
    fn cache_freshness_respects_ttl() {
        let now = Utc::now();
        let cached = now - Duration::seconds(10);
        assert!(is_fresh(cached, 60, now));
        assert!(!is_fresh(cached, 5, now));
        assert!(!is_fresh(cached, 0, now));
    }
}
