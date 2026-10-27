//! Handler for the GetModStatus request.

use std::{fs::File, io::Cursor};

use log::{error, info, warn};
use mbf_res_man::external_res;
use mbf_zip::ZipFile;

use crate::{
    manifest::ManifestInfo,
    models::response::{self, AppInfo, LoaderInfo, ModLoader, Response},
    parameters::PARAMETERS,
    patching,
};
use anyhow::{Context, Result};

/// Handles `GetModStatus` [Requests](response::Request).
///
/// # Returns
/// The [Response](response::Response) to the request (variant `ModStatus`)
pub(super) fn handle_get_mod_status() -> Result<Response> {
    info!("Searching for the game app");
    let app_info = get_app_info().context("Getting app info")?;

    let installed_mods = match &app_info {
        Some(_) => {
            let mod_manager = super::load_mod_manager()?;
            super::get_mod_models(mod_manager)?
        }
        None => {
            warn!("The game is not installed!");
            Vec::new()
        }
    };

    // Fetch the latest LemonLoader release, which is used to check whether the installed
    // version is up to date. If this fails, the user is assumed to have no internet connection.
    let loader_info = match fetch_loader_info(&app_info) {
        Ok(info) => Some(info),
        Err(err) => {
            error!("Failed to fetch LemonLoader release info: {err:#}");
            None
        }
    };

    Ok(Response::ModStatus {
        app_info,
        installed_mods,
        loader_info,
    })
}

/// Fetches the latest LemonLoader release info, and works out the install status of the
/// modloader within the installed app.
fn fetch_loader_info(app_info: &Option<AppInfo>) -> Result<LoaderInfo> {
    let res_cache = crate::load_res_cache()?;
    let latest = external_res::get_latest_melonloader_release(&res_cache)
        .context("Fetching latest LemonLoader release")?;

    let (is_patched, installed_version) = match &app_info {
        Some(app_info) => match &app_info.loader_installed {
            Some(ModLoader::MelonLoader) => (true, app_info.installed_loader_version.clone()),
            _ => (false, None),
        },
        None => (false, None),
    };

    Ok(LoaderInfo {
        latest_version: latest.tag.clone(),
        install_status: patching::get_loader_install_status(
            is_patched,
            installed_version.as_deref(),
            Some(&latest.tag),
        ),
    })
}

pub(super) fn get_app_info() -> Result<Option<response::AppInfo>> {
    let apk_path = match crate::get_apk_path().context("Finding APK path")? {
        Some(path) => path,
        None => return Ok(None),
    };

    let apk_reader = std::fs::File::open(&apk_path)?;
    let mut apk = ZipFile::open(apk_reader).context("Reading APK as ZIP")?;

    let patch_status = patching::get_patch_status(&mut apk)?;
    let obb_present = check_obb_present()?;

    let version = get_manifest_version(&mut apk)?;

    // Only check whether Unity dependencies are available if the game is unpatched,
    // as they are only needed for patching.
    let unity_deps_available = match (&patch_status.unity_version, &patch_status.loader) {
        (Some(unity_version), None) => {
            let res_cache = crate::load_res_cache()?;
            patching::check_unity_deps_available(&res_cache, unity_version)
        }
        _ => true,
    };

    Ok(Some(response::AppInfo {
        loader_installed: patch_status.loader,
        installed_loader_version: patch_status.installed_loader_version,
        obb_present,
        version,
        unity_version: patch_status.unity_version,
        unity_deps_available,
        path: apk_path,
    }))
}

fn get_manifest_version(apk: &mut ZipFile<File>) -> Result<String> {
    let manifest = apk
        .read_file("AndroidManifest.xml")
        .context("Reading manifest file from APK")?;

    // Decode various important information from the manifest
    let mut manifest_reader = Cursor::new(&manifest);
    let mut axml_reader =
        mbf_axml::AxmlReader::new(&mut manifest_reader).context("Manifest was not valid AXML")?;
    let manifest_info = ManifestInfo::read(&mut axml_reader).context("Parsing manifest from AXML")?;

    Ok(manifest_info.package_version)
}

/// Checks that there is at least one file with extension .obb in the OBB directory of the app.
///
/// BONELAB stores the majority of its assets within an OBB, so if this is not present
/// the installation is invalid and the game needs to be reinstalled.
fn check_obb_present() -> Result<bool> {
    if !std::path::Path::new(&PARAMETERS.obb_dir).exists() {
        return Ok(false);
    }

    // Check if any of the files in the OBB directory have extension OBB
    Ok(std::fs::read_dir(&PARAMETERS.obb_dir)?.any(|stat_res| {
        stat_res.is_ok_and(|path| {
            path.path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("obb"))
        })
    }))
}
