//! Thunderstore community package API client.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use reqwest::header::{
    HeaderMap, HeaderValue, IF_MODIFIED_SINCE, LAST_MODIFIED, LOCATION, USER_AGENT,
};
use serde::{Deserialize, Serialize};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use crate::config::{APP_NAME, APP_VERSION};
use crate::nexus::{TransferControl, CANCELLED_MSG, PAUSED_MSG};
use walkdir::WalkDir;

const TS_BASE: &str = "https://thunderstore.io";
const TS_CHUNK_CONCURRENCY: usize = 4;

#[derive(Clone)]
pub struct ThunderstoreClient {
    http: reqwest::Client,
    /// Does not follow redirects, so a listing-index 302 keeps its Last-Modified.
    http_noredir: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsPackageVersion {
    pub name: String,
    pub full_name: String,
    pub description: String,
    pub icon: String,
    pub version_number: String,
    pub dependencies: Vec<String>,
    pub download_url: String,
    pub downloads: u64,
    pub date_created: String,
    #[serde(default)]
    pub website_url: String,
    pub is_active: bool,
    pub uuid4: String,
    pub file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsPackage {
    pub name: String,
    pub full_name: String,
    pub owner: String,
    pub package_url: String,
    pub date_created: String,
    pub date_updated: String,
    pub uuid4: String,
    pub rating_score: i64,
    pub is_pinned: bool,
    pub is_deprecated: bool,
    pub has_nsfw_content: bool,
    pub categories: Vec<String>,
    pub versions: Vec<TsPackageVersion>,
}

/// Latest-version fields needed for browse cards. Version history stays on `TsPackage`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TsBrowsePackage {
    pub name: String,
    pub full_name: String,
    pub owner: String,
    pub package_url: String,
    pub uuid4: String,
    pub rating_score: i64,
    pub is_pinned: bool,
    pub is_deprecated: bool,
    pub has_nsfw_content: bool,
    pub categories: Vec<String>,
    pub description: String,
    pub icon: String,
    pub downloads: u64,
    pub latest_version: String,
    pub dependency_count: u64,
    pub file_size: u64,
}

/// Compact browse snapshot plus the chunk URLs that produced the full package list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TsBrowseDiskCache {
    pub last_modified: Option<String>,
    pub chunk_urls: Vec<String>,
    pub packages: Vec<TsBrowsePackage>,
}

/// Community package list fetched from the listing index or the v1 fallback.
#[derive(Debug, Clone)]
pub struct TsCommunityListing {
    pub last_modified: Option<String>,
    pub chunk_urls: Vec<String>,
    pub packages: Vec<TsPackage>,
}

#[derive(Debug, Clone)]
pub enum TsListingFetch {
    /// Index replied 304. The caller already has this community's snapshot.
    NotModified,
    Fresh(TsCommunityListing),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsCommunity {
    pub identifier: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct TsCommunityPage {
    pagination: TsCommunityPagination,
    results: Vec<TsCommunity>,
}

#[derive(Debug, Deserialize)]
struct TsCommunityPagination {
    next_link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsPackageDetail {
    pub community: String,
    pub namespace: String,
    pub name: String,
    pub full_name: String,
    pub package_url: String,
    pub uuid4: String,
    pub rating_score: i64,
    pub is_deprecated: bool,
    pub has_nsfw_content: bool,
    pub categories: Vec<String>,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub versions: Vec<TsPackageVersion>,
    pub latest_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct TsManifest {
    pub name: String,
    #[serde(default)]
    pub version_number: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Parsed Thunderstore dependency string: `Namespace-Name-Version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsDependency {
    pub namespace: String,
    pub name: String,
    pub version: String,
}

impl TsDependency {
    pub fn parse(raw: &str) -> Option<Self> {
        // Format is Owner-Name-X.Y.Z (name may contain hyphens).
        let parts: Vec<&str> = raw.split('-').collect();
        if parts.len() < 3 {
            return None;
        }
        // Prefer last segment with a '.' as the version start; else last segment.
        let ver_idx = parts
            .iter()
            .rposition(|p| p.contains('.'))
            .filter(|&i| i >= 2)
            .unwrap_or(parts.len() - 1);
        if ver_idx < 2 {
            return None;
        }
        let namespace = parts[0].to_string();
        let name = parts[1..ver_idx].join("-");
        let version = parts[ver_idx..].join("-");
        if namespace.is_empty() || name.is_empty() || version.is_empty() {
            return None;
        }
        Some(Self {
            namespace,
            name,
            version,
        })
    }

    pub fn key(&self) -> String {
        format!("{}-{}", self.namespace, self.name)
    }
}

fn build_http(follow_redirects: bool) -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&format!("{APP_NAME}/{APP_VERSION}"))?,
    );
    let mut builder = reqwest::Client::builder().default_headers(headers);
    if !follow_redirects {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }
    Ok(builder.build()?)
}

impl ThunderstoreClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: build_http(true)?,
            http_noredir: build_http(false)?,
        })
    }

    pub async fn list_communities(&self) -> Result<Vec<TsCommunity>> {
        let mut url = format!("{TS_BASE}/api/experimental/community/");
        let mut out = Vec::new();
        loop {
            let resp = self
                .http
                .get(&url)
                .send()
                .await
                .with_context(|| format!("GET {url}"))?;
            if !resp.status().is_success() {
                bail!("Thunderstore list communities failed: {}", resp.status());
            }
            let page: TsCommunityPage = resp.json().await.context("decode community list")?;
            out.extend(page.results);
            match page.pagination.next_link {
                Some(next) if !next.is_empty() => url = next,
                _ => break,
            }
        }
        Ok(out)
    }

    pub async fn list_packages(&self, community: &str) -> Result<Vec<TsPackage>> {
        match self.fetch_community_listing(community, None).await? {
            TsListingFetch::NotModified => Ok(Vec::new()),
            TsListingFetch::Fresh(listing) => Ok(listing.packages),
        }
    }

    /// Load a community's packages from the chunked listing index.
    ///
    /// `if_modified_since` is the previous index `Last-Modified` value. A 304
    /// returns [`TsListingFetch::NotModified`] and does not download chunks.
    /// A missing index (404/503) or a bad chunk falls back to `/api/v1/package/`.
    pub async fn fetch_community_listing(
        &self,
        community: &str,
        if_modified_since: Option<&str>,
    ) -> Result<TsListingFetch> {
        match self.fetch_listing_index(community, if_modified_since).await {
            Ok(fetch) => Ok(fetch),
            Err(err) => {
                log::warn!(
                    "Thunderstore listing index unavailable for {community}, using package dump: {err:#}"
                );
                let packages = self.list_packages_v1(community).await?;
                Ok(TsListingFetch::Fresh(TsCommunityListing {
                    last_modified: None,
                    chunk_urls: Vec::new(),
                    packages,
                }))
            }
        }
    }

    async fn fetch_listing_index(
        &self,
        community: &str,
        if_modified_since: Option<&str>,
    ) -> Result<TsListingFetch> {
        let index_url = format!("{TS_BASE}/c/{community}/api/v1/package-listing-index/");
        let mut req = self.http_noredir.get(&index_url);
        if let Some(since) = if_modified_since.map(str::trim).filter(|s| !s.is_empty()) {
            req = req.header(IF_MODIFIED_SINCE, since);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("GET {index_url}"))?;
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(TsListingFetch::NotModified);
        }
        if resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || resp.status() == reqwest::StatusCode::NOT_FOUND
        {
            bail!("Thunderstore listing index unavailable: {}", resp.status());
        }

        let last_modified = header_string(resp.headers(), LAST_MODIFIED);
        let chunk_urls = if resp.status().is_redirection() {
            let loc = header_string(resp.headers(), LOCATION)
                .context("listing index redirect missing Location")?;
            let base = reqwest::Url::parse(&index_url)?;
            let next = base
                .join(&loc)
                .with_context(|| format!("join listing index redirect {loc}"))?;
            let next = next.to_string();
            if !trusted_thunderstore_url(&next) {
                bail!("untrusted listing index url: {next}");
            }
            self.read_chunk_urls(&next).await?
        } else if resp.status().is_success() {
            resp.json().await.context("decode listing index")?
        } else {
            bail!("Thunderstore listing index failed: {}", resp.status());
        };
        for url in &chunk_urls {
            if !trusted_thunderstore_url(url) {
                bail!("untrusted package chunk url: {url}");
            }
        }
        let packages = self.download_package_chunks(&chunk_urls).await?;
        Ok(TsListingFetch::Fresh(TsCommunityListing {
            last_modified,
            chunk_urls,
            packages,
        }))
    }

    async fn read_chunk_urls(&self, url: &str) -> Result<Vec<String>> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if !resp.status().is_success() {
            bail!("Thunderstore listing index blob failed: {}", resp.status());
        }
        resp.json().await.context("decode listing index")
    }

    pub async fn download_package_chunks(&self, urls: &[String]) -> Result<Vec<TsPackage>> {
        if urls.is_empty() {
            return Ok(Vec::new());
        }
        let http = self.http.clone();
        let mut stream = futures_util::stream::iter(urls.iter().cloned())
            .map(|url| {
                let http = http.clone();
                async move { fetch_package_chunk(http, url).await }
            })
            .buffer_unordered(TS_CHUNK_CONCURRENCY);
        let mut packages = Vec::new();
        while let Some(chunk) = stream.next().await {
            packages.extend(chunk?);
        }
        Ok(packages)
    }

    async fn list_packages_v1(&self, community: &str) -> Result<Vec<TsPackage>> {
        let url = format!("{TS_BASE}/c/{community}/api/v1/package/");
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if !resp.status().is_success() {
            bail!("Thunderstore list packages failed: {}", resp.status());
        }
        let packages: Vec<TsPackage> = resp.json().await.context("decode package list")?;
        Ok(packages)
    }

    pub async fn get_package(
        &self,
        community: &str,
        namespace: &str,
        name: &str,
    ) -> Result<TsPackageDetail> {
        let packages = self.list_packages(community).await?;
        let pkg = packages
            .into_iter()
            .find(|p| p.owner.eq_ignore_ascii_case(namespace) && p.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow!("Package {namespace}-{name} not found in {community}"))?;
        Ok(detail_from_package(community, pkg))
    }

    #[allow(dead_code)]
    pub async fn search_packages(
        &self,
        community: &str,
        query: &str,
        include_nsfw: bool,
        offset: usize,
        count: usize,
    ) -> Result<(Vec<TsPackageDetail>, usize)> {
        let mut packages = self.list_packages(community).await?;
        packages.retain(|p| !p.is_deprecated);
        if !include_nsfw {
            packages.retain(|p| !p.has_nsfw_content);
        }
        let q = query.trim().to_lowercase();
        if !q.is_empty() {
            packages.retain(|p| {
                p.name.to_lowercase().contains(&q)
                    || p.full_name.to_lowercase().contains(&q)
                    || p.owner.to_lowercase().contains(&q)
                    || p.versions
                        .first()
                        .map(|v| v.description.to_lowercase().contains(&q))
                        .unwrap_or(false)
                    || p.categories.iter().any(|c| c.to_lowercase().contains(&q))
            });
        }
        // Sort by latest version downloads desc, pinned first
        packages.sort_by(|a, b| {
            b.is_pinned
                .cmp(&a.is_pinned)
                .then_with(|| {
                    let da = a.versions.first().map(|v| v.downloads).unwrap_or(0);
                    let db = b.versions.first().map(|v| v.downloads).unwrap_or(0);
                    db.cmp(&da)
                })
                .then_with(|| b.rating_score.cmp(&a.rating_score))
        });
        let total = packages.len();
        let page: Vec<TsPackageDetail> = packages
            .into_iter()
            .skip(offset)
            .take(count)
            .map(|p| detail_from_package(community, p))
            .collect();
        Ok((page, total))
    }

    /// Resolve dependency closure (dependencies first). `root` is the leaf package.
    ///
    /// Thunderstore dependency versions are **minimums** (`Owner-Name-1.2.3`),
    /// matching r2modman / Gale: install the newest published version that still
    /// satisfies every required floor. When two packages require the same
    /// dependency, the higher floor wins. An explicit `version` for the root
    /// package is treated as an exact pin (profile / modpack entries).
    #[allow(dead_code)]
    pub async fn resolve_install_order(
        &self,
        community: &str,
        namespace: &str,
        name: &str,
        version: Option<&str>,
    ) -> Result<Vec<TsPackageDetail>> {
        let packages = self.list_packages(community).await?;
        resolve_install_order_from(&packages, community, namespace, name, version)
    }

    pub async fn download_version(
        &self,
        version: &TsPackageVersion,
        dest: &Path,
        control: Option<&TransferControl>,
        on_progress: Option<&Arc<dyn Fn(u64, Option<u64>, u64) + Send + Sync>>,
    ) -> Result<()> {
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let resp = self
            .http
            .get(&version.download_url)
            .send()
            .await
            .with_context(|| format!("GET {}", version.download_url))?;
        if !resp.status().is_success() {
            bail!("Thunderstore download failed: {}", resp.status());
        }
        let total = resp
            .content_length()
            .or_else(|| (version.file_size > 0).then_some(version.file_size));
        let mut stream = resp.bytes_stream();
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(dest)
            .await?;
        let mut downloaded: u64 = 0;
        let started = Instant::now();
        let mut last_report = started;
        while let Some(chunk) = stream.next().await {
            if let Some(ctrl) = control {
                if ctrl.cancel.load(Ordering::Relaxed) {
                    bail!(CANCELLED_MSG);
                }
                if ctrl.pause.load(Ordering::Relaxed) {
                    file.flush().await?;
                    if let Some(cb) = on_progress {
                        cb(downloaded, total, 0);
                    }
                    bail!(PAUSED_MSG);
                }
            }
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            let now = Instant::now();
            if now.duration_since(last_report).as_millis() >= 150 {
                if let Some(cb) = on_progress {
                    let elapsed = started.elapsed().as_secs_f64().max(0.001);
                    cb(downloaded, total, (downloaded as f64 / elapsed) as u64);
                }
                last_report = now;
            }
        }
        file.flush().await?;
        if let Some(cb) = on_progress {
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            cb(
                downloaded,
                total.or(Some(downloaded)),
                (downloaded as f64 / elapsed) as u64,
            );
        }
        Ok(())
    }
}

/// True when `version` is semver-equal to or newer than `min`.
fn version_satisfies_min(version: &str, min: &str) -> bool {
    let version = version.trim();
    let min = min.trim();
    if version.is_empty() || min.is_empty() {
        return false;
    }
    version == min || crate::mods::incoming_is_newer(Some(min), version)
}

/// Merge a newly requested minimum into the recorded floor (highest wins).
/// Returns true when the floor was inserted or raised.
fn raise_version_floor(
    floors: &mut HashMap<String, String>,
    key: &str,
    wanted: Option<&str>,
) -> bool {
    let Some(wanted) = wanted.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    match floors.get(key).map(|s| s.as_str()) {
        Some(existing) if !crate::mods::incoming_is_newer(Some(existing), wanted) => false,
        _ => {
            floors.insert(key.to_string(), wanted.to_string());
            true
        }
    }
}

/// Highest minimum version required per package across the dependency closure.
///
/// The map stores **floors**, not the version that will be installed. Install
/// selection (except an exact root pin) picks the newest published release that
/// still satisfies the floor — see [`select_latest_satisfying`].
fn resolve_dependency_pins(
    root_key: &str,
    root_version: Option<&str>,
    by_key: &HashMap<String, TsPackage>,
) -> Result<HashMap<String, String>> {
    let mut floors: HashMap<String, String> = HashMap::new();
    let mut walked: HashSet<String> = HashSet::new();
    let mut pending: Vec<(String, Option<String>)> =
        vec![(root_key.to_string(), root_version.map(str::to_string))];

    while let Some((key, wanted)) = pending.pop() {
        // Unknown packages are reported when building the install order.
        let Some(pkg) = by_key.get(&key) else {
            continue;
        };
        let floor_raised = raise_version_floor(&mut floors, &key, wanted.as_deref());
        let first_walk = walked.insert(key.clone());
        if !first_walk && !floor_raised {
            continue;
        }
        let ver = if key == root_key {
            select_root_version(pkg, root_version)?
        } else {
            select_latest_satisfying(pkg, floors.get(&key).map(String::as_str))?
        };
        if let Some(floor) = floors.get(&key) {
            if !version_satisfies_min(&ver.version_number, floor) {
                bail!("No version of {} satisfies minimum {floor}", pkg.full_name);
            }
        }
        for dep_raw in &ver.dependencies {
            if let Some(dep) = TsDependency::parse(dep_raw) {
                pending.push((
                    format!("{}-{}", dep.namespace, dep.name).to_lowercase(),
                    Some(dep.version),
                ));
            }
        }
    }
    Ok(floors)
}

/// Emit packages dependencies-first, each at its resolved version.
fn collect_install_order(
    key: &str,
    root_key: &str,
    root_exact: Option<&str>,
    pins: &HashMap<String, String>,
    by_key: &HashMap<String, TsPackage>,
    community: &str,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    ordered: &mut Vec<TsPackageDetail>,
) -> Result<()> {
    let key_l = key.to_lowercase();
    if visited.contains(&key_l) {
        return Ok(());
    }
    if !visiting.insert(key_l.clone()) {
        // Soft cycles are common on Thunderstore (e.g. BepInExPack ↔ RoR2BepInExPack).
        // Skip the back-edge instead of aborting the whole install.
        log::warn!("Skipping circular Thunderstore dependency involving {key}");
        return Ok(());
    }
    let pkg = by_key
        .get(&key_l)
        .ok_or_else(|| anyhow!("Missing dependency package {key}"))?;
    let ver = if key_l == root_key {
        select_exact_or_latest(pkg, root_exact)?
    } else {
        select_latest_satisfying(pkg, pins.get(&key_l).map(String::as_str))?
    };
    for dep_raw in &ver.dependencies {
        if let Some(dep) = TsDependency::parse(dep_raw) {
            let dep_key = format!("{}-{}", dep.namespace, dep.name);
            collect_install_order(
                &dep_key, root_key, root_exact, pins, by_key, community, visiting, visited, ordered,
            )?;
        }
    }
    visiting.remove(&key_l);
    visited.insert(key_l);

    let mut detail = detail_from_package(community, pkg.clone());
    // Put the resolved version first so staging downloads that one.
    detail.latest_version = Some(ver.version_number.clone());
    if let Some(pos) = detail
        .versions
        .iter()
        .position(|v| v.version_number == ver.version_number)
    {
        let selected = detail.versions.remove(pos);
        detail.versions.insert(0, selected);
    }
    detail.description = Some(ver.description.clone());
    detail.icon_url = Some(ver.icon.clone());
    detail.downloads = ver.downloads;
    ordered.push(detail);
    Ok(())
}

/// Exact version when requested (profile / modpack root); otherwise latest.
fn select_root_version<'a>(
    pkg: &'a TsPackage,
    root_version: Option<&str>,
) -> Result<&'a TsPackageVersion> {
    select_exact_or_latest(pkg, root_version)
}

fn select_exact_or_latest<'a>(
    pkg: &'a TsPackage,
    preferred: Option<&str>,
) -> Result<&'a TsPackageVersion> {
    if let Some(v) = preferred.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(found) = pkg.versions.iter().find(|x| x.version_number == v) {
            return Ok(found);
        }
        // Prefer latest if exact version missing
    }
    pkg.versions
        .first()
        .ok_or_else(|| anyhow!("Package {} has no versions", pkg.full_name))
}

/// Newest published version that still satisfies an optional minimum floor.
///
/// Thunderstore lists versions newest-first, so the first match is the latest.
fn select_latest_satisfying<'a>(
    pkg: &'a TsPackage,
    min: Option<&str>,
) -> Result<&'a TsPackageVersion> {
    let Some(min) = min.map(str::trim).filter(|s| !s.is_empty()) else {
        return pkg
            .versions
            .first()
            .ok_or_else(|| anyhow!("Package {} has no versions", pkg.full_name));
    };
    if let Some(found) = pkg
        .versions
        .iter()
        .find(|v| version_satisfies_min(&v.version_number, min))
    {
        return Ok(found);
    }
    bail!("No version of {} satisfies minimum {min}", pkg.full_name)
}

pub fn resolve_install_order_from(
    packages: &[TsPackage],
    community: &str,
    namespace: &str,
    name: &str,
    version: Option<&str>,
) -> Result<Vec<TsPackageDetail>> {
    let by_key: HashMap<String, TsPackage> = packages
        .iter()
        .cloned()
        .map(|p| (format!("{}-{}", p.owner, p.name).to_lowercase(), p))
        .collect();

    let root_key = format!("{namespace}-{name}").to_lowercase();
    if !by_key.contains_key(&root_key) {
        bail!("Package {namespace}-{name} not found");
    }

    let pins = resolve_dependency_pins(&root_key, version, &by_key)?;

    let mut ordered: Vec<TsPackageDetail> = Vec::new();
    let mut visiting: HashSet<String> = HashSet::new();
    let mut visited: HashSet<String> = HashSet::new();
    collect_install_order(
        &root_key,
        &root_key,
        version,
        &pins,
        &by_key,
        community,
        &mut visiting,
        &mut visited,
        &mut ordered,
    )?;
    Ok(ordered)
}

pub fn packages_to_browse(packages: &[TsPackage]) -> Vec<TsBrowsePackage> {
    packages.iter().map(browse_from_package).collect()
}

pub fn browse_from_package(pkg: &TsPackage) -> TsBrowsePackage {
    let latest = pkg.versions.first();
    TsBrowsePackage {
        name: pkg.name.clone(),
        full_name: pkg.full_name.clone(),
        owner: pkg.owner.clone(),
        package_url: pkg.package_url.clone(),
        uuid4: pkg.uuid4.clone(),
        rating_score: pkg.rating_score,
        is_pinned: pkg.is_pinned,
        is_deprecated: pkg.is_deprecated,
        has_nsfw_content: pkg.has_nsfw_content,
        categories: pkg.categories.clone(),
        description: latest.map(|v| v.description.clone()).unwrap_or_default(),
        icon: latest.map(|v| v.icon.clone()).unwrap_or_default(),
        downloads: latest.map(|v| v.downloads).unwrap_or(0),
        latest_version: latest.map(|v| v.version_number.clone()).unwrap_or_default(),
        dependency_count: latest.map(|v| v.dependencies.len() as u64).unwrap_or(0),
        file_size: latest.map(|v| v.file_size).unwrap_or(0),
    }
}

/// Page a browse snapshot without cloning the packages that are not returned.
pub fn page_browse_packages<'a>(
    packages: &'a [TsBrowsePackage],
    query: &str,
    author: Option<&str>,
    include_nsfw: bool,
    modpacks_only: bool,
    offset: usize,
    count: usize,
) -> (Vec<&'a TsBrowsePackage>, usize) {
    let author = author.map(str::trim).filter(|s| !s.is_empty());
    let q = query.trim().to_lowercase();
    let mut idxs: Vec<usize> = packages
        .iter()
        .enumerate()
        .filter(|(_, pkg)| {
            if pkg.is_deprecated {
                return false;
            }
            if !include_nsfw && pkg.has_nsfw_content {
                return false;
            }
            if modpacks_only && !is_modpack_category(&pkg.categories) {
                return false;
            }
            if let Some(author) = author {
                if !pkg.owner.eq_ignore_ascii_case(author) {
                    return false;
                }
            }
            if q.is_empty() {
                return true;
            }
            pkg.name.to_lowercase().contains(&q)
                || pkg.full_name.to_lowercase().contains(&q)
                || pkg.owner.to_lowercase().contains(&q)
                || pkg.description.to_lowercase().contains(&q)
                || pkg.categories.iter().any(|c| c.to_lowercase().contains(&q))
        })
        .map(|(idx, _)| idx)
        .collect();
    idxs.sort_by(|&ia, &ib| {
        let a = &packages[ia];
        let b = &packages[ib];
        b.is_pinned
            .cmp(&a.is_pinned)
            .then_with(|| b.downloads.cmp(&a.downloads))
    });
    let total = idxs.len();
    let page = idxs
        .into_iter()
        .skip(offset)
        .take(count)
        .map(|idx| &packages[idx])
        .collect();
    (page, total)
}

pub fn browse_cache_path(cache_dir: &Path, community: &str) -> Option<PathBuf> {
    if !community_cache_slug(community) {
        return None;
    }
    Some(
        cache_dir
            .join("thunderstore")
            .join(format!("{community}.browse.json")),
    )
}

pub fn encode_browse_disk(cache: &TsBrowseDiskCache) -> Result<Vec<u8>> {
    serde_json::to_vec(cache).context("encode thunderstore browse cache")
}

pub fn decode_browse_disk(bytes: &[u8]) -> Result<TsBrowseDiskCache> {
    serde_json::from_slice(bytes).context("decode thunderstore browse cache")
}

fn community_cache_slug(community: &str) -> bool {
    !community.is_empty()
        && community.len() <= 128
        && community
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn trusted_thunderstore_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("thunderstore.io")
        || host.to_ascii_lowercase().ends_with(".thunderstore.io")
}

fn header_string(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

async fn fetch_package_chunk(http: reqwest::Client, url: String) -> Result<Vec<TsPackage>> {
    if !trusted_thunderstore_url(&url) {
        bail!("untrusted package chunk url: {url}");
    }
    let resp = http
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        bail!("Thunderstore package chunk failed: {}", resp.status());
    }
    resp.json().await.context("decode package chunk")
}

pub fn detail_from_package(community: &str, pkg: TsPackage) -> TsPackageDetail {
    let latest = pkg.versions.first();
    TsPackageDetail {
        community: community.to_string(),
        namespace: pkg.owner.clone(),
        name: pkg.name.clone(),
        full_name: pkg.full_name.clone(),
        package_url: pkg.package_url.clone(),
        uuid4: pkg.uuid4.clone(),
        rating_score: pkg.rating_score,
        is_deprecated: pkg.is_deprecated,
        has_nsfw_content: pkg.has_nsfw_content,
        categories: pkg.categories.clone(),
        description: latest.map(|v| v.description.clone()),
        icon_url: latest.map(|v| v.icon.clone()),
        downloads: latest.map(|v| v.downloads).unwrap_or(0),
        latest_version: latest.map(|v| v.version_number.clone()),
        versions: pkg.versions,
    }
}

pub fn is_modpack_category(categories: &[String]) -> bool {
    categories
        .iter()
        .any(|c| c.to_lowercase().contains("modpack"))
}

const PROFILE_DATA_PREFIX: &str = "#r2modman\n";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileManifest {
    #[serde(rename = "profileName", alias = "name")]
    pub profile_name: String,
    #[serde(default)]
    pub mods: Vec<ProfileMod>,
    #[serde(default, alias = "game_slug")]
    pub community: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileMod {
    pub name: String,
    #[serde(alias = "versionNumber")]
    pub version: ProfileVersion,
    #[serde(default = "profile_mod_enabled")]
    pub enabled: bool,
}

fn profile_mod_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ProfileVersion {
    Parts { major: u64, minor: u64, patch: u64 },
    String(String),
}

impl ProfileVersion {
    pub fn as_string(&self) -> String {
        match self {
            ProfileVersion::Parts {
                major,
                minor,
                patch,
            } => format!("{major}.{minor}.{patch}"),
            ProfileVersion::String(s) => s.clone(),
        }
    }
}

/// `Owner-PackageName` without a version suffix.
pub fn parse_package_ident(name: &str) -> Option<(String, String)> {
    let (ns, rest) = name.split_once('-')?;
    if ns.is_empty() || rest.is_empty() {
        return None;
    }
    Some((ns.to_string(), rest.to_string()))
}

pub fn community_slug_matches(profile: &str, community: &str) -> bool {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect::<String>()
    };
    norm(profile) == norm(community)
}

impl ThunderstoreClient {
    pub async fn fetch_legacy_profile(&self, key: &str) -> Result<Vec<u8>> {
        let key = key.trim();
        let url = format!("{TS_BASE}/api/experimental/legacyprofile/get/{key}/");
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("profile code is expired or invalid");
        }
        if !resp.status().is_success() {
            bail!("Thunderstore profile fetch failed: {}", resp.status());
        }
        let text = resp.text().await.context("read profile payload")?;
        let encoded = text
            .strip_prefix(PROFILE_DATA_PREFIX)
            .or_else(|| text.trim_start().strip_prefix(PROFILE_DATA_PREFIX))
            .ok_or_else(|| anyhow!("invalid profile data"))?;
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .context("decode profile payload")
    }
}

pub fn extract_profile_zip(bytes: &[u8], dest: &Path) -> Result<ProfileManifest> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("open profile zip")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let raw_name = file.name();
        let is_dir = raw_name.ends_with('/') || raw_name.ends_with('\\');
        let rel = match crate::mods::safe_archive_entry_path(raw_name.trim_end_matches(['/', '\\']))
        {
            Some(p) => p,
            None => continue,
        };
        let outpath = dest.join(rel);
        if is_dir {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    crate::mods::repair_backslash_entries(dest)?;
    let manifest_path = dest.join("export.r2x");
    let reader = fs::File::open(&manifest_path).context("missing export.r2x in profile")?;
    let manifest: ProfileManifest = serde_yaml::from_reader(reader).context("parse export.r2x")?;
    let _ = fs::remove_file(&manifest_path);
    Ok(manifest)
}

pub fn profile_has_overlay_files(root: &Path) -> bool {
    WalkDir::new(root).into_iter().flatten().any(|e| {
        e.file_type().is_file()
            && e.file_name()
                .to_str()
                .map(|n| n != "export.r2x")
                .unwrap_or(true)
    })
}

#[allow(dead_code)]
pub fn parse_manifest(staging_root: &Path) -> Option<TsManifest> {
    let path = staging_root.join("manifest.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// BFS helper kept for tests / future incremental resolve.
#[allow(dead_code)]
pub fn dependency_keys(deps: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut q: VecDeque<&str> = deps.iter().map(|s| s.as_str()).collect();
    while let Some(raw) = q.pop_front() {
        if let Some(d) = TsDependency::parse(raw) {
            out.push(d.key());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_dependency() {
        let d = TsDependency::parse("BepInEx-BepInExPack-5.4.2100").unwrap();
        assert_eq!(d.namespace, "BepInEx");
        assert_eq!(d.name, "BepInExPack");
        assert_eq!(d.version, "5.4.2100");
    }

    #[test]
    fn parse_hyphenated_name() {
        let d = TsDependency::parse("Owner-Cool-Mod-1.2.3").unwrap();
        assert_eq!(d.namespace, "Owner");
        assert_eq!(d.name, "Cool-Mod");
        assert_eq!(d.version, "1.2.3");
    }

    #[test]
    fn parse_package_ident_owner_name() {
        let (ns, name) = parse_package_ident("bbepis-BepInExPack").unwrap();
        assert_eq!(ns, "bbepis");
        assert_eq!(name, "BepInExPack");
    }

    #[test]
    fn community_slug_ignores_hyphens() {
        assert!(community_slug_matches("lethalcompany", "lethal-company"));
        assert!(!community_slug_matches("valheim", "lethal-company"));
    }

    fn version(number: &str, dependencies: &[&str]) -> TsPackageVersion {
        TsPackageVersion {
            name: "pkg".into(),
            full_name: format!("pkg-{number}"),
            description: String::new(),
            icon: String::new(),
            version_number: number.into(),
            dependencies: dependencies.iter().map(|d| d.to_string()).collect(),
            download_url: String::new(),
            downloads: 0,
            date_created: String::new(),
            website_url: String::new(),
            is_active: true,
            uuid4: String::new(),
            file_size: 0,
        }
    }

    /// Newest first, as the Thunderstore API returns them.
    fn package(owner: &str, name: &str, versions: Vec<TsPackageVersion>) -> TsPackage {
        TsPackage {
            name: name.into(),
            full_name: format!("{owner}-{name}"),
            owner: owner.into(),
            package_url: String::new(),
            date_created: String::new(),
            date_updated: String::new(),
            uuid4: String::new(),
            rating_score: 0,
            is_pinned: false,
            is_deprecated: false,
            has_nsfw_content: false,
            categories: Vec::new(),
            versions,
        }
    }

    fn index(packages: Vec<TsPackage>) -> HashMap<String, TsPackage> {
        packages
            .into_iter()
            .map(|p| (format!("{}-{}", p.owner, p.name).to_lowercase(), p))
            .collect()
    }

    #[test]
    fn dependency_floor_keeps_the_requested_minimum() {
        let by_key = index(vec![
            package(
                "Author",
                "Mod",
                vec![version("1.0.0", &["bbepis-BepInExPack-5.4.2120"])],
            ),
            package(
                "bbepis",
                "BepInExPack",
                vec![version("5.4.2122", &[]), version("5.4.2120", &[])],
            ),
        ]);

        let floors = resolve_dependency_pins("author-mod", None, &by_key).unwrap();
        assert_eq!(floors.get("bbepis-bepinexpack").unwrap(), "5.4.2120");

        let selected =
            select_latest_satisfying(by_key.get("bbepis-bepinexpack").unwrap(), Some("5.4.2120"))
                .unwrap();
        assert_eq!(selected.version_number, "5.4.2122");
    }

    #[test]
    fn highest_floor_wins_and_latest_satisfying_is_installed() {
        let by_key = index(vec![
            package(
                "Author",
                "Mod",
                vec![version(
                    "1.0.0",
                    &["Author-Shared-1.0.0", "Author-Other-2.0.0"],
                )],
            ),
            package(
                "Author",
                "Other",
                vec![version("2.0.0", &["Author-Shared-1.5.0"])],
            ),
            package(
                "Author",
                "Shared",
                vec![
                    version("2.0.0", &[]),
                    version("1.5.0", &[]),
                    version("1.0.0", &[]),
                ],
            ),
        ]);

        let floors = resolve_dependency_pins("author-mod", None, &by_key).unwrap();

        // Floor is the higher minimum; install picks newest that still satisfies it.
        assert_eq!(floors.get("author-shared").unwrap(), "1.5.0");
        let selected =
            select_latest_satisfying(by_key.get("author-shared").unwrap(), Some("1.5.0")).unwrap();
        assert_eq!(selected.version_number, "2.0.0");
    }

    #[test]
    fn install_order_lists_dependencies_first() {
        let by_key = index(vec![
            package(
                "Author",
                "Mod",
                vec![version("1.0.0", &["Author-Lib-1.0.0"])],
            ),
            package(
                "Author",
                "Lib",
                vec![version("1.1.0", &[]), version("1.0.0", &[])],
            ),
        ]);
        let floors = resolve_dependency_pins("author-mod", None, &by_key).unwrap();

        let mut ordered = Vec::new();
        collect_install_order(
            "author-mod",
            "author-mod",
            None,
            &floors,
            &by_key,
            "riskofrain2",
            &mut HashSet::new(),
            &mut HashSet::new(),
            &mut ordered,
        )
        .unwrap();

        let names: Vec<_> = ordered.iter().map(|p| p.full_name.clone()).collect();
        assert_eq!(names, vec!["Author-Lib", "Author-Mod"]);
        // Lib floor is 1.0.0; newest satisfying release is 1.1.0.
        assert_eq!(ordered[0].latest_version.as_deref(), Some("1.1.0"));
        assert_eq!(ordered[0].versions[0].version_number, "1.1.0");
    }

    #[test]
    fn ror2_bepinexpack_cycle_installs_latest_ror2_pack() {
        // Modern bbepis-BepInExPack depends on RoR2BepInExPack-1.9.0; old 1.9.0
        // still depends back on BepInExPack. Latest 1.50.0 does not — and ships
        // Newtonsoft.Json. Soft cycles + min-version selection must pick 1.50.0.
        let by_key = index(vec![
            package(
                "bbepis",
                "BepInExPack",
                vec![version(
                    "5.4.2122",
                    &["RiskofThunder-RoR2BepInExPack-1.9.0"],
                )],
            ),
            package(
                "RiskofThunder",
                "RoR2BepInExPack",
                vec![
                    version("1.50.0", &[]),
                    version("1.9.0", &["bbepis-BepInExPack-5.4.2100"]),
                ],
            ),
        ]);

        let floors = resolve_dependency_pins("bbepis-bepinexpack", Some("5.4.2122"), &by_key)
            .expect("cycle must not abort pin resolve");
        assert_eq!(
            floors.get("riskofthunder-ror2bepinexpack").unwrap(),
            "1.9.0"
        );

        let mut ordered = Vec::new();
        collect_install_order(
            "bbepis-bepinexpack",
            "bbepis-bepinexpack",
            Some("5.4.2122"),
            &floors,
            &by_key,
            "riskofrain2",
            &mut HashSet::new(),
            &mut HashSet::new(),
            &mut ordered,
        )
        .expect("cycle must not abort install order");

        let ror2 = ordered
            .iter()
            .find(|p| p.full_name == "RiskofThunder-RoR2BepInExPack")
            .expect("RoR2BepInExPack in order");
        assert_eq!(ror2.latest_version.as_deref(), Some("1.50.0"));
        assert_eq!(ror2.versions[0].version_number, "1.50.0");
    }

    #[test]
    fn browse_snapshot_keeps_latest_version_only() {
        let pkg = package(
            "Owner",
            "Mod",
            vec![
                version("2.0.0", &["Dep-A-1.0.0"]),
                version("1.0.0", &["Dep-A-0.1.0", "Dep-B-1.0.0"]),
            ],
        );
        let mut latest = version("2.0.0", &["Dep-A-1.0.0"]);
        latest.downloads = 12;
        latest.description = "hello".into();
        latest.icon = "https://thunderstore.io/icon.png".into();
        latest.file_size = 40;
        let mut pkg = pkg;
        pkg.versions[0] = latest;
        let snap = packages_to_browse(&[pkg]);
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].latest_version, "2.0.0");
        assert_eq!(snap[0].dependency_count, 1);
        assert_eq!(snap[0].downloads, 12);
        assert_eq!(snap[0].description, "hello");
        assert_eq!(snap[0].file_size, 40);
    }

    fn browse_pkg(name: &str, downloads: u64, pinned: bool) -> TsBrowsePackage {
        TsBrowsePackage {
            name: name.into(),
            full_name: format!("Owner-{name}"),
            owner: "Owner".into(),
            package_url: String::new(),
            uuid4: String::new(),
            rating_score: 0,
            is_pinned: pinned,
            is_deprecated: false,
            has_nsfw_content: false,
            categories: vec!["Mods".into()],
            description: format!("{name} description"),
            icon: String::new(),
            downloads,
            latest_version: "1.0.0".into(),
            dependency_count: 0,
            file_size: 1,
        }
    }

    #[test]
    fn page_browse_orders_pinned_then_downloads() {
        let mut deprecated = browse_pkg("Old", 1000, false);
        deprecated.is_deprecated = true;
        let mut nsfw = browse_pkg("Spicy", 500, false);
        nsfw.has_nsfw_content = true;
        let packages = vec![
            browse_pkg("Alpha", 10, false),
            browse_pkg("Beta", 50, false),
            browse_pkg("Pinned", 1, true),
            deprecated,
            nsfw,
        ];
        let (page, total) = page_browse_packages(&packages, "", None, false, false, 0, 10);
        let names: Vec<_> = page.iter().map(|pkg| pkg.name.as_str()).collect();
        assert_eq!(names, vec!["Pinned", "Beta", "Alpha"]);
        assert_eq!(total, 3);

        let (matched, _) = page_browse_packages(&packages, "alp", None, true, false, 0, 10);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].name, "Alpha");
    }

    #[test]
    fn trusted_chunk_urls_stay_on_thunderstore() {
        assert!(trusted_thunderstore_url(
            "https://thunderstore.io/c/valheim/api/v1/package-listing-index/"
        ));
        assert!(trusted_thunderstore_url(
            "https://gcdn.thunderstore.io/live/chunk.json"
        ));
        assert!(!trusted_thunderstore_url(
            "http://thunderstore.io/chunk.json"
        ));
        assert!(!trusted_thunderstore_url("https://evil.example/chunk.json"));
        assert!(!trusted_thunderstore_url(
            "https://thunderstore.io.evil.example/chunk.json"
        ));
    }

    #[test]
    fn browse_cache_path_rejects_traversal() {
        let root = Path::new("/tmp/emperor-cache");
        assert!(browse_cache_path(root, "../secret").is_none());
        assert!(browse_cache_path(root, "lethal/company").is_none());
        assert_eq!(
            browse_cache_path(root, "lethal-company"),
            Some(root.join("thunderstore").join("lethal-company.browse.json"))
        );
    }

    #[test]
    fn browse_disk_cache_roundtrip() {
        let cache = TsBrowseDiskCache {
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
            chunk_urls: vec!["https://gcdn.thunderstore.io/chunk.json".into()],
            packages: vec![browse_pkg("Alpha", 3, false)],
        };
        let bytes = encode_browse_disk(&cache).unwrap();
        let decoded = decode_browse_disk(&bytes).unwrap();
        assert_eq!(decoded, cache);
    }
}
