//! Nexus Mods REST v1 + GraphQL v2 client.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_RANGE, RANGE, USER_AGENT};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use crate::config::{APP_NAME, APP_VERSION};
use crate::rate_limit::{
    backoff_with_jitter, is_retryable_status, parse_retry_after, sleep_interruptible, RateLimitGate,
};

pub mod requirements;
pub use requirements::{collect_missing, MissingRequirements, RequirementRoot, StagedNexusMod};

const REST_BASE: &str = "https://api.nexusmods.com/v1";
const GQL_URL: &str = "https://api.nexusmods.com/v2/graphql";

pub const CANCELLED_MSG: &str = "CANCELLED";
pub const PAUSED_MSG: &str = "PAUSED";
/// A transfer that ended before `Content-Length` bytes arrived. Callers must
/// treat the partial file as garbage rather than resume from it.
pub const INCOMPLETE_MSG: &str = "INCOMPLETE";
/// A transfer that was deliberately aborted because sustained throughput fell
/// below the minimum speed floor (e.g. 1 KB/s CDN throttling). Partial bytes
/// on disk are flushed and valid for `Range` resume on another mirror.
pub const THROTTLED_STALL_MSG: &str = "THROTTLED_STALL";

/// Initial grace period before enforcing the low-speed stall detector.
const STALL_GRACE_SECS: u64 = 15;
/// Rolling evaluation window for sustained low-speed detection.
const STALL_WINDOW_SECS: u64 = 20;
/// Minimum sustained throughput (16 KB/s) before aborting to switch mirrors.
const MIN_TRANSFER_SPEED_BPS: u64 = 16 * 1024;
/// Sliding window for live UI speed reporting.
const SPEED_REPORT_WINDOW_SECS: u64 = 3;

/// Cooperative cancel / pause flags for an in-flight HTTP transfer.
pub struct TransferControl {
    pub cancel: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct NexusClient {
    http: reqwest::Client,
    /// Separate client without the `apikey` header for CDN mirror downloads
    /// so personal API keys are never sent to third-party CDN hosts or redirects.
    cdn_http: reqwest::Client,
    /// Public mod pages sit behind a Cloudflare check that rejects the API
    /// client's rustls fingerprint. This client impersonates Chrome and does
    /// not carry the API key.
    page_http: wreq::Client,
    rate_limit: Arc<RateLimitGate>,
    is_anonymous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexusUser {
    pub user_id: u64,
    pub key: String,
    pub name: String,
    pub is_premium: bool,
    pub is_supporter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModSearchHit {
    pub mod_id: u64,
    pub name: String,
    pub summary: Option<String>,
    pub picture_url: Option<String>,
    pub downloads: Option<u64>,
    pub endorsements: Option<u64>,
    pub author: Option<String>,
    pub domain_name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameCategory {
    pub category_id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfo {
    pub id: u64,
    pub name: String,
    pub domain_name: String,
    pub genre: Option<String>,
    pub forum_url: Option<String>,
    pub nexusmods_url: Option<String>,
    pub mods: Option<u64>,
    pub file_count: Option<u64>,
    pub downloads: Option<u64>,
    #[serde(default)]
    pub categories: Vec<GameCategory>,
}

/// Lightweight game row from `GET /v1/games.json` (catalog matching).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexusGameEntry {
    pub id: u64,
    pub name: String,
    pub domain_name: String,
}

/// Nexus user hit from GraphQL `users` search (author autocomplete).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorSuggestion {
    pub name: String,
    pub member_id: Option<u64>,
    pub mod_count: Option<u64>,
    pub avatar: Option<String>,
}

/// One slide from the author gallery on the Nexus mod page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModGalleryImage {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModDetail {
    pub mod_id: u64,
    pub name: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub picture_url: Option<String>,
    /// Author gallery from the mod page carousel, in page order. Empty when
    /// that page could not be read.
    #[serde(default)]
    pub image_urls: Vec<ModGalleryImage>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub downloads: Option<u64>,
    pub endorsements: Option<u64>,
    pub created_timestamp: Option<u64>,
    pub updated_timestamp: Option<u64>,
    pub domain_name: String,
    #[serde(default)]
    pub category_id: Option<u64>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub uploaded_by: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub contains_adult_content: bool,
    #[serde(default)]
    pub uploaded_users_profile_url: Option<String>,
    /// `false` when Nexus has hidden, removed, or otherwise unpublished the mod.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    /// Hidden/removed notice from the public mod page, when that page says why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_note: Option<String>,
}

/// Author gallery and any hidden/removed notice from one public mod page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModPageRead {
    pub images: Vec<ModGalleryImage>,
    pub availability_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModFileInfo {
    pub file_id: u64,
    pub name: String,
    /// The uploaded archive's real name, e.g. `Cool Mod-123-1-0.7z`. `name` is
    /// the display title and says nothing about the payload's format.
    #[serde(default)]
    pub file_name: Option<String>,
    pub version: Option<String>,
    pub category_name: Option<String>,
    pub size_kb: Option<u64>,
    pub uploaded_timestamp: Option<u64>,
    pub is_primary: bool,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionHit {
    pub slug: String,
    pub name: String,
    pub summary: Option<String>,
    pub endorsements: Option<u64>,
    pub total_downloads: Option<u64>,
    pub domain_name: Option<String>,
    pub revision_number: Option<i64>,
    #[serde(default)]
    pub tile_image_url: Option<String>,
    pub author: Option<String>,
    pub category: Option<String>,
    pub overall_rating: Option<f64>,
    pub overall_rating_count: Option<u64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub mod_count: Option<u64>,
    pub file_size: Option<u64>,
    #[serde(default = "nexus_collection_source")]
    pub source: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub community: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub package_name: Option<String>,
    #[serde(default)]
    pub latest_version: Option<String>,
}

fn nexus_collection_source() -> String {
    "nexus".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionDetail {
    pub slug: String,
    pub name: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub endorsements: Option<u64>,
    pub total_downloads: Option<u64>,
    pub domain_name: Option<String>,
    pub revision_number: Option<i64>,
    #[serde(default)]
    pub tile_image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionModFile {
    pub file_id: u64,
    pub optional: bool,
    pub mod_id: u64,
    pub mod_name: String,
    pub file_name: String,
    pub version: Option<String>,
    pub domain_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseMeta {
    pub categories: Vec<String>,
    /// Searchable mod tags (`legacyTags` + mod facets).
    pub mod_tags: Vec<String>,
    /// Collection tags (`availableTags` / `specificTags` + collection facets).
    pub collection_tags: Vec<String>,
    pub game_versions: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BrowseSearchOpts {
    pub sort: String,
    pub category: Option<String>,
    pub tags_include: Vec<String>,
    pub tags_exclude: Vec<String>,
    pub game_version: Option<String>,
    /// Nexus uploader / Thunderstore owner account name (exact match).
    pub author: Option<String>,
    pub offset: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModSearchPage {
    pub items: Vec<ModSearchHit>,
    pub nodes_count: u64,
    pub total_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSearchPage {
    pub items: Vec<CollectionHit>,
    pub nodes_count: u64,
    pub total_count: u64,
}

const DEFAULT_MODS_PAGE_SIZE: u32 = 30;
const DEFAULT_COLLECTIONS_PAGE_SIZE: u32 = 25;

impl NexusClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::new_with_gate(api_key, Arc::new(RateLimitGate::new()))
    }

    pub fn new_with_gate(
        api_key: impl Into<String>,
        rate_limit: Arc<RateLimitGate>,
    ) -> Result<Self> {
        let api_key = api_key.into();
        let trimmed = api_key.trim();
        if trimmed.is_empty() {
            bail!("API key is empty");
        }
        Self::build_client(Some(trimmed), rate_limit)
    }

    /// Build an unauthenticated client for public Nexus GraphQL v2 browsing
    /// and public mod page fetches. REST v1 endpoints and downloads are disabled.
    pub fn new_anonymous(rate_limit: Arc<RateLimitGate>) -> Result<Self> {
        Self::build_client(None, rate_limit)
    }

    #[cfg(test)]
    pub fn is_anonymous(&self) -> bool {
        self.is_anonymous
    }

    fn build_client(api_key: Option<&str>, rate_limit: Arc<RateLimitGate>) -> Result<Self> {
        let mut base_headers = HeaderMap::new();
        base_headers.insert("Application-Name", HeaderValue::from_static(APP_NAME));
        base_headers.insert(
            "Application-Version",
            HeaderValue::from_str(APP_VERSION).unwrap_or(HeaderValue::from_static("0.1.0")),
        );
        base_headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&format!("{APP_NAME}/{APP_VERSION}"))
                .unwrap_or(HeaderValue::from_static("emperor-mod-manager/0.1.0")),
        );

        let mut api_headers = base_headers.clone();
        if let Some(key) = api_key {
            api_headers.insert(
                "apikey",
                HeaderValue::from_str(key).context("invalid API key header")?,
            );
        }

        let http = reqwest::Client::builder()
            .default_headers(api_headers)
            // A read timeout is per-chunk, so a stalled CDN connection surfaces
            // as an error rather than a silent short read. Deliberately not
            // `.timeout()`, which is a whole-request deadline and would abort
            // legitimate multi-hundred-megabyte downloads.
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()?;
        let cdn_http = reqwest::Client::builder()
            .default_headers(base_headers)
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()?;
        let page_http = wreq::Client::builder()
            .emulation(wreq_util::Emulation::Chrome136)
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()
            .context("chrome page client")?;
        Ok(Self {
            http,
            cdn_http,
            page_http,
            rate_limit,
            is_anonymous: api_key.is_none(),
        })
    }

    pub async fn validate(&self) -> Result<NexusUser> {
        if self.is_anonymous {
            bail!("AUTH_REQUIRED: Nexus API key is not set.");
        }
        let url = format!("{REST_BASE}/users/validate.json");
        let resp = self
            .send_with_retry(None, || self.http.get(&url))
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("validate failed ({status}): {body}");
        }
        let user: NexusUser = resp.json().await?;
        Ok(user)
    }

    fn check_rate_limit(&self, resp: &reqwest::Response) {
        self.rate_limit.observe_nexus_headers(resp.headers());
    }

    async fn send_with_retry<F>(
        &self,
        control: Option<&TransferControl>,
        build_req: F,
    ) -> Result<reqwest::Response>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        const MAX_ATTEMPTS: u32 = 4;
        let cancel = control.map(|c| c.cancel.as_ref());
        let pause = control.map(|c| c.pause.as_ref());
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..MAX_ATTEMPTS {
            if !self.rate_limit.wait_if_cooled_down(cancel, pause).await {
                if cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
                    bail!("{CANCELLED_MSG}");
                }
                bail!("{PAUSED_MSG}");
            }

            match build_req().send().await {
                Ok(resp) => {
                    self.check_rate_limit(&resp);
                    let status = resp.status();
                    if is_retryable_status(status) && attempt + 1 < MAX_ATTEMPTS {
                        let delay = parse_retry_after(resp.headers())
                            .unwrap_or_else(|| backoff_with_jitter(attempt, 1500, 30_000));
                        self.rate_limit
                            .record_throttle(delay, &format!("Nexus API HTTP {status}"));
                        if !sleep_interruptible(delay, cancel, pause).await {
                            if cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
                                bail!("{CANCELLED_MSG}");
                            }
                            bail!("{PAUSED_MSG}");
                        }
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    let is_transient = e.is_timeout() || e.is_connect() || e.is_request();
                    if is_transient && attempt + 1 < MAX_ATTEMPTS {
                        let delay = backoff_with_jitter(attempt, 1000, 15_000);
                        log::warn!(
                            "Nexus API request error (attempt {}/{}), retrying in {}ms: {e}",
                            attempt + 1,
                            MAX_ATTEMPTS,
                            delay.as_millis()
                        );
                        if !sleep_interruptible(delay, cancel, pause).await {
                            if cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
                                bail!("{CANCELLED_MSG}");
                            }
                            bail!("{PAUSED_MSG}");
                        }
                        last_err = Some(e.into());
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow!("Nexus API request failed after retries")))
    }

    pub async fn search_mods(
        &self,
        domain: &str,
        query: &str,
        adult_content: bool,
        opts: &BrowseSearchOpts,
    ) -> Result<ModSearchPage> {
        let sort_key = normalize_sort(&opts.sort);
        let count = if opts.count == 0 {
            DEFAULT_MODS_PAGE_SIZE
        } else {
            opts.count
        };
        let offset = opts.offset;

        // Unpaged REST trending shortcut only for the first empty trending page
        // when authenticated with an API key.
        if !self.is_anonymous
            && query.trim().is_empty()
            && sort_key == "trending"
            && !has_extra_filters(opts)
            && offset == 0
        {
            let items = self.trending_mods(domain).await?;
            let n = items.len() as u64;
            return Ok(ModSearchPage {
                items,
                nodes_count: n,
                total_count: n,
            });
        }

        let sort = mods_sort_json(&sort_key, !query.trim().is_empty());
        let filter = build_mods_filter(domain, query, adult_content, opts);

        let gql = r#"
        query SearchMods($filter: ModsFilter, $count: Int!, $offset: Int, $sort: [ModsSort!]) {
          mods(filter: $filter, count: $count, offset: $offset, sort: $sort) {
            nodesCount
            totalCount
            nodes {
              modId
              name
              summary
              pictureUrl
              downloads
              endorsements
              author
              category
              tags { name }
              game { domainName }
            }
          }
        }
        "#;

        let variables = json!({
            "filter": filter,
            "count": count,
            "offset": offset,
            "sort": [sort],
        });

        match self.graphql(gql, variables).await {
            Ok(data) => {
                let page = data.get("mods").cloned().unwrap_or(Value::Null);
                let nodes = page
                    .get("nodes")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let items: Vec<ModSearchHit> = nodes
                    .into_iter()
                    .filter_map(|n| parse_mod_hit(&n, domain))
                    .collect();
                let nodes_count = page
                    .get("nodesCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(items.len() as u64);
                let total_count = page
                    .get("totalCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(nodes_count);
                Ok(ModSearchPage {
                    items,
                    nodes_count,
                    total_count,
                })
            }
            Err(e) => {
                // REST fallback requires an API key and cannot honor offset / tag filters.
                if self.is_anonymous || offset > 0 || has_extra_filters(opts) {
                    return Err(e);
                }
                log::warn!("GraphQL search failed, falling back to REST: {e}");
                let items = if query.trim().is_empty() {
                    self.trending_mods(domain).await?
                } else {
                    self.search_mods_rest(domain, query).await?
                };
                let n = items.len() as u64;
                Ok(ModSearchPage {
                    items,
                    nodes_count: n,
                    total_count: n,
                })
            }
        }
    }

    async fn search_mods_rest(&self, domain: &str, query: &str) -> Result<Vec<ModSearchHit>> {
        let url = format!("{REST_BASE}/games/{domain}/mods/search.json");
        let resp = self
            .send_with_retry(None, || {
                self.http.get(&url).query(&[("search", query), ("page", "1")])
            })
            .await?;
        if !resp.status().is_success() {
            let trending = self.trending_mods(domain).await?;
            let q = query.to_lowercase();
            return Ok(trending
                .into_iter()
                .filter(|m| m.name.to_lowercase().contains(&q))
                .collect());
        }
        let data: Value = resp.json().await?;
        let arr = data
            .as_array()
            .cloned()
            .or_else(|| data.get("results").and_then(|v| v.as_array()).cloned())
            .unwrap_or_default();
        Ok(arr
            .into_iter()
            .filter_map(|n| {
                Some(ModSearchHit {
                    mod_id: n.get("mod_id").or_else(|| n.get("id"))?.as_u64()?,
                    name: n.get("name")?.as_str()?.to_string(),
                    summary: n
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    picture_url: n
                        .get("picture_url")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    downloads: n.get("mod_downloads").and_then(|v| v.as_u64()),
                    endorsements: n.get("mod_endorsements").and_then(|v| v.as_u64()),
                    author: n.get("author").and_then(|v| v.as_str()).map(str::to_string),
                    domain_name: domain.to_string(),
                    category: None,
                    tags: Vec::new(),
                })
            })
            .collect())
    }

    pub async fn trending_mods(&self, domain: &str) -> Result<Vec<ModSearchHit>> {
        let url = format!("{REST_BASE}/games/{domain}/mods/trending.json");
        let resp = self.send_with_retry(None, || self.http.get(&url)).await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("trending failed ({status}): {body}");
        }
        let arr: Vec<Value> = resp.json().await?;
        Ok(arr
            .into_iter()
            .filter_map(|n| {
                Some(ModSearchHit {
                    mod_id: n.get("mod_id")?.as_u64()?,
                    name: n.get("name")?.as_str()?.to_string(),
                    summary: n
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    picture_url: n
                        .get("picture_url")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    downloads: None,
                    endorsements: n.get("endorsement_count").and_then(|v| v.as_u64()),
                    author: n.get("author").and_then(|v| v.as_str()).map(str::to_string),
                    domain_name: domain.to_string(),
                    category: None,
                    tags: Vec::new(),
                })
            })
            .collect())
    }

    pub async fn list_games(&self) -> Result<Vec<NexusGameEntry>> {
        let url = format!("{REST_BASE}/games.json");
        let resp = self.send_with_retry(None, || self.http.get(&url)).await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("list games failed ({status}): {body}");
        }
        let n: Value = resp.json().await?;
        let arr = n
            .as_array()
            .ok_or_else(|| anyhow!("games.json: expected array"))?;
        Ok(arr
            .iter()
            .filter_map(|g| {
                let domain_name = g.get("domain_name")?.as_str()?.to_string();
                let name = g
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(domain_name.as_str())
                    .to_string();
                let id = g.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                Some(NexusGameEntry {
                    id,
                    name,
                    domain_name,
                })
            })
            .collect())
    }

    pub async fn get_game(&self, domain: &str) -> Result<GameInfo> {
        if self.is_anonymous {
            return self.get_game_graphql(domain).await;
        }
        let url = format!("{REST_BASE}/games/{domain}.json");
        let resp = self.send_with_retry(None, || self.http.get(&url)).await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("get game failed ({status}): {body}");
        }
        let n: Value = resp.json().await?;
        let domain_name = n
            .get("domain_name")
            .and_then(|v| v.as_str())
            .unwrap_or(domain)
            .to_string();
        let name = n
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(domain)
            .to_string();
        let id = n.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let categories = n
            .get("categories")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|c| {
                Some(GameCategory {
                    category_id: c.get("category_id")?.as_u64()?,
                    name: c.get("name")?.as_str()?.to_string(),
                })
            })
            .collect();
        Ok(GameInfo {
            id,
            name,
            domain_name,
            genre: n.get("genre").and_then(|v| v.as_str()).map(str::to_string),
            forum_url: n
                .get("forum_url")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            nexusmods_url: n
                .get("nexusmods_url")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            mods: n.get("mods").and_then(|v| v.as_u64()),
            file_count: n.get("file_count").and_then(|v| v.as_u64()),
            downloads: n.get("downloads").and_then(|v| v.as_u64()),
            categories,
        })
    }

    async fn get_game_graphql(&self, domain: &str) -> Result<GameInfo> {
        let gql = r#"
        query GameInfo($domainName: String!) {
          game(domainName: $domainName) {
            id
            name
            domainName
            genre
          }
        }
        "#;
        let data = self.graphql(gql, json!({ "domainName": domain })).await?;
        let g = data.get("game").cloned().unwrap_or(Value::Null);
        if g.is_null() {
            bail!("get game failed: game '{domain}' not found");
        }
        let id = g
            .get("id")
            .and_then(|v| v.as_u64())
            .or_else(|| g.get("id").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()))
            .unwrap_or(0);
        Ok(GameInfo {
            id,
            name: g
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(domain)
                .to_string(),
            domain_name: g
                .get("domainName")
                .and_then(|v| v.as_str())
                .unwrap_or(domain)
                .to_string(),
            genre: g.get("genre").and_then(|v| v.as_str()).map(str::to_string),
            forum_url: None,
            nexusmods_url: Some(format!("https://www.nexusmods.com/{domain}")),
            mods: None,
            file_count: None,
            downloads: None,
            categories: Vec::new(),
        })
    }

    pub async fn get_mod(&self, domain: &str, mod_id: u64) -> Result<ModDetail> {
        if self.is_anonymous {
            return self.get_mod_graphql(domain, mod_id).await;
        }
        let url = format!("{REST_BASE}/games/{domain}/mods/{mod_id}.json");
        let resp = self.send_with_retry(None, || self.http.get(&url)).await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        detail_from_mod_response(status, &body, domain, mod_id)
    }

    async fn get_mod_graphql(&self, domain: &str, mod_id: u64) -> Result<ModDetail> {
        let gql = r#"
        query ModDetail($filter: ModsFilter!) {
          mods(filter: $filter, count: 1) {
            nodes {
              modId
              name
              summary
              description
              pictureUrl
              author
              version
              downloads
              endorsements
              createdAt
              updatedAt
              category
              status
              adultContent
              uploader {
                name
                memberId
              }
              game {
                domainName
              }
            }
          }
        }
        "#;
        let filter = json!({
            "gameDomainName": [{ "value": domain, "op": "EQUALS" }],
            "modId": [{ "value": mod_id.to_string(), "op": "EQUALS" }],
        });
        let data = self.graphql(gql, json!({ "filter": filter })).await?;
        let node = data
            .pointer("/mods/nodes/0")
            .cloned()
            .unwrap_or(Value::Null);
        if node.is_null() {
            return Ok(unavailable_mod_detail(domain, mod_id));
        }
        let status = node
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let available = Some(!matches!(
            status.as_deref().map(str::to_ascii_lowercase).as_deref(),
            Some("removed" | "hidden" | "under_moderation" | "not_published" | "deleted")
        ));
        Ok(ModDetail {
            mod_id: node.get("modId").and_then(|v| v.as_u64()).unwrap_or(mod_id),
            name: node
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown mod")
                .to_string(),
            summary: node
                .get("summary")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            description: node
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            picture_url: node
                .get("pictureUrl")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            image_urls: Vec::new(),
            author: node
                .get("author")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            version: node
                .get("version")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            downloads: node.get("downloads").and_then(|v| v.as_u64()),
            endorsements: node.get("endorsements").and_then(|v| v.as_u64()),
            created_timestamp: None,
            updated_timestamp: None,
            domain_name: node
                .pointer("/game/domainName")
                .and_then(|v| v.as_str())
                .unwrap_or(domain)
                .to_string(),
            category_id: None,
            category: node
                .get("category")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            uploaded_by: node
                .pointer("/uploader/name")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            status,
            contains_adult_content: node
                .get("adultContent")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            uploaded_users_profile_url: None,
            available,
            availability_note: None,
        })
    }

    /// Author image carousel and hidden/removed notice from the public mod
    /// page. The REST mod payload only includes the primary `picture_url`.
    pub async fn mod_page_gallery(&self, domain: &str, mod_id: u64) -> Result<ModPageRead> {
        let domain = domain.trim();
        if domain.is_empty() || !domain.bytes().all(|b| b.is_ascii_alphanumeric()) {
            bail!("invalid nexus domain");
        }
        let url = format!("https://www.nexusmods.com/{domain}/mods/{mod_id}");
        let resp = self.page_http.get(&url).send().await?;
        if !resp.status().is_success() {
            bail!("mod page gallery failed ({})", resp.status());
        }
        let body = resp.text().await?;
        Ok(read_mod_page(&body))
    }

    pub async fn list_mod_files(&self, domain: &str, mod_id: u64) -> Result<Vec<ModFileInfo>> {
        if self.is_anonymous {
            return self.list_mod_files_graphql(domain, mod_id).await;
        }
        let url = format!("{REST_BASE}/games/{domain}/mods/{mod_id}/files.json");
        let resp = self.send_with_retry(None, || self.http.get(&url)).await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("list files failed ({status}): {body}");
        }
        let data: Value = resp.json().await?;
        let files = data
            .get("files")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out: Vec<ModFileInfo> = files
            .into_iter()
            .filter_map(|n| {
                Some(ModFileInfo {
                    file_id: n.get("file_id")?.as_u64()?,
                    name: n.get("name")?.as_str()?.to_string(),
                    file_name: n
                        .get("file_name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    version: n
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    category_name: n
                        .get("category_name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    size_kb: n.get("size_kb").and_then(|v| v.as_u64()),
                    uploaded_timestamp: n.get("uploaded_timestamp").and_then(|v| v.as_u64()),
                    is_primary: n
                        .get("is_primary")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    description: n
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                })
            })
            .collect();
        out.sort_by(|a, b| {
            b.uploaded_timestamp
                .cmp(&a.uploaded_timestamp)
                .then_with(|| b.file_id.cmp(&a.file_id))
        });
        Ok(out)
    }

    async fn list_mod_files_graphql(&self, domain: &str, mod_id: u64) -> Result<Vec<ModFileInfo>> {
        let game = self.get_game_graphql(domain).await?;
        let gql = r#"
        query ModFiles($modId: ID!, $gameId: ID!) {
          modFiles(modId: $modId, gameId: $gameId) {
            fileId
            name
            uri
            version
            category
            size
            date
            primary
            description
          }
        }
        "#;
        let data = self
            .graphql(
                gql,
                json!({
                    "modId": mod_id.to_string(),
                    "gameId": game.id.to_string(),
                }),
            )
            .await?;
        let files = data
            .get("modFiles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out: Vec<ModFileInfo> = files
            .into_iter()
            .filter_map(|n| {
                let file_id = n.get("fileId")?.as_u64()?;
                let name = n.get("name")?.as_str()?.to_string();
                let size_kb = n.get("size").and_then(|v| v.as_u64());
                let uploaded_timestamp = n.get("date").and_then(|v| v.as_u64());
                Some(ModFileInfo {
                    file_id,
                    name,
                    file_name: n.get("uri").and_then(|v| v.as_str()).map(str::to_string),
                    version: n
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    category_name: n
                        .get("category")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    size_kb,
                    uploaded_timestamp,
                    is_primary: n.get("primary").and_then(|v| v.as_bool()).unwrap_or(false),
                    description: n
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                })
            })
            .collect();
        out.sort_by(|a, b| {
            b.uploaded_timestamp
                .cmp(&a.uploaded_timestamp)
                .then_with(|| b.file_id.cmp(&a.file_id))
        });
        Ok(out)
    }

    async fn fetch_download_mirrors(
        &self,
        domain: &str,
        mod_id: u64,
        file_id: u64,
        nxm_key: Option<&str>,
        nxm_expires: Option<u64>,
        control: Option<&TransferControl>,
    ) -> Result<Vec<String>> {
        let url =
            format!("{REST_BASE}/games/{domain}/mods/{mod_id}/files/{file_id}/download_link.json");
        let expires_str = nxm_expires.map(|e| e.to_string());
        let resp = self
            .send_with_retry(control, || {
                let mut req = self.http.get(&url);
                if let (Some(key), Some(exp)) = (nxm_key, expires_str.as_deref()) {
                    req = req.query(&[("key", key), ("expires", exp)]);
                }
                req
            })
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("download_link failed ({status}): {body}");
        }
        let links: Value = resp.json().await?;
        let urls: Vec<String> = links
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.get("URI").and_then(|u| u.as_str()))
            .filter(|u| !u.trim().is_empty())
            .map(str::to_string)
            .collect();
        if urls.is_empty() {
            bail!("no download URI in response");
        }
        Ok(urls)
    }

    pub async fn download_file(
        &self,
        domain: &str,
        mod_id: u64,
        file_id: u64,
        dest: &Path,
        is_premium: bool,
        nxm_key: Option<&str>,
        nxm_expires: Option<u64>,
        start_offset: u64,
        control: Option<&TransferControl>,
        on_progress: Option<&Arc<dyn Fn(u64, Option<u64>, u64) + Send + Sync>>,
    ) -> Result<Option<String>> {
        let has_nxm = nxm_key.map(|k| !k.is_empty()).unwrap_or(false) && nxm_expires.is_some();
        if !is_premium && !has_nxm {
            bail!(
                "NEEDS_NXM: Free accounts need a site-issued download key. \
                 Use Download Assist (Mod Manager Download) or import a local archive. \
                 Premium unlocks one-click API downloads."
            );
        }

        let cancel = control.map(|c| c.cancel.as_ref());
        let pause = control.map(|c| c.pause.as_ref());
        const MAX_ROUNDS: u32 = 2;
        let mut last_err: Option<anyhow::Error> = None;

        for round in 0..MAX_ROUNDS {
            let mirrors = self
                .fetch_download_mirrors(domain, mod_id, file_id, nxm_key, nxm_expires, control)
                .await?;
            let total_mirrors = mirrors.len();

            for (idx, download_url) in mirrors.into_iter().enumerate() {
                if !self.rate_limit.wait_if_cooled_down(cancel, pause).await {
                    if cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
                        bail!("{CANCELLED_MSG}");
                    }
                    bail!("{PAUSED_MSG}");
                }

                let offset = if round == 0 && idx == 0 {
                    start_offset
                } else {
                    tokio::fs::metadata(dest).await.map(|m| m.len()).unwrap_or(0)
                };

                match stream_url_to_file(
                    &self.cdn_http,
                    &download_url,
                    dest,
                    offset,
                    None,
                    control,
                    on_progress,
                )
                .await
                {
                    Ok(()) => {
                        self.rate_limit.record_healthy();
                        return Ok(filename_from_url(&download_url));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains(CANCELLED_MSG) || msg.contains(PAUSED_MSG) {
                            return Err(e);
                        }
                        if is_retryable_transfer_err(&msg) {
                            let delay = backoff_with_jitter(idx as u32 + round, 1500, 20_000);
                            self.rate_limit.record_throttle(
                                delay,
                                &format!("CDN mirror {}/{total_mirrors} failed: {msg}", idx + 1),
                            );
                            last_err = Some(e);
                            if !sleep_interruptible(delay, cancel, pause).await {
                                if cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
                                    bail!("{CANCELLED_MSG}");
                                }
                                bail!("{PAUSED_MSG}");
                            }
                            continue;
                        }
                        // Non-retryable (e.g. 404/403 expired); re-fetch mirrors once on round 0.
                        last_err = Some(e);
                        break;
                    }
                }
            }

            if round + 1 < MAX_ROUNDS {
                let delay = backoff_with_jitter(round + 1, 3000, 30_000);
                if !sleep_interruptible(delay, cancel, pause).await {
                    if cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false) {
                        bail!("{CANCELLED_MSG}");
                    }
                    bail!("{PAUSED_MSG}");
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow!("all CDN mirrors failed")))
    }
}

/// The archive's real name from a CDN link. Nexus appends `?md5=...&expires=...`,
/// so this parses the URL instead of slicing the raw string.
pub fn filename_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.next_back()?;
    let decoded = percent_decode(last);
    let name = sanitize_filename::sanitize(decoded.trim());
    if name.is_empty() || !name.contains('.') {
        return None;
    }
    Some(name)
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&raw[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Rolling-window low-speed stall policy for [`stream_url_to_file_with_policy`].
#[derive(Debug, Clone, Copy)]
pub struct StallPolicy {
    pub grace: Duration,
    pub window: Duration,
    pub min_bps: u64,
}

impl Default for StallPolicy {
    fn default() -> Self {
        Self {
            grace: Duration::from_secs(STALL_GRACE_SECS),
            window: Duration::from_secs(STALL_WINDOW_SECS),
            min_bps: MIN_TRANSFER_SPEED_BPS,
        }
    }
}

/// Returns `true` when a transfer error is transient/throttled and worth
/// retrying or switching CDN mirrors (keeping partial bytes on disk).
pub fn is_retryable_transfer_err(msg: &str) -> bool {
    if msg.contains(CANCELLED_MSG) || msg.contains(PAUSED_MSG) {
        return false;
    }
    msg.contains(THROTTLED_STALL_MSG)
        || msg.contains(INCOMPLETE_MSG)
        || msg.contains("(429")
        || msg.contains("(408")
        || msg.contains("(502")
        || msg.contains("(503")
        || msg.contains("(504")
        || msg.contains("timed out")
        || msg.contains("connection reset")
        || msg.contains("error decoding response body")
        || msg.contains("error reading a body from connection")
}

/// Stream an HTTP URL to `dest`, optionally resuming with a Range request.
pub async fn stream_url_to_file(
    http: &reqwest::Client,
    download_url: &str,
    dest: &Path,
    start_offset: u64,
    extra_headers: Option<&HeaderMap>,
    control: Option<&TransferControl>,
    on_progress: Option<&Arc<dyn Fn(u64, Option<u64>, u64) + Send + Sync>>,
) -> Result<()> {
    stream_url_to_file_with_policy(
        http,
        download_url,
        dest,
        start_offset,
        extra_headers,
        control,
        on_progress,
        StallPolicy::default(),
    )
    .await
}

/// Stream an HTTP URL to `dest` with a configurable low-speed [`StallPolicy`].
#[allow(clippy::too_many_arguments)]
pub async fn stream_url_to_file_with_policy(
    http: &reqwest::Client,
    download_url: &str,
    dest: &Path,
    start_offset: u64,
    extra_headers: Option<&HeaderMap>,
    control: Option<&TransferControl>,
    on_progress: Option<&Arc<dyn Fn(u64, Option<u64>, u64) + Send + Sync>>,
    stall_policy: StallPolicy,
) -> Result<()> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut offset = start_offset;
    let resp = loop {
        let mut req = http.get(download_url);
        if let Some(headers) = extra_headers {
            req = req.headers(headers.clone());
        }
        if offset > 0 {
            req = req.header(RANGE, format!("bytes={offset}-"));
        }

        let resp = req.send().await?;
        let status = resp.status();

        if offset > 0 && status == StatusCode::OK {
            // Server ignored Range — restart from scratch.
            offset = 0;
            break resp;
        } else if offset > 0 && status == StatusCode::PARTIAL_CONTENT {
            // Resume from offset.
            break resp;
        } else if offset > 0 && status == StatusCode::RANGE_NOT_SATISFIABLE {
            if let Some(total) = parse_content_range_total(resp.headers().get(CONTENT_RANGE)) {
                if total > 0 && offset == total {
                    if let Some(cb) = on_progress {
                        cb(offset, Some(total), 0);
                    }
                    return Ok(());
                }
            }
            // Partial file is oversized/stale or server rejected the range;
            // restart from scratch without a Range header.
            offset = 0;
            continue;
        } else if offset > 0
            && (status == StatusCode::FORBIDDEN
                || status == StatusCode::UNAUTHORIZED
                || status == StatusCode::GONE
                || status == StatusCode::NOT_FOUND)
        {
            let body = resp.text().await.unwrap_or_default();
            bail!("resume failed ({status}): link expired or unavailable. {body}");
        } else if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("download failed ({status}): {body}");
        } else {
            break resp;
        }
    };
    let status = resp.status();

    let content_len = resp.content_length();
    let total = if status == StatusCode::PARTIAL_CONTENT {
        parse_content_range_total(resp.headers().get(CONTENT_RANGE))
            .or_else(|| content_len.map(|n| n + offset))
    } else {
        content_len
    };

    let mut file = if offset > 0 {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(dest)
            .await?
    } else {
        tokio::fs::File::create(dest).await?
    };

    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = offset;
    let started = Instant::now();
    let mut last_report = started;
    let session_start = offset;
    let mut speed_samples: VecDeque<(Instant, u64)> = VecDeque::new();
    let mut stall_samples: VecDeque<(Instant, u64)> = VecDeque::new();
    speed_samples.push_back((started, downloaded));
    stall_samples.push_back((started, downloaded));

    while let Some(chunk) = stream.next().await {
        if control
            .map(|c| c.cancel.load(Ordering::SeqCst))
            .unwrap_or(false)
        {
            drop(file);
            let _ = tokio::fs::remove_file(dest).await;
            bail!("{CANCELLED_MSG}");
        }
        if control
            .map(|c| c.pause.load(Ordering::SeqCst))
            .unwrap_or(false)
        {
            file.flush().await?;
            drop(file);
            if let Some(cb) = on_progress {
                cb(downloaded, total, 0);
            }
            bail!("{PAUSED_MSG}");
        }
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let _ = file.flush().await;
                return Err(e.into());
            }
        };
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        let now = Instant::now();

        speed_samples.push_back((now, downloaded));
        let speed_cutoff = now.checked_sub(Duration::from_secs(SPEED_REPORT_WINDOW_SECS));
        while speed_samples.len() > 2 {
            let Some((front_t, _)) = speed_samples.get(1).copied() else {
                break;
            };
            if speed_cutoff.map(|c| front_t <= c).unwrap_or(false) {
                speed_samples.pop_front();
            } else {
                break;
            }
        }

        stall_samples.push_back((now, downloaded));
        let stall_cutoff = now.checked_sub(stall_policy.window);
        while stall_samples.len() > 2 {
            let Some((front_t, _)) = stall_samples.get(1).copied() else {
                break;
            };
            if stall_cutoff.map(|c| front_t <= c).unwrap_or(false) {
                stall_samples.pop_front();
            } else {
                break;
            }
        }

        if stall_policy.min_bps > 0 && now.duration_since(started) >= stall_policy.grace {
            if let Some((oldest_t, oldest_bytes)) = stall_samples.front().copied() {
                let window_elapsed = now.duration_since(oldest_t);
                if window_elapsed >= stall_policy.window {
                    let window_bytes = downloaded.saturating_sub(oldest_bytes);
                    let window_bps =
                        (window_bytes as f64 / window_elapsed.as_secs_f64().max(0.001)) as u64;
                    if window_bps < stall_policy.min_bps {
                        file.flush().await?;
                        drop(file);
                        bail!(
                            "{THROTTLED_STALL_MSG}: sustained {window_bps} B/s (< {} B/s)",
                            stall_policy.min_bps
                        );
                    }
                }
            }
        }

        if now.duration_since(last_report).as_millis() >= 150
            || downloaded == total.unwrap_or(u64::MAX)
        {
            let speed = window_speed(&speed_samples, now, downloaded, session_start, started);
            if let Some(cb) = on_progress {
                cb(downloaded, total, speed);
            }
            last_report = now;
        }
    }
    file.flush().await?;
    // A dropped connection ends the stream without an error, so the byte count
    // is the only thing that separates a finished download from a truncated
    // one. Without this an incomplete archive reaches the extractor and fails
    // there, where the cause is no longer visible.
    verify_complete(downloaded, total)?;
    if let Some(cb) = on_progress {
        let now = Instant::now();
        let speed = window_speed(&speed_samples, now, downloaded, session_start, started);
        cb(downloaded, total.or(Some(downloaded)), speed);
    }
    Ok(())
}

fn window_speed(
    samples: &VecDeque<(Instant, u64)>,
    now: Instant,
    downloaded: u64,
    session_start: u64,
    started: Instant,
) -> u64 {
    if let Some((oldest_t, oldest_bytes)) = samples.front().copied() {
        let dt = now.duration_since(oldest_t).as_secs_f64();
        if dt >= 0.25 {
            return (downloaded.saturating_sub(oldest_bytes) as f64 / dt) as u64;
        }
    }
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    ((downloaded - session_start) as f64 / elapsed) as u64
}

/// `total` is `None` when the server sent no length, which leaves nothing to
/// check against. Only a short read is an error; a server over-reporting its
/// own length is not worth failing a usable download over.
fn verify_complete(downloaded: u64, total: Option<u64>) -> Result<()> {
    if let Some(expected) = total {
        if downloaded < expected {
            bail!("{INCOMPLETE_MSG}: got {downloaded} of {expected} bytes");
        }
    }
    Ok(())
}

fn parse_content_range_total(header: Option<&HeaderValue>) -> Option<u64> {
    let s = header?.to_str().ok()?;
    // e.g. "bytes 1000-1999/5000" or "bytes 1000-1999/*"
    let total = s.split('/').nth(1)?.trim();
    if total == "*" {
        return None;
    }
    total.parse().ok()
}

impl NexusClient {
    pub async fn search_collections(
        &self,
        domain: &str,
        query: &str,
        adult_content: bool,
        opts: &BrowseSearchOpts,
    ) -> Result<CollectionSearchPage> {
        let sort_key = normalize_sort(&opts.sort);
        let sort = collections_sort_json(&sort_key, !query.trim().is_empty());
        let filters = build_collections_filter(domain, query, adult_content, opts);
        let count = if opts.count == 0 {
            DEFAULT_COLLECTIONS_PAGE_SIZE
        } else {
            opts.count
        };
        let offset = opts.offset;

        let gql = r#"
        query SearchCollections($filters: CollectionsSearchFilter, $count: Int, $offset: Int, $sort: [CollectionsSearchSort!]) {
          collectionsV2(filter: $filters, count: $count, offset: $offset, sort: $sort) {
            nodesCount
            totalCount
            nodes {
              slug
              name
              summary
              endorsements
              totalDownloads
              overallRating
              overallRatingCount
              firstPublishedAt
              updatedAt
              category { name }
              user { name }
              game { domainName }
              latestPublishedRevision {
                revisionNumber
                modCount
                fileSize
                updatedAt
              }
              tileImage {
                url
                thumbnailUrl(size: med)
              }
            }
          }
        }
        "#;

        let data = self
            .graphql(
                gql,
                json!({
                    "filters": filters,
                    "count": count,
                    "offset": offset,
                    "sort": [sort],
                }),
            )
            .await?;

        let page = data.get("collectionsV2").cloned().unwrap_or(Value::Null);
        let nodes = page
            .get("nodes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let items: Vec<CollectionHit> = nodes
            .into_iter()
            .filter_map(|n| {
                let tile = n.get("tileImage");
                let tile_image_url = tile
                    .and_then(|t| t.get("thumbnailUrl").and_then(|v| v.as_str()))
                    .or_else(|| tile.and_then(|t| t.get("url").and_then(|v| v.as_str())))
                    .map(str::to_string);
                let updated_at = n
                    .get("updatedAt")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        n.pointer("/latestPublishedRevision/updatedAt")
                            .and_then(|v| v.as_str())
                    })
                    .map(str::to_string);
                let overall_rating =
                    n.get("overallRating").and_then(|v| v.as_f64()).or_else(|| {
                        n.get("overallRating")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok())
                    });
                Some(CollectionHit {
                    slug: n.get("slug")?.as_str()?.to_string(),
                    name: n.get("name")?.as_str()?.to_string(),
                    summary: n
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    endorsements: n.get("endorsements").and_then(|v| v.as_u64()),
                    total_downloads: n.get("totalDownloads").and_then(|v| v.as_u64()),
                    domain_name: n
                        .pointer("/game/domainName")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    revision_number: n
                        .pointer("/latestPublishedRevision/revisionNumber")
                        .and_then(|v| v.as_i64()),
                    tile_image_url,
                    author: n
                        .pointer("/user/name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    category: n
                        .pointer("/category/name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    overall_rating,
                    overall_rating_count: n.get("overallRatingCount").and_then(|v| v.as_u64()),
                    created_at: n
                        .get("firstPublishedAt")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    updated_at,
                    mod_count: n
                        .pointer("/latestPublishedRevision/modCount")
                        .and_then(|v| {
                            v.as_u64()
                                .or_else(|| v.as_i64().map(|i| i as u64))
                                .or_else(|| v.as_f64().map(|f| f as u64))
                        }),
                    file_size: n
                        .pointer("/latestPublishedRevision/fileSize")
                        .and_then(|v| {
                            v.as_u64()
                                .or_else(|| v.as_i64().map(|i| i as u64))
                                .or_else(|| v.as_f64().map(|f| f as u64))
                        }),
                    source: "nexus".into(),
                    id: format!(
                        "nexus:{}:{}",
                        n.pointer("/game/domainName")
                            .and_then(|v| v.as_str())
                            .unwrap_or(domain),
                        n.get("slug")?.as_str()?
                    ),
                    community: None,
                    namespace: None,
                    package_name: None,
                    latest_version: None,
                })
            })
            .collect();

        let nodes_count = page
            .get("nodesCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(items.len() as u64);
        let total_count = page
            .get("totalCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(nodes_count);

        Ok(CollectionSearchPage {
            items,
            nodes_count,
            total_count,
        })
    }

    pub async fn browse_meta(&self, domain: &str, adult_content: bool) -> Result<BrowseMeta> {
        let game_gql = r#"
        query GameMeta($domainName: String!) {
          game(domainName: $domainName) {
            id
            availableTags { name }
            specificTags { name }
          }
        }
        "#;
        let game_data = self
            .graphql(game_gql, json!({ "domainName": domain }))
            .await?;
        let game = game_data.get("game").cloned().unwrap_or(Value::Null);
        let game_id = game.get("id").and_then(|v| v.as_i64()).or_else(|| {
            game.get("id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        });

        let mut collection_tags: Vec<String> = Vec::new();
        for key in ["availableTags", "specificTags"] {
            if let Some(arr) = game.get(key).and_then(|v| v.as_array()) {
                for t in arr {
                    if let Some(name) = t.get("name").and_then(|v| v.as_str()) {
                        push_unique(&mut collection_tags, name);
                    }
                }
            }
        }

        let mut mod_tags: Vec<String> = Vec::new();
        if let Some(gid) = game_id {
            let legacy_gql = r#"
            query LegacyTags($gameId: ID, $excludeAdult: Boolean) {
              legacyTags(gameId: $gameId, excludeAdult: $excludeAdult) {
                name
                searchable
              }
            }
            "#;
            let exclude_adult = if adult_content {
                Value::Null
            } else {
                json!(true)
            };
            match self
                .graphql(
                    legacy_gql,
                    json!({ "gameId": gid.to_string(), "excludeAdult": exclude_adult }),
                )
                .await
            {
                Ok(data) => {
                    if let Some(arr) = data.get("legacyTags").and_then(|v| v.as_array()) {
                        for t in arr {
                            let searchable = t
                                .get("searchable")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if !searchable {
                                continue;
                            }
                            if let Some(name) = t.get("name").and_then(|v| v.as_str()) {
                                push_unique(&mut mod_tags, name);
                            }
                        }
                    }
                }
                Err(e) => log::warn!("legacyTags query failed: {e}"),
            }
        }

        let mut categories = Vec::new();
        if let Some(gid) = game_id {
            let cat_gql = r#"
            query Cats($gameId: Int) {
              categories(gameId: $gameId, global: false) {
                name
              }
            }
            "#;
            match self.graphql(cat_gql, json!({ "gameId": gid })).await {
                Ok(data) => {
                    if let Some(arr) = data.get("categories").and_then(|v| v.as_array()) {
                        for c in arr {
                            if let Some(name) = c.get("name").and_then(|v| v.as_str()) {
                                push_unique(&mut categories, name);
                            }
                        }
                    }
                }
                Err(e) => log::warn!("categories query failed: {e}"),
            }
        }

        let mut game_versions = Vec::new();

        let collection_facet_gql = r#"
        query CollectionFacets($filters: CollectionsSearchFilter, $count: Int) {
          collectionsV2(
            filter: $filters
            count: $count
            facets: { gameVersion: [], tag: [], categoryName: [] }
          ) {
            facetsData
            nodesFacets {
              facet
              value
              count
            }
          }
        }
        "#;
        let mut collection_filters = json!({
            "gameDomain": [{ "value": domain, "op": "EQUALS" }],
        });
        apply_adult_content_filter(&mut collection_filters, adult_content, false);
        match self
            .graphql(
                collection_facet_gql,
                json!({ "filters": collection_filters, "count": 1 }),
            )
            .await
        {
            Ok(data) => {
                merge_facet_bucket(
                    &data,
                    "/collectionsV2/nodesFacets",
                    "/collectionsV2/facetsData",
                    &["gameVersion", "game_version"],
                    &mut game_versions,
                );
                merge_facet_bucket(
                    &data,
                    "/collectionsV2/nodesFacets",
                    "/collectionsV2/facetsData",
                    &["tag"],
                    &mut collection_tags,
                );
                merge_facet_bucket(
                    &data,
                    "/collectionsV2/nodesFacets",
                    "/collectionsV2/facetsData",
                    &["categoryName", "category_name"],
                    &mut categories,
                );
            }
            Err(e) => log::warn!("collection facets failed: {e}"),
        }

        let mods_facet_gql = r#"
        query ModFacets($filter: ModsFilter, $count: Int!) {
          mods(
            filter: $filter
            count: $count
            facets: { tag: [], categoryName: [] }
          ) {
            facetsData
            nodesFacets {
              facet
              value
              count
            }
          }
        }
        "#;
        let mut mods_filter = json!({
            "gameDomainName": [{ "value": domain, "op": "EQUALS" }],
        });
        apply_adult_content_filter(&mut mods_filter, adult_content, true);
        match self
            .graphql(mods_facet_gql, json!({ "filter": mods_filter, "count": 1 }))
            .await
        {
            Ok(data) => {
                merge_facet_bucket(
                    &data,
                    "/mods/nodesFacets",
                    "/mods/facetsData",
                    &["tag"],
                    &mut mod_tags,
                );
                merge_facet_bucket(
                    &data,
                    "/mods/nodesFacets",
                    "/mods/facetsData",
                    &["categoryName", "category_name"],
                    &mut categories,
                );
            }
            Err(e) => log::warn!("mod facets failed: {e}"),
        }

        // Version-like collection tags as fallback for game-version filter
        if game_versions.is_empty() {
            for t in &collection_tags {
                if looks_like_version(t) {
                    push_unique(&mut game_versions, t);
                }
            }
        }

        mod_tags.sort_by_key(|s| s.to_lowercase());
        collection_tags.sort_by_key(|s| s.to_lowercase());
        categories.sort_by_key(|s| s.to_lowercase());
        game_versions.sort_by_key(|s| s.to_lowercase());

        Ok(BrowseMeta {
            categories,
            mod_tags,
            collection_tags,
            game_versions,
        })
    }

    pub async fn get_collection(
        &self,
        slug: &str,
        domain: Option<&str>,
        adult_content: bool,
    ) -> Result<CollectionDetail> {
        let gql = r#"
        query CollectionDetail($slug: String!, $viewAdultContent: Boolean, $domainName: String) {
          collection(slug: $slug, viewAdultContent: $viewAdultContent, domainName: $domainName) {
            slug
            name
            summary
            description
            endorsements
            totalDownloads
            game { domainName }
            latestPublishedRevision { revisionNumber }
            tileImage {
              url
              thumbnailUrl(size: med)
            }
          }
        }
        "#;

        let data = self
            .graphql(
                gql,
                json!({
                    "slug": slug,
                    "viewAdultContent": adult_content,
                    "domainName": domain,
                }),
            )
            .await?;

        let n = data
            .get("collection")
            .cloned()
            .ok_or_else(|| anyhow!("collection detail missing collection node"))?;
        if n.is_null() {
            bail!("collection not found: {slug}");
        }

        let tile = n.get("tileImage");
        let tile_image_url = tile
            .and_then(|t| t.get("thumbnailUrl").and_then(|v| v.as_str()))
            .or_else(|| tile.and_then(|t| t.get("url").and_then(|v| v.as_str())))
            .map(str::to_string);

        Ok(CollectionDetail {
            slug: n
                .get("slug")
                .and_then(|v| v.as_str())
                .unwrap_or(slug)
                .to_string(),
            name: n
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("collection missing name"))?
                .to_string(),
            summary: n
                .get("summary")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            description: n
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            endorsements: n.get("endorsements").and_then(|v| v.as_u64()),
            total_downloads: n.get("totalDownloads").and_then(|v| v.as_u64()),
            domain_name: n
                .pointer("/game/domainName")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            revision_number: n
                .pointer("/latestPublishedRevision/revisionNumber")
                .and_then(|v| v.as_i64()),
            tile_image_url,
        })
    }

    pub async fn collection_mod_files(
        &self,
        slug: &str,
        revision: Option<i64>,
        adult_content: bool,
    ) -> Result<Vec<CollectionModFile>> {
        let gql = r#"
        query CollectionRevisionMods($revision: Int, $slug: String!, $viewAdultContent: Boolean) {
          collectionRevision(revision: $revision, slug: $slug, viewAdultContent: $viewAdultContent) {
            modFiles {
              fileId
              optional
              file {
                fileId
                name
                version
                mod {
                  modId
                  name
                  game { domainName }
                }
              }
            }
          }
        }
        "#;

        let data = self
            .graphql(
                gql,
                json!({
                    "slug": slug,
                    "revision": revision,
                    "viewAdultContent": adult_content,
                }),
            )
            .await?;

        let files = data
            .pointer("/collectionRevision/modFiles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(files
            .into_iter()
            .filter_map(|n| {
                let file = n.get("file")?;
                let mod_obj = file.get("mod")?;
                Some(CollectionModFile {
                    file_id: file.get("fileId")?.as_u64()?,
                    optional: n.get("optional").and_then(|v| v.as_bool()).unwrap_or(false),
                    mod_id: mod_obj.get("modId")?.as_u64()?,
                    mod_name: mod_obj.get("name")?.as_str()?.to_string(),
                    file_name: file.get("name")?.as_str()?.to_string(),
                    version: file
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    domain_name: mod_obj
                        .pointer("/game/domainName")
                        .and_then(|v| v.as_str())?
                        .to_string(),
                })
            })
            .collect())
    }

    /// Version-like author tags for the given mod ids, keyed by mod id.
    pub async fn mod_version_tags(
        &self,
        domain: &str,
        mod_ids: &[u64],
    ) -> Result<std::collections::HashMap<u64, Vec<String>>> {
        let mut out = std::collections::HashMap::new();
        if mod_ids.is_empty() {
            return Ok(out);
        }
        let gql = r#"
        query ModVersionTags($filter: ModsFilter, $count: Int!) {
          mods(filter: $filter, count: $count) {
            nodes {
              modId
              tags { name }
            }
          }
        }
        "#;
        for chunk in mod_ids.chunks(40) {
            let data = self
                .graphql(
                    gql,
                    json!({
                        "filter": mods_id_filter(domain, chunk),
                        "count": chunk.len().max(1),
                    }),
                )
                .await?;
            let nodes = data
                .pointer("/mods/nodes")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for node in nodes {
                let Some(id) = node.get("modId").and_then(|v| v.as_u64()) else {
                    continue;
                };
                let tags = node
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|t| {
                                t.get("name").and_then(|v| v.as_str()).map(str::to_string)
                            })
                            .filter(|t| {
                                looks_like_version(t)
                                    || crate::mods::game_version::is_line_alias(domain, t)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.insert(id, tags);
            }
        }
        Ok(out)
    }

    /// Game versions a collection revision says it was built for.
    pub async fn collection_game_versions(
        &self,
        slug: &str,
        revision: Option<i64>,
        adult_content: bool,
    ) -> Result<Vec<String>> {
        let gql = r#"
        query CollectionRevisionVersions($revision: Int, $slug: String!, $viewAdultContent: Boolean) {
          collectionRevision(revision: $revision, slug: $slug, viewAdultContent: $viewAdultContent) {
            gameVersions { reference }
          }
        }
        "#;
        let data = self
            .graphql(
                gql,
                json!({
                    "slug": slug,
                    "revision": revision,
                    "viewAdultContent": adult_content,
                }),
            )
            .await?;
        let mut versions = Vec::new();
        if let Some(arr) = data
            .pointer("/collectionRevision/gameVersions")
            .and_then(|v| v.as_array())
        {
            for item in arr {
                if let Some(reference) = item.get("reference").and_then(|v| v.as_str()) {
                    push_unique(&mut versions, reference);
                }
            }
        }
        Ok(versions)
    }

    /// Fuzzy username search for browse `@creator` autocomplete.
    pub async fn search_users(&self, query: &str, count: u32) -> Result<Vec<AuthorSuggestion>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let count = count.clamp(1, 25);
        let gql = r#"
        query SearchUsers($filter: UsersSearchFilter, $count: Int!) {
          users(
            filter: $filter,
            count: $count,
            sort: [{ relevance: { direction: DESC } }]
          ) {
            nodes {
              memberId
              name
              modCount
              avatar
            }
          }
        }
        "#;
        let variables = json!({
            "filter": {
                "nameWildcard": [{ "value": query, "op": "WILDCARD" }]
            },
            "count": count,
        });
        let data = self.graphql(gql, variables).await?;
        let nodes = data
            .get("users")
            .and_then(|v| v.get("nodes"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(nodes
            .into_iter()
            .filter_map(|n| {
                let name = n.get("name")?.as_str()?.trim().to_string();
                if name.is_empty() {
                    return None;
                }
                Some(AuthorSuggestion {
                    name,
                    member_id: n.get("memberId").and_then(|v| v.as_u64()),
                    mod_count: n.get("modCount").and_then(|v| v.as_u64()),
                    avatar: n
                        .get("avatar")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                })
            })
            .collect())
    }

    pub(super) async fn graphql(&self, query: &str, variables: Value) -> Result<Value> {
        let payload = json!({ "query": query, "variables": variables });
        let resp = self
            .send_with_retry(None, || self.http.post(GQL_URL).json(&payload))
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await?;
        if !status.is_success() {
            bail!("GraphQL HTTP {status}: {body}");
        }
        if let Some(errors) = body.get("errors") {
            bail!("GraphQL errors: {errors}");
        }
        body.get("data")
            .cloned()
            .ok_or_else(|| anyhow!("GraphQL response missing data"))
    }
}

fn detail_from_mod_response(
    status: StatusCode,
    body: &str,
    domain: &str,
    mod_id: u64,
) -> Result<ModDetail> {
    if status == StatusCode::NOT_FOUND {
        return Ok(unavailable_mod_detail(domain, mod_id));
    }
    if !status.is_success() {
        bail!("get mod failed ({status}): {body}");
    }
    let n: Value = serde_json::from_str(body).context("Nexus did not return this mod")?;
    mod_detail_from_json(&n, domain, mod_id)
}

fn unavailable_mod_detail(domain: &str, mod_id: u64) -> ModDetail {
    ModDetail {
        mod_id,
        name: String::new(),
        summary: None,
        description: None,
        picture_url: None,
        image_urls: Vec::new(),
        author: None,
        version: None,
        downloads: None,
        endorsements: None,
        created_timestamp: None,
        updated_timestamp: None,
        domain_name: domain.to_string(),
        category_id: None,
        category: None,
        uploaded_by: None,
        status: None,
        contains_adult_content: false,
        uploaded_users_profile_url: None,
        available: Some(false),
        availability_note: None,
    }
}

fn mod_detail_from_json(n: &Value, domain: &str, fallback_mod_id: u64) -> Result<ModDetail> {
    let mod_id = n
        .get("mod_id")
        .and_then(|v| v.as_u64())
        .unwrap_or(fallback_mod_id);
    let name = json_text(n, "name");
    let status = json_text(n, "status");
    let mut available = n.get("available").and_then(|v| v.as_bool());
    let unavailable = available == Some(false)
        || status.as_deref().is_some_and(is_non_public_status)
        || (name.is_none() && status.is_some());
    if name.is_none() && !unavailable {
        bail!("Nexus did not return this mod");
    }
    if unavailable {
        available = Some(false);
    }
    let domain_name = json_text(n, "domain_name").unwrap_or_else(|| domain.to_string());
    Ok(ModDetail {
        mod_id,
        name: name.unwrap_or_default(),
        summary: json_text(n, "summary"),
        description: json_text(n, "description"),
        picture_url: json_text(n, "picture_url"),
        image_urls: Vec::new(),
        author: json_text(n, "author"),
        version: json_text(n, "version"),
        downloads: n
            .get("mod_downloads")
            .and_then(|v| v.as_u64())
            .or_else(|| n.get("downloads").and_then(|v| v.as_u64())),
        endorsements: n.get("endorsement_count").and_then(|v| v.as_u64()),
        created_timestamp: n.get("created_timestamp").and_then(|v| v.as_u64()),
        updated_timestamp: n.get("updated_timestamp").and_then(|v| v.as_u64()),
        domain_name,
        category_id: n.get("category_id").and_then(|v| v.as_u64()),
        category: None,
        uploaded_by: json_text(n, "uploaded_by"),
        status,
        contains_adult_content: n
            .get("contains_adult_content")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        uploaded_users_profile_url: json_text(n, "uploaded_users_profile_url"),
        available,
        availability_note: None,
    })
}

fn json_text(n: &Value, key: &str) -> Option<String> {
    n.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn is_non_public_status(status: &str) -> bool {
    let normalized = status
        .trim()
        .chars()
        .map(|c| {
            if c == '_' {
                ' '
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect::<String>();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    const MARKERS: &[&str] = &[
        "hidden",
        "removed",
        "deleted",
        "under moderation",
        "not published",
        "wastebinned",
    ];
    MARKERS
        .iter()
        .any(|marker| normalized == *marker || normalized.contains(marker))
}

fn read_mod_page(html: &str) -> ModPageRead {
    ModPageRead {
        images: parse_mod_gallery(html),
        availability_note: parse_availability_note(html),
    }
}

/// Hidden, removed, or deleted banner on the public mod page.
fn parse_availability_note(html: &str) -> Option<String> {
    let at = find_ignore_ascii_case(html, "has been set to")?;
    let start = rfind_ignore_ascii_case(&html[..at], "this mod")
        .filter(|index| at.saturating_sub(*index) < 200)
        .unwrap_or(at);
    let tail = &html[at..];
    let end_rel = find_ignore_ascii_case(tail, "</div>").unwrap_or_else(|| tail.len().min(1200));
    let text = notice_text(&html[start..at + end_rel])?;
    let lower = text.to_ascii_lowercase();
    if !lower.contains("has been set to") || !is_non_public_status(&lower) {
        return None;
    }
    Some(text)
}

fn notice_text(chunk: &str) -> Option<String> {
    let flattened = chunk.replace(['\n', '\r'], " ");
    let broken = break_tags_to_newlines(&flattened);
    let decoded = decode_entities(&broken);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut in_tag = false;
    for c in decoded.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if in_tag {
            continue;
        }
        if c == '\n' {
            push_notice_line(&mut lines, &mut current);
        } else {
            current.push(c);
        }
    }
    push_notice_line(&mut lines, &mut current);
    if lines.is_empty() {
        return None;
    }
    let mut out = String::new();
    for line in lines {
        if !out.is_empty() {
            let last = out.chars().last()?;
            if !matches!(last, '.' | ':' | '!' | '?') {
                out.push('.');
            }
            out.push(' ');
        }
        out.push_str(&line);
    }
    Some(out)
}

fn push_notice_line(lines: &mut Vec<String>, current: &mut String) {
    let trimmed = current.split_whitespace().collect::<Vec<_>>().join(" ");
    current.clear();
    if !trimmed.is_empty() {
        lines.push(trimmed);
    }
}

fn break_tags_to_newlines(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = find_ignore_ascii_case(rest, "<br") {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let Some(end) = after.find('>') else {
            out.push_str(after);
            return out;
        };
        let tag = &after[1..end];
        let name_end = tag
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .unwrap_or(tag.len());
        if tag[..name_end].eq_ignore_ascii_case("br") {
            out.push('\n');
            rest = &after[end + 1..];
        } else {
            out.push('<');
            rest = &rest[at + 1..];
        }
    }
    out.push_str(rest);
    out
}

/// Images from the author carousel (`ul.thumbgallery`) on a Nexus mod page.
fn parse_mod_gallery(html: &str) -> Vec<ModGalleryImage> {
    let Some(open) = thumbgallery_ul(html) else {
        return Vec::new();
    };
    let Some(inner) = element_inner(html, open) else {
        return Vec::new();
    };
    let mut images: Vec<ModGalleryImage> = Vec::new();
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        let is_li = bytes[i] == b'<'
            && bytes[i + 1].eq_ignore_ascii_case(&b'l')
            && bytes[i + 2].eq_ignore_ascii_case(&b'i')
            && matches!(bytes[i + 3], b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/');
        if !is_li {
            i += 1;
            continue;
        }
        let Some(end) = opening_tag_end(inner, i) else {
            break;
        };
        let tag = &inner[i..=end];
        if let Some(url) = attr_value(tag, "data-src").and_then(normalize_image_url) {
            if !images.iter().any(|image| image.url == url) {
                images.push(ModGalleryImage {
                    url,
                    caption: attr_value(tag, "data-sub-html").and_then(caption_text),
                });
            }
        }
        i = end + 1;
    }
    images
}

fn thumbgallery_ul(html: &str) -> Option<usize> {
    let mut search = 0;
    while let Some(at) = find_ignore_ascii_case(&html[search..], "thumbgallery") {
        let abs = search + at;
        if let Some(open) = html[..abs].rfind('<') {
            let tag = html.as_bytes();
            if tag
                .get(open + 1)
                .is_some_and(|b| b.eq_ignore_ascii_case(&b'u'))
                && tag
                    .get(open + 2)
                    .is_some_and(|b| b.eq_ignore_ascii_case(&b'l'))
                && tag
                    .get(open + 3)
                    .is_some_and(|b| b.is_ascii_whitespace() || *b == b'>')
            {
                return Some(open);
            }
        }
        search = abs + "thumbgallery".len();
    }
    None
}

fn element_inner(html: &str, open_at: usize) -> Option<&str> {
    let gt = opening_tag_end(html, open_at)?;
    let mut depth = 1usize;
    let mut i = gt + 1;
    let bytes = html.as_bytes();
    while i + 4 < bytes.len() {
        if bytes[i] == b'<'
            && bytes[i + 1].eq_ignore_ascii_case(&b'u')
            && bytes[i + 2].eq_ignore_ascii_case(&b'l')
            && (bytes[i + 3].is_ascii_whitespace() || bytes[i + 3] == b'>')
        {
            depth += 1;
            i += 3;
            continue;
        }
        if bytes[i] == b'<'
            && bytes[i + 1] == b'/'
            && bytes[i + 2].eq_ignore_ascii_case(&b'u')
            && bytes[i + 3].eq_ignore_ascii_case(&b'l')
        {
            depth -= 1;
            if depth == 0 {
                return Some(&html[gt + 1..i]);
            }
            i += 4;
            continue;
        }
        i += 1;
    }
    None
}

fn opening_tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut i = start;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == b'"' || c == b'\'' {
            quote = Some(c);
        } else if c == b'>' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=");
    let mut search = 0;
    while let Some(rel) = find_ignore_ascii_case(&tag[search..], &needle) {
        let at = search + rel;
        if at > 0 {
            let prev = tag.as_bytes()[at - 1];
            if !prev.is_ascii_whitespace() && prev != b'<' {
                search = at + needle.len();
                continue;
            }
        }
        let rest = &tag[at + needle.len()..];
        let bytes = rest.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        if bytes[0] == b'"' || bytes[0] == b'\'' {
            let q = bytes[0] as char;
            let val = &rest[1..];
            let end = val.find(q)?;
            return Some(&val[..end]);
        }
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest.len());
        return Some(&rest[..end]);
    }
    None
}

fn normalize_image_url(raw: &str) -> Option<String> {
    let decoded = decode_entities(raw.trim());
    let url = if let Some(rest) = decoded.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        decoded
    };
    if url.starts_with("https://") || url.starts_with("http://") {
        Some(url)
    } else {
        None
    }
}

fn caption_text(raw: &str) -> Option<String> {
    let decoded = decode_entities(raw);
    let mut out = String::new();
    let mut in_tag = false;
    for c in decoded.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if !in_tag {
            out.push(c);
        }
    }
    let trimmed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let Some(semi) = after.find(';') else {
            out.push('&');
            rest = after;
            continue;
        };
        if semi > 16 {
            out.push('&');
            rest = after;
            continue;
        }
        if let Some(ch) = decode_one(&after[..semi]) {
            out.push(ch);
            rest = &after[semi + 1..];
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn decode_one(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "nbsp" => Some(' '),
        _ => {
            let num = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|num| num.parse().ok()))?;
            char::from_u32(num)
        }
    }
}

fn rfind_ignore_ascii_case(haystack: &str, needle: &str) -> Option<usize> {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let mut i = haystack.len() - needle.len();
    loop {
        if haystack[i..i + needle.len()].eq_ignore_ascii_case(needle) {
            return Some(i);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

fn find_ignore_ascii_case(haystack: &str, needle: &str) -> Option<usize> {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let last = haystack.len() - needle.len();
    let mut i = 0;
    while i <= last {
        if haystack[i..i + needle.len()].eq_ignore_ascii_case(needle) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn normalize_sort(sort: &str) -> String {
    let s = sort.trim().to_ascii_lowercase();
    match s.as_str() {
        "downloads" | "most_downloaded" | "most-downloaded" => "downloads".into(),
        "newest" | "created" | "created_at" | "createdat" => "createdAt".into(),
        "updated" | "recently_updated" | "updated_at" | "updatedat" => "updatedAt".into(),
        "relevance" | "best_match" | "best-match" => "relevance".into(),
        "rating" | "highest_rated" | "highest-rated" => "rating".into(),
        "trending" => "trending".into(),
        _ => "endorsements".into(),
    }
}

fn has_extra_filters(opts: &BrowseSearchOpts) -> bool {
    opts.category.as_ref().is_some_and(|s| !s.is_empty())
        || !opts.tags_include.is_empty()
        || !opts.tags_exclude.is_empty()
        || opts.game_version.as_ref().is_some_and(|s| !s.is_empty())
        || opts.author.as_ref().is_some_and(|s| !s.trim().is_empty())
}

fn mods_sort_json(sort_key: &str, has_query: bool) -> Value {
    let key = if sort_key == "relevance" && !has_query {
        "endorsements"
    } else if sort_key == "trending" {
        "endorsements"
    } else if sort_key == "rating" {
        "endorsements"
    } else {
        sort_key
    };
    json!({ key: { "direction": "DESC" } })
}

fn collections_sort_json(sort_key: &str, has_query: bool) -> Value {
    let key = if sort_key == "relevance" && !has_query {
        "endorsements"
    } else if sort_key == "trending" {
        "endorsements"
    } else {
        sort_key
    };
    json!({ key: { "direction": "DESC" } })
}

/// Official Nexus clients only set adultContent when excluding adult results.
fn apply_adult_content_filter(filter: &mut Value, adult_content: bool, as_array: bool) {
    if adult_content {
        return;
    }
    let clause = json!({ "value": false, "op": "EQUALS" });
    filter["adultContent"] = if as_array { json!([clause]) } else { clause };
}

fn tag_value(value: &str, op: &str) -> Value {
    json!({ "value": value, "op": op })
}

fn name_search_clause(query: &str) -> Value {
    json!({
        "op": "OR",
        "filter": [
            { "name": [{ "value": query, "op": "WILDCARD" }] },
            { "nameStemmed": [{ "value": query, "op": "MATCHES" }] }
        ]
    })
}

/// AND `clause` into a nested filter without replacing clauses already there.
fn and_clause(filter: &mut Value, clause: Value) {
    if let Some(clauses) = filter.get_mut("filter").and_then(|v| v.as_array_mut()) {
        clauses.push(clause);
        if filter.get("op").is_none() {
            filter["op"] = json!("AND");
        }
        return;
    }
    filter["op"] = json!("AND");
    filter["filter"] = json!([clause]);
}

fn apply_tag_filters(filter: &mut Value, include: &[String], exclude: &[String]) {
    let includes: Vec<&String> = include.iter().filter(|t| !t.is_empty()).collect();
    let excludes: Vec<&String> = exclude.iter().filter(|t| !t.is_empty()).collect();
    let total = includes.len() + excludes.len();
    if total == 0 {
        return;
    }
    if total == 1 {
        if let Some(t) = includes.first() {
            filter["tag"] = json!([tag_value(t, "EQUALS")]);
        } else if let Some(t) = excludes.first() {
            filter["tag"] = json!([tag_value(t, "NOT_EQUALS")]);
        }
        return;
    }

    let mut nested: Vec<Value> = Vec::with_capacity(total);
    for t in includes {
        nested.push(json!({ "tag": [tag_value(t, "EQUALS")] }));
    }
    for t in excludes {
        nested.push(json!({ "tag": [tag_value(t, "NOT_EQUALS")] }));
    }
    filter["filter"] = Value::Array(nested);
    filter["op"] = json!("AND");
}

fn build_mods_filter(
    domain: &str,
    query: &str,
    adult_content: bool,
    opts: &BrowseSearchOpts,
) -> Value {
    let mut filter = json!({
        "gameDomainName": [{ "value": domain, "op": "EQUALS" }],
    });
    apply_adult_content_filter(&mut filter, adult_content, true);
    if let Some(cat) = opts.category.as_ref().filter(|s| !s.is_empty()) {
        filter["categoryName"] = json!([{ "value": cat, "op": "EQUALS" }]);
    }
    if let Some(author) = opts
        .author
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        filter["uploader"] = json!([{ "value": author, "op": "EQUALS" }]);
    }
    // ModsFilter has no gameVersion field. Authors publish support as tags.
    let mut include = opts.tags_include.clone();
    if let Some(ver) = opts
        .game_version
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if !include.iter().any(|t| t.eq_ignore_ascii_case(ver)) {
            include.insert(0, ver.to_string());
        }
    }
    apply_tag_filters(&mut filter, &include, &opts.tags_exclude);
    // `name` matches punctuation, so "SMAPI" misses "SMAPI - Stardew Modding API".
    // `nameStemmed` matches tokens. Nest the OR under the tag AND.
    let query = query.trim();
    if !query.is_empty() {
        and_clause(&mut filter, name_search_clause(query));
    }
    filter
}

fn build_collections_filter(
    domain: &str,
    query: &str,
    adult_content: bool,
    opts: &BrowseSearchOpts,
) -> Value {
    let mut filter = json!({
        "gameDomain": [{ "value": domain, "op": "EQUALS" }],
    });
    apply_adult_content_filter(&mut filter, adult_content, true);
    if !query.trim().is_empty() {
        filter["generalSearch"] = json!([{ "value": query.trim(), "op": "WILDCARD" }]);
    }
    if let Some(cat) = opts.category.as_ref().filter(|s| !s.is_empty()) {
        filter["categoryName"] = json!([{ "value": cat, "op": "EQUALS" }]);
    }
    if let Some(ver) = opts.game_version.as_ref().filter(|s| !s.is_empty()) {
        filter["gameVersion"] = json!([{ "value": ver, "op": "EQUALS" }]);
    }
    apply_tag_filters(&mut filter, &opts.tags_include, &opts.tags_exclude);
    filter
}

fn parse_mod_hit(n: &Value, domain: &str) -> Option<ModSearchHit> {
    let tags = n
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.get("name").and_then(|v| v.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(ModSearchHit {
        mod_id: n.get("modId")?.as_u64()?,
        name: n.get("name")?.as_str()?.to_string(),
        summary: n
            .get("summary")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        picture_url: n
            .get("pictureUrl")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        downloads: n.get("downloads").and_then(|v| v.as_u64()),
        endorsements: n.get("endorsements").and_then(|v| v.as_u64()),
        author: n.get("author").and_then(|v| v.as_str()).map(str::to_string),
        domain_name: n
            .pointer("/game/domainName")
            .and_then(|v| v.as_str())
            .unwrap_or(domain)
            .to_string(),
        category: n.get("category").and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.get("name").and_then(|x| x.as_str()).map(str::to_string))
        }),
        tags,
    })
}

fn push_unique(out: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if !out.iter().any(|x| x.eq_ignore_ascii_case(value)) {
        out.push(value.to_string());
    }
}

fn facet_name_matches(name: &str, aliases: &[&str]) -> bool {
    aliases.iter().any(|alias| name.eq_ignore_ascii_case(alias))
}

/// Collect values for the given facet names from nodesFacets and/or facetsData.
///
/// `nodesFacets` entries are flat `{ facet, value, count }`.
/// `facetsData` is `{"facetName":{"facetValue":count}}` (nested object map).
fn merge_facet_bucket(
    data: &Value,
    nodes_path: &str,
    facets_data_path: &str,
    facet_aliases: &[&str],
    out: &mut Vec<String>,
) {
    if let Some(facets) = data.pointer(nodes_path).and_then(|v| v.as_array()) {
        for facet in facets {
            let name = facet.get("facet").and_then(|v| v.as_str()).unwrap_or("");
            if !facet_name_matches(name, facet_aliases) {
                continue;
            }
            if let Some(val) = facet.get("value").and_then(|v| v.as_str()) {
                push_unique(out, val);
            }
        }
    }

    if let Some(obj) = data.pointer(facets_data_path).and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if !facet_name_matches(k, facet_aliases) {
                continue;
            }
            match v {
                Value::Object(map) => {
                    for value_key in map.keys() {
                        push_unique(out, value_key);
                    }
                }
                Value::Array(arr) => {
                    for item in arr {
                        let val = item
                            .as_str()
                            .or_else(|| item.get("value").and_then(|x| x.as_str()));
                        if let Some(val) = val {
                            push_unique(out, val);
                        }
                    }
                }
                Value::String(s) => push_unique(out, s),
                _ => {}
            }
        }
    }
}

pub(crate) fn mods_id_filter(domain: &str, mod_ids: &[u64]) -> Value {
    if mod_ids.len() == 1 {
        return json!({
            "gameDomainName": [{ "value": domain, "op": "EQUALS" }],
            "modId": [{ "value": mod_ids[0].to_string(), "op": "EQUALS" }],
        });
    }
    let nested: Vec<Value> = mod_ids
        .iter()
        .map(|id| json!({ "modId": [{ "value": id.to_string(), "op": "EQUALS" }] }))
        .collect();
    json!({
        "gameDomainName": [{ "value": domain, "op": "EQUALS" }],
        "op": "AND",
        "filter": [{ "op": "OR", "filter": nested }],
    })
}

pub(crate) fn looks_like_version(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.len() > 24 {
        return false;
    }
    let has_digit = trimmed.chars().any(|c| c.is_ascii_digit());
    let ok_chars = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '));
    has_digit && ok_chars && trimmed.contains('.')
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NxmLink {
    pub domain: String,
    pub mod_id: u64,
    pub file_id: u64,
    pub key: Option<String>,
    pub expires: Option<u64>,
}

/// Parse `nxm://domain/mods/123/files/456?key=...&expires=...`
pub fn parse_nxm(url_str: &str) -> Result<NxmLink> {
    let url_str = url_str.trim();
    if !url_str.starts_with("nxm://") {
        bail!("not an nxm:// URL");
    }
    // url crate needs a standard scheme for query parsing; rewrite temporarily.
    let httpish = format!("https://{}", url_str.trim_start_matches("nxm://"));
    let parsed = url::Url::parse(&httpish).context("invalid nxm URL")?;
    let mut parts: Vec<&str> = parsed.path().split('/').filter(|p| !p.is_empty()).collect();
    // host is the domain name (first segment of nxm path)
    let domain = parsed
        .host_str()
        .ok_or_else(|| anyhow!("nxm URL missing game domain"))?
        .to_string();
    // path is /mods/{id}/files/{id}
    if parts.len() < 4 || parts[0] != "mods" || parts[2] != "files" {
        // Some parsers keep domain in path if host empty — handle path-only form
        let rest = url_str.trim_start_matches("nxm://");
        let path = rest.split('?').next().unwrap_or(rest);
        parts = path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() < 5 || parts[1] != "mods" || parts[3] != "files" {
            bail!("unrecognized nxm path: {path}");
        }
        let mut key = None;
        let mut expires = None;
        if let Some(q) = rest.split('?').nth(1) {
            for pair in q.split('&') {
                let mut kv = pair.splitn(2, '=');
                let k = kv.next().unwrap_or("");
                let v = kv.next().unwrap_or("");
                match k {
                    "key" => key = Some(v.to_string()),
                    "expires" => expires = v.parse().ok(),
                    _ => {}
                }
            }
        }
        return Ok(NxmLink {
            domain: parts[0].to_string(),
            mod_id: parts[2].parse()?,
            file_id: parts[4].parse()?,
            key,
            expires,
        });
    }

    let mut key = None;
    let mut expires = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "key" => key = Some(v.to_string()),
            "expires" => expires = v.parse().ok(),
            _ => {}
        }
    }

    Ok(NxmLink {
        domain,
        mod_id: parts[1].parse()?,
        file_id: parts[3].parse()?,
        key,
        expires,
    })
}

pub fn store_api_key(key: &str) -> Result<()> {
    let entry = keyring::Entry::new(crate::config::KEYRING_SERVICE, crate::config::KEYRING_USER)?;
    entry.set_password(key.trim())?;
    Ok(())
}

pub fn load_api_key() -> Result<Option<String>> {
    let entry = keyring::Entry::new(crate::config::KEYRING_SERVICE, crate::config::KEYRING_USER)?;
    match entry.get_password() {
        Ok(k) if !k.is_empty() => Ok(Some(k)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            // Fall back to config file for headless / missing secret service
            log::warn!("keyring read failed: {e}");
            Ok(None)
        }
    }
}

pub fn clear_api_key() -> Result<()> {
    let entry = keyring::Entry::new(crate::config::KEYRING_SERVICE, crate::config::KEYRING_USER)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Encrypted file-based API key fallback when keyring is unavailable.
pub fn store_api_key_file(path: &Path, key: &str) -> Result<()> {
    crate::config::secret_store::store_encrypted_secret(path, key)
}

pub fn load_api_key_file(path: &Path) -> Result<Option<String>> {
    crate::config::secret_store::load_encrypted_secret(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_and_legacy_gallery_urls() {
        let html = r#"
            <div class="clearfix modimages">
              <img data-src="https://example.com/not-in-gallery.jpg" alt="">
              <ul class="thumbgallery gallery clearfix">
                <li class="thumb"
                    data-src="https://staticdelivery.nexusmods.com/mods/1704/images/12604/12604-1774526205-321078529.png"
                    data-sub-html="New &amp; shiny">
                </li>
                <li class="thumb"
                    data-sub-html="Inventory Comparison"
                    data-src="https://staticdelivery.nexusmods.com/mods/1704/images/12604-1-1506820354.jpg">
                </li>
                <li data-src="//staticdelivery.nexusmods.com/mods/1704/images/12604/12604-1774526205-321078529.png"></li>
              </ul>
            </div>
        "#;
        let images = parse_mod_gallery(html);
        assert_eq!(
            images,
            vec![
                ModGalleryImage {
                    url: "https://staticdelivery.nexusmods.com/mods/1704/images/12604/12604-1774526205-321078529.png".into(),
                    caption: Some("New & shiny".into()),
                },
                ModGalleryImage {
                    url: "https://staticdelivery.nexusmods.com/mods/1704/images/12604-1-1506820354.jpg".into(),
                    caption: Some("Inventory Comparison".into()),
                },
            ]
        );
    }

    #[test]
    fn hidden_mod_without_a_name_is_unavailable() {
        let body = r#"{"mod_id":5188,"name":null,"status":"hidden","available":false,"domain_name":"starfield"}"#;
        let detail = detail_from_mod_response(StatusCode::OK, body, "starfield", 5188).unwrap();
        assert_eq!(detail.name, "");
        assert_eq!(detail.status.as_deref(), Some("hidden"));
        assert_eq!(detail.available, Some(false));
        assert_eq!(detail.mod_id, 5188);
        assert_eq!(detail.domain_name, "starfield");
        assert!(detail.availability_note.is_none());
    }

    #[test]
    fn a_named_hidden_mod_keeps_its_name() {
        let body =
            r#"{"mod_id":9,"name":"Show Star Names","status":"under_moderation","available":true}"#;
        let detail = detail_from_mod_response(StatusCode::OK, body, "starfield", 9).unwrap();
        assert_eq!(detail.name, "Show Star Names");
        assert_eq!(detail.status.as_deref(), Some("under_moderation"));
        assert_eq!(detail.available, Some(false));
    }

    #[test]
    fn a_missing_mod_is_unavailable() {
        let detail =
            detail_from_mod_response(StatusCode::NOT_FOUND, "missing", "starfield", 5188).unwrap();
        assert_eq!(detail.name, "");
        assert_eq!(detail.available, Some(false));
        assert_eq!(detail.mod_id, 5188);
        assert_eq!(detail.domain_name, "starfield");
        assert!(detail.status.is_none());
    }

    #[test]
    fn a_nameless_payload_without_status_is_an_error() {
        let err = detail_from_mod_response(StatusCode::OK, r#"{"mod_id":1}"#, "starfield", 1)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Nexus did not return this mod"), "{msg}");
        assert!(!msg.contains("mod response missing name"), "{msg}");
    }

    #[test]
    fn a_published_mod_keeps_its_name() {
        let body = r#"{"mod_id":8,"name":"Darktide Mod Framework","status":"published","available":true,"summary":"Framework","author":"Author","version":"1.0","domain_name":"warhammer40kdarktide"}"#;
        let detail = detail_from_mod_response(StatusCode::OK, body, "starfield", 8).unwrap();
        assert_eq!(detail.name, "Darktide Mod Framework");
        assert_eq!(detail.summary.as_deref(), Some("Framework"));
        assert_eq!(detail.author.as_deref(), Some("Author"));
        assert_eq!(detail.version.as_deref(), Some("1.0"));
        assert_eq!(detail.available, Some(true));
        assert_eq!(detail.status.as_deref(), Some("published"));
        assert!(detail.availability_note.is_none());
    }

    #[test]
    fn hidden_page_notice_includes_the_reason() {
        let html = r#"
            <div class="info warning clearfix site-notice">
              <div class="info-content">
                <h3>Hidden mod</h3>
                <p>This mod has been set to hidden<br><br>
                  <p><b>Hidden at 06 Nov 2026, 7:57PM by <a href="/users/191643000">FSNovask</a></b>
                  for the following reason: <br/ >
                  <i>This mod is currently not supported by the author(s) and/or has issue(s) they are unable to fix yet.</i></p>
                </p>
              </div>
            </div>
        "#;
        let note = parse_availability_note(html).unwrap();
        assert_eq!(
            note,
            "This mod has been set to hidden. Hidden at 06 Nov 2026, 7:57PM by FSNovask for the following reason: This mod is currently not supported by the author(s) and/or has issue(s) they are unable to fix yet."
        );
    }

    #[test]
    fn an_unrelated_page_has_no_availability_note() {
        assert!(parse_availability_note("<p>Just a moment...</p>").is_none());
        assert!(parse_availability_note(
            r#"<p>This mod has been set to published and is ready.</p>"#
        )
        .is_none());
    }

    #[test]
    fn an_empty_or_unrelated_page_has_no_gallery() {
        assert!(parse_mod_gallery("<p>Just a moment...</p>").is_empty());
        assert!(parse_mod_gallery(
            r#"<ul class="other"><li data-src="https://example.com/a.jpg"></li></ul>"#
        )
        .is_empty());
    }

    #[test]
    fn a_short_body_is_an_error_not_a_finished_download() {
        let err = verify_complete(199_000_000, Some(224_000_000)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(INCOMPLETE_MSG), "{msg}");
        assert!(msg.contains("199000000"), "{msg}");
        assert!(msg.contains("224000000"), "{msg}");
    }

    #[test]
    fn a_complete_or_unmeasurable_transfer_passes() {
        verify_complete(1024, Some(1024)).unwrap();
        // Some servers over-report; that is not worth failing a usable file over.
        verify_complete(1024, Some(1000)).unwrap();
        // No Content-Length leaves nothing to compare against.
        verify_complete(1024, None).unwrap();
    }

    #[test]
    fn cdn_query_parameters_do_not_become_part_of_the_filename() {
        let name = filename_from_url(
            "https://cf.nexus.com/1034/Cool%20Mod-1034-1-0.7z?md5=abc&expires=123",
        );
        assert_eq!(name.as_deref(), Some("Cool Mod-1034-1-0.7z"));
    }

    #[test]
    fn a_url_without_a_real_filename_yields_nothing() {
        assert_eq!(filename_from_url("https://cf.nexus.com/1034/"), None);
        assert_eq!(filename_from_url("https://cf.nexus.com/download"), None);
        assert_eq!(filename_from_url("not a url"), None);
    }

    #[test]
    fn parses_nxm_url() {
        let link =
            parse_nxm("nxm://stardewvalley/mods/2400/files/12345?key=abc&expires=1&user_id=2")
                .unwrap();
        assert_eq!(link.domain, "stardewvalley");
        assert_eq!(link.mod_id, 2400);
        assert_eq!(link.file_id, 12345);
        assert_eq!(link.key.as_deref(), Some("abc"));
        assert_eq!(link.expires, Some(1));
    }

    fn base_opts() -> BrowseSearchOpts {
        BrowseSearchOpts {
            sort: "endorsements".into(),
            ..Default::default()
        }
    }

    #[test]
    fn mods_filter_omits_adult_when_included() {
        let filter = build_mods_filter("skyrim", "", true, &base_opts());
        assert!(filter.get("adultContent").is_none());
    }

    #[test]
    fn mods_filter_excludes_adult_when_disabled() {
        let filter = build_mods_filter("skyrim", "", false, &base_opts());
        assert_eq!(
            filter["adultContent"],
            json!([{ "value": false, "op": "EQUALS" }])
        );
    }

    #[test]
    fn mods_filter_query_uses_bare_wildcard_value() {
        let filter = build_mods_filter("skyrim", "SKSE", true, &base_opts());
        assert_eq!(filter["op"], json!("AND"));
        assert_eq!(
            filter["filter"],
            json!([{
                "op": "OR",
                "filter": [
                    { "name": [{ "value": "SKSE", "op": "WILDCARD" }] },
                    { "nameStemmed": [{ "value": "SKSE", "op": "MATCHES" }] }
                ]
            }])
        );
        let value = filter["filter"][0]["filter"][0]["name"][0]["value"]
            .as_str()
            .unwrap();
        assert!(
            !value.contains('*'),
            "WILDCARD must not wrap query with '*'"
        );
    }

    #[test]
    fn mods_filter_query_nests_inside_tag_and() {
        let mut opts = base_opts();
        opts.tags_include = vec!["A".into(), "B".into()];
        let filter = build_mods_filter("skyrim", "SMAPI", true, &opts);
        assert_eq!(filter["op"], json!("AND"));
        assert_eq!(
            filter["filter"],
            json!([
                { "tag": [{ "value": "A", "op": "EQUALS" }] },
                { "tag": [{ "value": "B", "op": "EQUALS" }] },
                {
                    "op": "OR",
                    "filter": [
                        { "name": [{ "value": "SMAPI", "op": "WILDCARD" }] },
                        { "nameStemmed": [{ "value": "SMAPI", "op": "MATCHES" }] }
                    ]
                }
            ])
        );
    }

    #[test]
    fn mods_filter_single_include_tag() {
        let mut opts = base_opts();
        opts.tags_include = vec!["Performance".into()];
        let filter = build_mods_filter("skyrim", "", true, &opts);
        assert_eq!(
            filter["tag"],
            json!([{ "value": "Performance", "op": "EQUALS" }])
        );
        assert!(filter.get("filter").is_none());
    }

    #[test]
    fn mods_filter_author_uses_uploader_equals() {
        let mut opts = base_opts();
        opts.author = Some("CyanideX".into());
        let filter = build_mods_filter("cyberpunk2077", "", true, &opts);
        assert_eq!(
            filter["uploader"],
            json!([{ "value": "CyanideX", "op": "EQUALS" }])
        );
        assert_eq!(
            filter["gameDomainName"],
            json!([{ "value": "cyberpunk2077", "op": "EQUALS" }])
        );
        assert!(has_extra_filters(&opts));
    }

    #[test]
    fn mods_filter_single_exclude_tag() {
        let mut opts = base_opts();
        opts.tags_exclude = vec!["NSFW".into()];
        let filter = build_mods_filter("skyrim", "", true, &opts);
        assert_eq!(
            filter["tag"],
            json!([{ "value": "NSFW", "op": "NOT_EQUALS" }])
        );
    }

    #[test]
    fn mods_filter_multi_include_uses_and() {
        let mut opts = base_opts();
        opts.tags_include = vec!["A".into(), "B".into()];
        let filter = build_mods_filter("skyrim", "", true, &opts);
        assert_eq!(filter["op"], json!("AND"));
        assert_eq!(
            filter["filter"],
            json!([
                { "tag": [{ "value": "A", "op": "EQUALS" }] },
                { "tag": [{ "value": "B", "op": "EQUALS" }] },
            ])
        );
    }

    #[test]
    fn mods_filter_mixed_include_exclude() {
        let mut opts = base_opts();
        opts.tags_include = vec!["A".into()];
        opts.tags_exclude = vec!["B".into()];
        let filter = build_mods_filter("skyrim", "", true, &opts);
        assert_eq!(filter["op"], json!("AND"));
        assert_eq!(
            filter["filter"],
            json!([
                { "tag": [{ "value": "A", "op": "EQUALS" }] },
                { "tag": [{ "value": "B", "op": "NOT_EQUALS" }] },
            ])
        );
    }

    #[test]
    fn mods_id_filter_ors_multiple_ids() {
        let filter = mods_id_filter("skyrim", &[1, 2]);
        assert_eq!(filter["op"], json!("AND"));
        assert_eq!(
            filter["filter"][0]["filter"],
            json!([
                { "modId": [{ "value": "1", "op": "EQUALS" }] },
                { "modId": [{ "value": "2", "op": "EQUALS" }] },
            ])
        );
    }

    #[test]
    fn mods_filter_game_version_is_a_tag() {
        let mut opts = base_opts();
        opts.game_version = Some("1.6.0".into());
        let filter = build_mods_filter("skyrim", "", true, &opts);
        assert!(filter.get("gameVersion").is_none());
        assert_eq!(filter["tag"], json!([{ "value": "1.6.0", "op": "EQUALS" }]));
    }

    #[test]
    fn mods_filter_game_version_ands_with_tags() {
        let mut opts = base_opts();
        opts.game_version = Some("1.6.0".into());
        opts.tags_include = vec!["Gameplay".into()];
        let filter = build_mods_filter("skyrim", "", true, &opts);
        assert_eq!(filter["op"], json!("AND"));
        assert_eq!(
            filter["filter"],
            json!([
                { "tag": [{ "value": "1.6.0", "op": "EQUALS" }] },
                { "tag": [{ "value": "Gameplay", "op": "EQUALS" }] },
            ])
        );
    }

    #[test]
    fn collections_filter_arrays_and_version() {
        let mut opts = base_opts();
        opts.game_version = Some("1.6.0".into());
        opts.tags_include = vec!["Gameplay".into()];
        let filter = build_collections_filter("skyrim", "quest", false, &opts);
        assert_eq!(
            filter["gameDomain"],
            json!([{ "value": "skyrim", "op": "EQUALS" }])
        );
        assert_eq!(
            filter["adultContent"],
            json!([{ "value": false, "op": "EQUALS" }])
        );
        assert_eq!(
            filter["gameVersion"],
            json!([{ "value": "1.6.0", "op": "EQUALS" }])
        );
        assert_eq!(
            filter["tag"],
            json!([{ "value": "Gameplay", "op": "EQUALS" }])
        );
    }

    #[test]
    fn has_extra_filters_detects_include_and_exclude() {
        let mut opts = base_opts();
        assert!(!has_extra_filters(&opts));
        opts.tags_include = vec!["A".into()];
        assert!(has_extra_filters(&opts));
        opts.tags_include.clear();
        opts.tags_exclude = vec!["B".into()];
        assert!(has_extra_filters(&opts));
    }

    #[test]
    fn page_size_defaults() {
        assert_eq!(DEFAULT_MODS_PAGE_SIZE, 30);
        assert_eq!(DEFAULT_COLLECTIONS_PAGE_SIZE, 25);
    }

    #[test]
    fn retryable_transfer_err_classification() {
        assert!(is_retryable_transfer_err("THROTTLED_STALL: too slow"));
        assert!(is_retryable_transfer_err("download failed (429 Too Many Requests): slow"));
        assert!(is_retryable_transfer_err("download failed (503 Service Unavailable): "));
        assert!(!is_retryable_transfer_err("CANCELLED"));
        assert!(!is_retryable_transfer_err("PAUSED"));
        assert!(!is_retryable_transfer_err("download failed (404 Not Found): "));
    }

    #[tokio::test]
    async fn slow_drip_aborts_and_resumes_from_partial_offset() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let payload: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_payload = payload.clone();

        let server = tokio::spawn(async move {
            // Connection 1: send headers + 512 bytes, then drip 16 bytes every 60ms.
            let (mut sock1, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock1.read(&mut buf).await;
            let hdr1 = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                server_payload.len()
            );
            let _ = sock1.write_all(hdr1.as_bytes()).await;
            let _ = sock1.write_all(&server_payload[..512]).await;
            let _ = sock1.flush().await;
            for chunk in server_payload[512..1024].chunks(16) {
                tokio::time::sleep(Duration::from_millis(60)).await;
                if sock1.write_all(chunk).await.is_err() {
                    break;
                }
            }
            drop(sock1);

            // Connection 2: expect Range header and serve remaining bytes with 206.
            let (mut sock2, _) = listener.accept().await.unwrap();
            let n = sock2.read(&mut buf).await.unwrap_or(0);
            let req_text = String::from_utf8_lossy(&buf[..n]);
            let range_start = req_text
                .lines()
                .find_map(|l| {
                    let lower = l.to_ascii_lowercase();
                    lower
                        .strip_prefix("range: bytes=")
                        .and_then(|rest| rest.trim().strip_suffix('-'))
                        .and_then(|num| num.parse::<usize>().ok())
                })
                .unwrap_or(0);
            let remaining = &server_payload[range_start..];
            let hdr2 = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                remaining.len(),
                range_start,
                server_payload.len() - 1,
                server_payload.len()
            );
            let _ = sock2.write_all(hdr2.as_bytes()).await;
            let _ = sock2.write_all(remaining).await;
            let _ = sock2.flush().await;
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("mod.zip");
        let client = reqwest::Client::new();
        let url = format!("http://{addr}/mod.zip");

        let policy = StallPolicy {
            grace: Duration::from_millis(80),
            window: Duration::from_millis(180),
            min_bps: 4096,
        };
        let err = stream_url_to_file_with_policy(&client, &url, &dest, 0, None, None, None, policy)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains(THROTTLED_STALL_MSG),
            "unexpected error: {err}"
        );

        let partial_len = std::fs::metadata(&dest).unwrap().len();
        assert!(partial_len >= 512 && partial_len < payload.len() as u64);

        // Resume from partial_len and verify full payload.
        stream_url_to_file_with_policy(
            &client,
            &url,
            &dest,
            partial_len,
            None,
            None,
            None,
            StallPolicy::default(),
        )
        .await
        .unwrap();

        let got = std::fs::read(&dest).unwrap();
        assert_eq!(got, payload);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn stream_416_at_exact_total_treats_transfer_as_complete() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let payload = b"complete-mod-archive-bytes".to_vec();
        let total = payload.len();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.flush().await;
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("mod.zip");
        std::fs::write(&dest, &payload).unwrap();

        let progress_seen = Arc::new(std::sync::Mutex::new(None));
        let progress_seen_cb = Arc::clone(&progress_seen);
        let on_progress: Arc<dyn Fn(u64, Option<u64>, u64) + Send + Sync> =
            Arc::new(move |dl, tot, spd| {
                *progress_seen_cb.lock().unwrap() = Some((dl, tot, spd));
            });

        let client = reqwest::Client::new();
        let url = format!("http://{addr}/mod.zip");
        stream_url_to_file(
            &client,
            &url,
            &dest,
            total as u64,
            None,
            None,
            Some(&on_progress),
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), payload);
        assert_eq!(
            *progress_seen.lock().unwrap(),
            Some((total as u64, Some(total as u64), 0))
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn stream_416_oversized_or_without_content_range_restarts_from_zero() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let payload = b"fresh-mod-archive-bytes".to_vec();
        let server_payload = payload.clone();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            // Connection 1: receives Range, returns 416 HTML without Content-Range.
            let (mut sock1, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let n = sock1.read(&mut buf).await.unwrap_or(0);
            let req1 = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
            assert!(req1.contains("range: bytes="), "expected Range on first request: {req1}");
            let html = "<html><head><title>416 Requested Range Not Satisfiable</title></head></html>";
            let resp1 = format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            let _ = sock1.write_all(resp1.as_bytes()).await;
            let _ = sock1.flush().await;
            drop(sock1);

            // Connection 2: expects no Range header, serves full 200 OK.
            let (mut sock2, _) = listener.accept().await.unwrap();
            let n = sock2.read(&mut buf).await.unwrap_or(0);
            let req2 = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
            assert!(!req2.contains("range:"), "unexpected Range on restart request: {req2}");
            let resp2 = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                server_payload.len()
            );
            let _ = sock2.write_all(resp2.as_bytes()).await;
            let _ = sock2.write_all(&server_payload).await;
            let _ = sock2.flush().await;
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("mod.zip");
        std::fs::write(&dest, vec![b'x'; payload.len() + 100]).unwrap();

        let client = reqwest::Client::new();
        let url = format!("http://{addr}/mod.zip");
        stream_url_to_file(
            &client,
            &url,
            &dest,
            (payload.len() + 100) as u64,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), payload);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn anonymous_client_omits_apikey_and_blocks_validate() {
        let client = NexusClient::new_anonymous(Arc::new(RateLimitGate::new())).unwrap();
        assert!(client.is_anonymous());
        let err = client.validate().await.unwrap_err().to_string();
        assert!(err.contains("AUTH_REQUIRED"));

        let authed = NexusClient::new("test-key").unwrap();
        assert!(!authed.is_anonymous());
    }
}
