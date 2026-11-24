#![cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
//! Native on-device mod & collection catalog service for Android/mobile.
//!
//! Uses the encrypted Nexus Mods and mod.io API keys synced from the paired
//! desktop (plus public Nexus GraphQL and Thunderstore APIs) to browse mods,
//! search collections, inspect files, decode share codes, and check mod updates
//! directly from the mobile device — without waiting on or waking the desktop.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use flate2::read::DeflateDecoder;
use serde::Serialize;

use crate::modio_api::{ModioClient, ModioModHit};
use crate::nexus::{BrowseSearchOpts, ModSearchHit, NexusClient};
use crate::rate_limit::RateLimitGate;
use crate::remote::client::RemoteClientService;
use crate::thunderstore::{ThunderstoreClient, TsPackageDetail};

const BROWSE_CACHE_TTL: Duration = Duration::from_secs(90);
const DETAIL_CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_CACHE_ENTRIES: usize = 96;
const MAX_SHARE_CODE_BYTES: usize = 512 * 1024;
const MAX_SHARE_MANIFEST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
struct CacheEntry {
    value: serde_json::Value,
    expires_at: Instant,
}

#[derive(Debug, Clone, Serialize)]
struct MobileCatalogHit {
    source: &'static str,
    id: String,
    name: String,
    summary: Option<String>,
    picture_url: Option<String>,
    author: Option<String>,
    downloads: Option<u64>,
    endorsements: Option<u64>,
    category: Option<String>,
    tags: Vec<String>,
    mod_id: Option<u64>,
    domain_name: Option<String>,
    community: Option<String>,
    namespace: Option<String>,
    package_name: Option<String>,
    full_name: Option<String>,
    package_url: Option<String>,
    rating_score: Option<u64>,
    latest_version: Option<String>,
    modio_game_id: Option<u32>,
    modio_mod_id: Option<u64>,
    profile_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MobileCatalogSearchPage {
    items: Vec<MobileCatalogHit>,
    total_count: u64,
    next_offset: u32,
    has_more: bool,
    nexus_available: bool,
    thunderstore_available: bool,
    modio_available: bool,
}

struct GameCatalogMeta {
    nexus_domain: Option<String>,
    thunderstore_community: Option<String>,
    modio_game_id: Option<u32>,
    adult_content: bool,
}

pub struct MobileCatalogService {
    remote: Arc<RemoteClientService>,
    rate_limit: Arc<RateLimitGate>,
    ts_client: Mutex<Option<ThunderstoreClient>>,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl MobileCatalogService {
    pub fn new(remote: Arc<RemoteClientService>) -> Self {
        Self {
            remote,
            rate_limit: Arc::new(RateLimitGate::new()),
            ts_client: Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn nexus_client(&self) -> Result<NexusClient> {
        let key = self.remote.load_synced_nexus_api_key().ok().flatten();
        match key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(k) => NexusClient::new_with_gate(k, Arc::clone(&self.rate_limit)),
            None => NexusClient::new_anonymous(Arc::clone(&self.rate_limit)),
        }
    }

    fn modio_client(&self) -> Result<Option<ModioClient>> {
        let key = self.remote.load_synced_modio_api_key().ok().flatten();
        match key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(k) => Ok(Some(ModioClient::new(k)?)),
            None => Ok(None),
        }
    }

    fn thunderstore_client(&self) -> Result<ThunderstoreClient> {
        if let Ok(guard) = self.ts_client.lock() {
            if let Some(c) = guard.as_ref() {
                return Ok(c.clone());
            }
        }
        let client = ThunderstoreClient::new()?;
        if let Ok(mut guard) = self.ts_client.lock() {
            *guard = Some(client.clone());
        }
        Ok(client)
    }

    fn game_meta(&self, game_id: &str) -> Result<GameCatalogMeta> {
        let snap = self
            .remote
            .cached_snapshot()
            .ok_or_else(|| anyhow!("No synced desktop library yet"))?;
        let game = snap
            .managed_games
            .iter()
            .find(|g| g.get("id").and_then(|v| v.as_str()) == Some(game_id))
            .ok_or_else(|| anyhow!("Managed game not found in synced library"))?;

        let nexus_domain = game
            .get("nexus_domain")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let thunderstore_community = game
            .get("thunderstore_community")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let modio_game_id = game
            .get("modio_game_id")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);

        Ok(GameCatalogMeta {
            nexus_domain,
            thunderstore_community,
            modio_game_id,
            adult_content: snap.account.adult_content,
        })
    }

    fn cache_get(&self, key: &str) -> Option<serde_json::Value> {
        let mut guard = self.cache.lock().ok()?;
        let now = Instant::now();
        if let Some(entry) = guard.get(key) {
            if entry.expires_at > now {
                return Some(entry.value.clone());
            }
        }
        guard.remove(key);
        None
    }

    fn cache_put(&self, key: String, value: serde_json::Value, ttl: Duration) {
        let Ok(mut guard) = self.cache.lock() else {
            return;
        };
        let now = Instant::now();
        if guard.len() >= MAX_CACHE_ENTRIES {
            guard.retain(|_, v| v.expires_at > now);
            if guard.len() >= MAX_CACHE_ENTRIES {
                guard.clear();
            }
        }
        guard.insert(
            key,
            CacheEntry {
                value,
                expires_at: now + ttl,
            },
        );
    }

    /// Execute a catalog RPC natively on-device when supported.
    /// Returns `Ok(None)` for desktop-only methods that should fall back over `iroh`.
    pub async fn handle_rpc(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        let str_param = |k: &str| -> Option<String> {
            params.get(k).and_then(|v| v.as_str()).map(String::from)
        };
        let u64_param = |k: &str| -> Option<u64> { params.get(k).and_then(|v| v.as_u64()) };
        let u32_param = |k: &str| -> Option<u32> {
            params.get(k).and_then(|v| v.as_u64()).map(|n| n as u32)
        };

        let cache_key = format!("{method}:{}", params);
        let ttl = match method {
            "search_catalog" | "search_collection_catalog" => Some(BROWSE_CACHE_TTL),
            "get_mod"
            | "mod_files"
            | "get_collection"
            | "collection_files"
            | "get_thunderstore_package"
            | "get_modio_mod"
            | "modio_files"
            | "browse_meta" => Some(DETAIL_CACHE_TTL),
            _ => None,
        };
        if ttl.is_some() {
            if let Some(hit) = self.cache_get(&cache_key) {
                return Ok(Some(hit));
            }
        }

        let result: Option<serde_json::Value> = match method {
            "search_catalog" => {
                let game_id = str_param("game_id").ok_or_else(|| anyhow!("missing game_id"))?;
                let query = str_param("query").unwrap_or_default();
                let page = self
                    .search_catalog(
                        &game_id,
                        &query,
                        str_param("source_filter"),
                        str_param("sort"),
                        str_param("category"),
                        str_param("game_version"),
                        str_param("author"),
                        u32_param("offset"),
                        u32_param("count"),
                    )
                    .await?;
                Some(serde_json::to_value(page)?)
            }
            "search_collection_catalog" => {
                let game_id = str_param("game_id").ok_or_else(|| anyhow!("missing game_id"))?;
                let query = str_param("query").unwrap_or_default();
                let meta = self.game_meta(&game_id)?;
                let Some(domain) = meta.nexus_domain else {
                    bail!("No Nexus Mods domain configured for this game");
                };
                let opts = BrowseSearchOpts {
                    sort: str_param("sort").unwrap_or_else(|| "endorsements".into()),
                    category: str_param("category"),
                    tags_include: Vec::new(),
                    tags_exclude: Vec::new(),
                    game_version: str_param("game_version"),
                    author: str_param("author"),
                    offset: u32_param("offset").unwrap_or(0),
                    count: u32_param("count").unwrap_or(20),
                };
                let client = self.nexus_client()?;
                let page = client
                    .search_collections(&domain, &query, meta.adult_content, &opts)
                    .await?;
                let count_len = page.items.len() as u32;
                let next_offset = opts.offset.saturating_add(count_len);
                let has_more = (next_offset as u64) < page.total_count && count_len > 0;
                Some(serde_json::json!({
                    "items": page.items,
                    "nodes_count": page.nodes_count,
                    "total_count": page.total_count,
                    "next_offset": next_offset,
                    "has_more": has_more,
                    "nexus_available": true,
                    "thunderstore_available": false,
                }))
            }
            "get_mod" => {
                let domain = str_param("domain").ok_or_else(|| anyhow!("missing domain"))?;
                let mod_id = u64_param("mod_id").ok_or_else(|| anyhow!("missing mod_id"))?;
                let client = self.nexus_client()?;
                let detail = client.get_mod(&domain, mod_id).await?;
                Some(serde_json::to_value(detail)?)
            }
            "mod_files" => {
                let domain = str_param("domain").ok_or_else(|| anyhow!("missing domain"))?;
                let mod_id = u64_param("mod_id").ok_or_else(|| anyhow!("missing mod_id"))?;
                let client = self.nexus_client()?;
                let files = client.list_mod_files(&domain, mod_id).await?;
                Some(serde_json::to_value(files)?)
            }
            "get_collection" => {
                let slug = str_param("slug").ok_or_else(|| anyhow!("missing slug"))?;
                let domain = str_param("domain");
                let adult = self
                    .remote
                    .cached_snapshot()
                    .map(|s| s.account.adult_content)
                    .unwrap_or(false);
                let client = self.nexus_client()?;
                let detail = client
                    .get_collection(&slug, domain.as_deref(), adult)
                    .await?;
                Some(serde_json::to_value(detail)?)
            }
            "collection_files" => {
                let slug = str_param("slug").ok_or_else(|| anyhow!("missing slug"))?;
                let revision = params.get("revision").and_then(|v| v.as_i64());
                let adult = self
                    .remote
                    .cached_snapshot()
                    .map(|s| s.account.adult_content)
                    .unwrap_or(false);
                let client = self.nexus_client()?;
                let files = client.collection_mod_files(&slug, revision, adult).await?;
                Some(serde_json::to_value(files)?)
            }
            "get_thunderstore_package" => {
                let community =
                    str_param("community").ok_or_else(|| anyhow!("missing community"))?;
                let namespace =
                    str_param("namespace").ok_or_else(|| anyhow!("missing namespace"))?;
                let name = str_param("name").ok_or_else(|| anyhow!("missing name"))?;
                let client = self.thunderstore_client()?;
                let pkg = client.get_package(&community, &namespace, &name).await?;
                Some(serde_json::to_value(pkg)?)
            }
            "get_modio_mod" => {
                let game_id = u32_param("game_id").ok_or_else(|| anyhow!("missing game_id"))?;
                let mod_id = u64_param("mod_id").ok_or_else(|| anyhow!("missing mod_id"))?;
                let client = self
                    .modio_client()?
                    .ok_or_else(|| anyhow!("mod.io API key not synced from desktop"))?;
                let detail = client.get_mod(game_id, mod_id).await?;
                Some(serde_json::to_value(detail)?)
            }
            "modio_files" => {
                let game_id = u32_param("game_id").ok_or_else(|| anyhow!("missing game_id"))?;
                let mod_id = u64_param("mod_id").ok_or_else(|| anyhow!("missing mod_id"))?;
                let client = self
                    .modio_client()?
                    .ok_or_else(|| anyhow!("mod.io API key not synced from desktop"))?;
                let files = client.list_files(game_id, mod_id, None).await?;
                Some(serde_json::to_value(files)?)
            }
            "browse_meta" => {
                let domain = str_param("domain").ok_or_else(|| anyhow!("missing domain"))?;
                let adult = self
                    .remote
                    .cached_snapshot()
                    .map(|s| s.account.adult_content)
                    .unwrap_or(false);
                let client = self.nexus_client()?;
                let meta = client.browse_meta(&domain, adult).await?;
                Some(serde_json::to_value(meta)?)
            }
            "decode_share_code" => {
                let code = str_param("code").ok_or_else(|| anyhow!("missing code"))?;
                let manifest = decode_share_code_local(&code)?;
                Some(manifest)
            }
            _ => None,
        };

        if let (Some(val), Some(entry_ttl)) = (&result, ttl) {
            self.cache_put(cache_key, val.clone(), entry_ttl);
        }
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    async fn search_catalog(
        &self,
        game_id: &str,
        query: &str,
        source_filter: Option<String>,
        sort: Option<String>,
        category: Option<String>,
        game_version: Option<String>,
        author: Option<String>,
        offset: Option<u32>,
        count: Option<u32>,
    ) -> Result<MobileCatalogSearchPage> {
        let meta = self.game_meta(game_id)?;
        let filter = source_filter
            .unwrap_or_else(|| "all".into())
            .to_lowercase();
        let author_filter = author
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(String::from);

        let want_nexus = (filter == "all" || filter == "nexus") && meta.nexus_domain.is_some();
        let want_ts =
            (filter == "all" || filter == "thunderstore") && meta.thunderstore_community.is_some();
        let want_modio = (filter == "all" || filter == "modio")
            && meta.modio_game_id.is_some()
            && author_filter.is_none();

        if !want_nexus && !want_ts && !want_modio {
            bail!("No catalog sources are available for this game with the selected filter.");
        }

        let offset = offset.unwrap_or(0);
        let count = count.unwrap_or(24).max(1);
        let sort = sort.unwrap_or_else(|| {
            if query.trim().is_empty() {
                "downloads".into()
            } else {
                "relevance".into()
            }
        });

        let nexus_fut = async {
            if !want_nexus {
                return Ok::<(Vec<MobileCatalogHit>, u64), anyhow::Error>((Vec::new(), 0));
            }
            let domain = meta.nexus_domain.as_deref().unwrap_or_default();
            let client = self.nexus_client()?;
            let opts = BrowseSearchOpts {
                sort: sort.clone(),
                category: category.clone(),
                tags_include: Vec::new(),
                tags_exclude: Vec::new(),
                game_version: game_version.clone(),
                author: author_filter.clone(),
                offset,
                count,
            };
            let page = client
                .search_mods(domain, query, meta.adult_content, &opts)
                .await?;
            let hits = page
                .items
                .into_iter()
                .map(|m| hit_from_nexus(m, domain))
                .collect();
            Ok((hits, page.total_count))
        };

        let ts_fut = async {
            if !want_ts {
                return Ok::<(Vec<MobileCatalogHit>, u64), anyhow::Error>((Vec::new(), 0));
            }
            let community = meta.thunderstore_community.as_deref().unwrap_or_default();
            let client = self.thunderstore_client()?;
            let (pkgs, total) = client
                .search_packages(
                    community,
                    query,
                    meta.adult_content,
                    offset as usize,
                    count as usize,
                )
                .await?;
            let hits = pkgs.into_iter().map(hit_from_ts).collect();
            Ok((hits, total as u64))
        };

        let modio_fut = async {
            if !want_modio {
                return Ok::<(Vec<MobileCatalogHit>, u64), anyhow::Error>((Vec::new(), 0));
            }
            let Some(gid) = meta.modio_game_id else {
                return Ok((Vec::new(), 0));
            };
            let Some(client) = self.modio_client()? else {
                return Ok((Vec::new(), 0));
            };
            let (mods, total) = client
                .search_mods(gid, query, offset, count, meta.adult_content)
                .await?;
            let hits = mods.into_iter().map(hit_from_modio).collect();
            Ok((hits, total as u64))
        };

        let (nexus_res, ts_res, modio_res) = tokio::join!(nexus_fut, ts_fut, modio_fut);

        let mut items = Vec::new();
        let mut total_count: u64 = 0;
        let mut first_err: Option<anyhow::Error> = None;

        for res in [nexus_res, ts_res, modio_res] {
            match res {
                Ok((mut hits, total)) => {
                    items.append(&mut hits);
                    total_count = total_count.saturating_add(total);
                }
                Err(e) => {
                    log::warn!("mobile native catalog source failed: {e}");
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }

        if items.is_empty() && total_count == 0 {
            if let Some(err) = first_err {
                return Err(err);
            }
        }

        // Sort merged multi-source pages cleanly when browsing "all"
        if filter == "all" && items.len() > 1 {
            match sort.as_str() {
                "endorsements" | "rating" => {
                    items.sort_by(|a, b| {
                        b.endorsements
                            .unwrap_or(0)
                            .cmp(&a.endorsements.unwrap_or(0))
                    });
                }
                "downloads" => {
                    items.sort_by(|a, b| {
                        b.downloads.unwrap_or(0).cmp(&a.downloads.unwrap_or(0))
                    });
                }
                _ => {}
            }
        }

        let next_offset = offset.saturating_add(count);
        let has_more = (next_offset as u64) < total_count && !items.is_empty();

        Ok(MobileCatalogSearchPage {
            items,
            total_count,
            next_offset,
            has_more,
            nexus_available: meta.nexus_domain.is_some(),
            thunderstore_available: meta.thunderstore_community.is_some(),
            modio_available: meta.modio_game_id.is_some(),
        })
    }

    /// Check mod updates natively on-device for a game's synced staged mods.
    pub async fn check_mod_updates(&self, game_id: &str) -> Result<Vec<serde_json::Value>> {
        let snap = self
            .remote
            .cached_snapshot()
            .ok_or_else(|| anyhow!("No synced desktop library yet"))?;
        let mods = snap
            .mods_by_game
            .get(game_id)
            .cloned()
            .unwrap_or_default();
        if mods.is_empty() {
            return Ok(Vec::new());
        }

        let nexus = self.nexus_client().ok();
        let ts = self.thunderstore_client().ok();
        let modio = self.modio_client().ok().flatten();

        let mut updates = Vec::new();
        for m in mods {
            let Some(staged_id) = m.get("id").and_then(|v| v.as_str()).map(String::from) else {
                continue;
            };
            let current_version = m
                .get("version")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or("");
            let source = m.get("source").and_then(|v| v.as_str()).unwrap_or("");

            match source {
                "nexus" => {
                    let Some(client) = nexus.as_ref() else { continue };
                    let Some(domain) = m
                        .get("domain")
                        .or_else(|| m.get("nexus_domain"))
                        .and_then(|v| v.as_str())
                    else {
                        continue;
                    };
                    let Some(mod_id) = m.get("nexus_mod_id").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    let current_file_id = m.get("nexus_file_id").and_then(|v| v.as_u64());
                    if let Ok(detail) = client.get_mod(domain, mod_id).await {
                        let remote_ver = detail.version.as_deref().unwrap_or("").trim();
                        if !remote_ver.is_empty() && remote_ver != current_version {
                            let latest_file = client
                                .list_mod_files(domain, mod_id)
                                .await
                                .ok()
                                .and_then(|files| {
                                    files
                                        .into_iter()
                                        .find(|f| f.is_primary)
                                        .or(None)
                                });
                            let nexus_file_id = latest_file.as_ref().map(|f| f.file_id);
                            if current_file_id.is_some() && nexus_file_id == current_file_id {
                                continue;
                            }
                            updates.push(serde_json::json!({
                                "staged_id": staged_id,
                                "available_version": remote_ver,
                                "source": "nexus",
                                "nexus_file_id": nexus_file_id,
                                "file_name": latest_file.as_ref().map(|f| f.file_name.clone()),
                            }));
                        }
                    }
                }
                "thunderstore" => {
                    let Some(client) = ts.as_ref() else { continue };
                    let Some(community) = m.get("ts_community").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let Some(namespace) = m.get("ts_namespace").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let Some(name) = m.get("ts_name").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    if let Ok(pkg) = client.get_package(community, namespace, name).await {
                        let remote_ver = pkg.latest_version.as_deref().unwrap_or("").trim();
                        if !remote_ver.is_empty() && remote_ver != current_version {
                            updates.push(serde_json::json!({
                                "staged_id": staged_id,
                                "available_version": remote_ver,
                                "source": "thunderstore",
                                "ts_version": remote_ver,
                            }));
                        }
                    }
                }
                "modio" => {
                    let Some(client) = modio.as_ref() else { continue };
                    let Some(mgid) = m.get("modio_game_id").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    let Some(mid) = m.get("modio_mod_id").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    let current_file_id = m.get("modio_file_id").and_then(|v| v.as_u64());
                    if let Ok(files) = client.list_files(mgid as u32, mid, None).await {
                        if let Some(latest) = files.into_iter().find(|f| f.is_primary) {
                            let ver_differs = latest
                                .version
                                .as_deref()
                                .map(|v| !v.trim().is_empty() && v.trim() != current_version)
                                .unwrap_or(false);
                            let file_differs =
                                current_file_id.map(|id| id != latest.file_id).unwrap_or(true);
                            if ver_differs || file_differs {
                                updates.push(serde_json::json!({
                                    "staged_id": staged_id,
                                    "available_version": latest.version,
                                    "source": "modio",
                                    "modio_file_id": latest.file_id,
                                    "file_name": latest.filename,
                                }));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(updates)
    }
}

fn hit_from_nexus(m: ModSearchHit, fallback_domain: &str) -> MobileCatalogHit {
    let domain = if m.domain_name.is_empty() {
        fallback_domain.to_string()
    } else {
        m.domain_name
    };
    MobileCatalogHit {
        source: "nexus",
        id: format!("nexus:{domain}:{}", m.mod_id),
        name: m.name,
        summary: m.summary,
        picture_url: m.picture_url,
        author: m.author,
        downloads: m.downloads,
        endorsements: m.endorsements,
        category: m.category,
        tags: m.tags,
        mod_id: Some(m.mod_id),
        domain_name: Some(domain),
        community: None,
        namespace: None,
        package_name: None,
        full_name: None,
        package_url: None,
        rating_score: None,
        latest_version: None,
        modio_game_id: None,
        modio_mod_id: None,
        profile_url: None,
    }
}

fn hit_from_ts(p: TsPackageDetail) -> MobileCatalogHit {
    let rating = p.rating_score.max(0) as u64;
    MobileCatalogHit {
        source: "thunderstore",
        id: format!("ts:{}:{}:{}", p.community, p.namespace, p.name),
        name: p.name.clone(),
        summary: p.description.filter(|s| !s.is_empty()),
        picture_url: p.icon_url.filter(|s| !s.is_empty()),
        author: Some(p.namespace.clone()),
        downloads: Some(p.downloads),
        endorsements: Some(rating),
        category: p.categories.first().cloned(),
        tags: p.categories,
        mod_id: None,
        domain_name: None,
        community: Some(p.community),
        namespace: Some(p.namespace),
        package_name: Some(p.name),
        full_name: Some(p.full_name),
        package_url: Some(p.package_url),
        rating_score: Some(rating),
        latest_version: p.latest_version.filter(|s| !s.is_empty()),
        modio_game_id: None,
        modio_mod_id: None,
        profile_url: None,
    }
}

fn hit_from_modio(m: ModioModHit) -> MobileCatalogHit {
    MobileCatalogHit {
        source: "modio",
        id: format!("modio:{}:{}", m.game_id, m.mod_id),
        name: m.name,
        summary: if m.summary.is_empty() {
            None
        } else {
            Some(m.summary)
        },
        picture_url: m.picture_url,
        author: m.author,
        downloads: Some(m.downloads),
        endorsements: None,
        category: m.tags.first().cloned(),
        tags: m.tags,
        mod_id: None,
        domain_name: None,
        community: None,
        namespace: None,
        package_name: None,
        full_name: None,
        package_url: None,
        rating_score: None,
        latest_version: None,
        modio_game_id: Some(m.game_id),
        modio_mod_id: Some(m.mod_id),
        profile_url: if m.profile_url.is_empty() {
            None
        } else {
            Some(m.profile_url)
        },
    }
}

fn decode_share_code_local(raw: &str) -> Result<serde_json::Value> {
    let trimmed = raw.trim_start_matches('\u{feff}').trim();
    if trimmed.len() > MAX_SHARE_CODE_BYTES {
        bail!("share code exceeds maximum allowed size");
    }
    let encoded = trimmed
        .strip_prefix("#emperor1:")
        .or_else(|| {
            trimmed
                .strip_prefix("#emperor1")
                .map(|s| s.trim_start_matches(['\n', '\r', ' ']))
        })
        .ok_or_else(|| anyhow!("not an Emperor share code (missing #emperor1 prefix)"))?;
    let compact: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
    let compressed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(compact.as_bytes())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(compact.as_bytes()))
        .context("decode share payload")?;
    let decoder = DeflateDecoder::new(compressed.as_slice());
    let mut json = Vec::new();
    decoder
        .take((MAX_SHARE_MANIFEST_BYTES + 1) as u64)
        .read_to_end(&mut json)
        .context("inflate share payload")?;
    if json.len() > MAX_SHARE_MANIFEST_BYTES {
        bail!("share manifest exceeds maximum size");
    }
    let val: serde_json::Value =
        serde_json::from_slice(&json).context("parse share manifest JSON")?;
    Ok(val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::Write;

    #[test]
    fn decode_share_code_local_roundtrip() {
        let payload = serde_json::json!({
            "version": 1,
            "name": "Test Pack",
            "mods": []
        });
        let raw_json = serde_json::to_vec(&payload).unwrap();
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw_json).unwrap();
        let compressed = enc.finish().unwrap();
        let encoded = format!(
            "#emperor1:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&compressed)
        );

        let decoded = decode_share_code_local(&encoded).unwrap();
        assert_eq!(decoded["name"], "Test Pack");
        assert_eq!(decoded["version"], 1);
    }

    #[test]
    fn decode_share_code_local_rejects_invalid_prefix() {
        assert!(decode_share_code_local("not-a-share-code").is_err());
    }
}
