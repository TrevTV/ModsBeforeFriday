//! Module for fetching external resources used by MBF.
//!
//! Unlike the old (Beat Saber) versions of MBF, this tool no longer downloads core mods or
//! version diffs. The only external resources needed are:
//! - The latest LemonLoader (MelonLoader for Android) release, from which the `melon_data.zip`
//!   asset is downloaded when patching.
//! - The unstripped `libunity.so` for the game's Unity version, from the
//!   MelonLoader.UnityDependencies repository.

use std::fs::File;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::res_cache::ResCache;

const MELONLOADER_RELEASE_INFO_URL: &str =
    "https://api.github.com/repos/LemonLoader/MelonLoader/releases/latest";

const UNITY_DEPS_URL_TEMPLATE: &str =
    "https://github.com/LavaGang/MelonLoader.UnityDependencies/releases/download/{0}/libunity.so.arm64-v8a";
const CHINA_UNITY_DEPS_URL_TEMPLATE: &str =
    "https://github.com/LemonLoader/MelonLoader.UnityDependencies.China/releases/download/{0}/libunity.so.arm64-v8a";

/// The subset of the GitHub release JSON that MBF needs.
#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Information about the latest LemonLoader (MelonLoader) release.
pub struct MelonLoaderRelease {
    /// The release tag, e.g. `0.6.5.1`.
    pub tag: String,
    /// The URL of the `melon_data.zip` asset within the release.
    pub melon_data_url: String,
}

/// Fetches information about the latest LemonLoader (MelonLoader) release.
///
/// The release info is fetched through the given resource cache, which will make use of a
/// locally cached copy of the release info when possible.
pub fn get_latest_melonloader_release(res_cache: &ResCache) -> Result<MelonLoaderRelease> {
    let release: GitHubRelease = res_cache
        .get_json_cached(MELONLOADER_RELEASE_INFO_URL, "melonloader_release.json")
        .map_err(|err| match err {
            crate::res_cache::JsonPullError::FetchError(err) => err,
            crate::res_cache::JsonPullError::ParseError(err) => err.into(),
        })
        .context("Fetching LemonLoader release info")?;

    let melon_data_asset = release
        .assets
        .iter()
        .find(|asset| asset.name.starts_with("melon_data"))
        .ok_or(anyhow::anyhow!(
            "The latest LemonLoader release did not contain a melon_data asset"
        ))?;

    Ok(MelonLoaderRelease {
        tag: release.tag_name,
        melon_data_url: melon_data_asset.browser_download_url.clone(),
    })
}

/// Gets the URL to download the unstripped `libunity.so` for the given Unity version from.
///
/// Unity "China" versions (e.g. `2021.3.16c1`) have their dependencies generated in a separate
/// repository, matching the behaviour of the LemonLoader installer.
pub fn get_unity_deps_url(unity_version: &str) -> String {
    let is_china = unity_version.contains('c');
    let template = if is_china {
        CHINA_UNITY_DEPS_URL_TEMPLATE
    } else {
        UNITY_DEPS_URL_TEMPLATE
    };

    template.replace("{0}", unity_version)
}

/// NB: Kept for convenience; a File is returned by the resource cache for large downloads
/// so that they can be streamed without being loaded into memory.
pub type CachedFile = File;
