//! Implementation of the APK patching for BONELAB using MelonLoader (LemonLoader).
//!
//! The patching process closely mirrors the one used by the official LemonLoader installer
//! (https://github.com/LemonLoader/MelonLoaderInstaller):
//! 1. Detect the Unity version of the game (from `globalgamemanagers`/`data.unity3d`).
//! 2. Download the latest `melon_data.zip` from the LemonLoader/MelonLoader GitHub releases, and
//!    an unstripped `libunity.so` for the game's Unity version from MelonLoader.UnityDependencies.
//! 3. Patch the Android manifest (permissions + `debuggable`/`requestLegacyExternalStorage`).
//! 4. Add `assets/MelonLoader` + `assets/dotnet` and the MelonLoader native libraries
//!    (incl. `libBootstrap.so`, `libdobby.so` and MelonLoader's own `libmain.so`) to the APK,
//!    replacing the stripped `libunity.so` with the unstripped one.
//! 5. Sign the APK and reinstall it.

use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::Cursor,
    path::Path,
    process::Command,
};
use regex::regex;

use crate::{
    downloads,
    models::response::{AppInfo, InstallStatus, ModLoader},
    parameters::PARAMETERS,
};
use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};
use mbf_res_man::{external_res, res_cache::ResCache};
use mbf_zip::{signing, FileCompression, ZipFile};

const DEBUG_CERT_PEM: &[u8] = include_bytes!("debug_cert.pem");

// Aligment to use for ZIP entries with the STORE compression method, in bytes.
// 4 is the standard value.
const STORE_ALIGNMENT: u16 = 4;

const LIB_DIR: &str = "lib/arm64-v8a";
const MANIFEST_PATH: &str = "AndroidManifest.xml";

const MELON_DATA_ASSETS_DIR: &str = "assets/MelonLoader";
const DOTNET_ASSETS_DIR: &str = "assets/dotnet";
const PATCH_DATE_ASSET: &str = "assets/lemon_patch_date.txt";
const PATCH_INFO_ASSET: &str = "assets/lemon_patch_info.json";

const LIBUNITY_PATH: &str = "lib/arm64-v8a/libunity.so";
const LIB_BOOTSTRAP_PATH: &str = "lib/arm64-v8a/libBootstrap.so";
const LIB_DOBBY_PATH: &str = "lib/arm64-v8a/libdobby.so";

const GLOBALGAMEMANAGERS_PATH: &str = "assets/bin/Data/globalgamemanagers";
const DATA_UNITY3D_PATH: &str = "assets/bin/Data/data.unity3d";

/// The name of the patcher written into the patch info file, for identification purposes.
pub const PATCHER_NAME: &str = "BONELAB Lemonifier";

/// The state of the MelonLoader installation within the APK of the installed game.
pub struct PatchStatus {
    /// The modloader installed within the APK, if any.
    pub loader: Option<ModLoader>,
    /// The version of LemonLoader this APK was patched with, if it was patched by this tool.
    /// (The official LemonLoader installer does not write this info.)
    pub installed_loader_version: Option<String>,
    /// The Unity version the game was built with, if it could be detected.
    pub unity_version: Option<String>,
}

/// Mods the installed game and reinstalls it, adding MelonLoader to the APK.
pub fn mod_game(
    temp_path: &Path,
    app_info: &AppInfo,
    res_cache: &ResCache,
) -> Result<()> {
    kill_app().context("Killing BONELAB")?;

    info!("Copying APK to temporary location");
    let temp_apk_path = temp_path.join("mbf-tmp.apk");
    std::fs::copy(&app_info.path, &temp_apk_path).context("Copying APK to temp")?;

    // Make sure the APK is writable.  Sometimes Android will mark it as read-only and
    // the resulting copy will inherit those permissions.
    ensure_can_write_apk(&temp_apk_path).context("Marking APK as writable")?;

    // Move the OBB and app data directories out of the way, as uninstalling the app
    // (which is necessary as the signature changes) would otherwise delete them.
    // NB: These are renames on the same filesystem, so they are instant.
    // Both are moved back after the patched app has been installed.
    let obb_backed_up = move_dir(&PARAMETERS.obb_dir, &PARAMETERS.obb_backup_dir)
        .context("Backing up OBB files")?;
    let app_data_backed_up = move_dir(&PARAMETERS.app_data_dir, &PARAMETERS.app_data_backup_dir)
        .context("Backing up app data")
        .unwrap_or_else(|err| {
            warn!("{err:#} - any saved game data may be lost!");
            false
        });

    let result = patch_and_reinstall(&temp_apk_path, res_cache, obb_backed_up, app_data_backed_up);

    // No matter the outcome of the patching, attempt to put the user's files back.
    restore_dir(&PARAMETERS.obb_backup_dir, &PARAMETERS.obb_dir, "OBB files")
        .map_err(|err| warn!("{err:#}"))
        .ok();
    restore_dir(&PARAMETERS.app_data_backup_dir, &PARAMETERS.app_data_dir, "app data")
        .map_err(|err| warn!("{err:#}"))
        .ok();

    // NB: The patched APK is deliberately kept (not deleted) so that failed installs
    // can be debugged: it ends up at /data/local/tmp/mbf/last-patched.apk.
    std::fs::rename(&temp_apk_path, Path::new(&PARAMETERS.temp).with_file_name("last-patched.apk")).ok();
    result
}

fn ensure_can_write_apk(temp_apk_path: &Path) -> Result<()> {
    let mut permissions = std::fs::metadata(temp_apk_path)
        .context("Reading temp APK permissions")?
        .permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(temp_apk_path, permissions).context("Making temp APK writable")?;
    Ok(())
}

/// Moves a directory to the given destination, returning whether it was actually moved.
/// The destination must not already exist. If the source does not exist, no move occurs.
///
/// A plain rename is attempted first, but on Android the FUSE layer treats certain
/// directories (e.g. `/sdcard/Android/obb`, `/sdcard/Android/data`) as separate mount
/// boundaries, so renames across them fail with `EXDEV`. In that case, this falls back
/// to copying the directory tree and deleting the source - which is what the official
/// LemonLoader installer's move scripts effectively do.
fn move_dir(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<bool> {
    let from = from.as_ref();
    let to = to.as_ref();
    if !from.exists() {
        debug!("{from:?} does not exist, not moving");
        return Ok(false);
    }
    if to.exists() {
        std::fs::remove_dir_all(to).context("Removing existing backup directory")?;
    }

    match std::fs::rename(from, to) {
        Ok(_) => Ok(true),
        Err(rename_err) => {
            debug!("Rename of {from:?} to {to:?} failed ({rename_err:#}), falling back to copy+delete");
            if let Err(copy_err) = copy_dir_recursive(from, to) {
                // Clean up any partially copied destination so that a retry starts fresh.
                std::fs::remove_dir_all(to).ok();
                return Err(copy_err).with_context(|| format!("Copying {from:?} to {to:?} (rename was not possible)"));
            }

            std::fs::remove_dir_all(from)
                .with_context(|| format!("Deleting source directory {from:?} after copying it to {to:?}"))?;
            Ok(true)
        }
    }
}

/// Recursively copies the directory tree from `from` to `to`, creating `to` if needed.
fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)
        .with_context(|| format!("Creating destination directory {to:?}"))?;

    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = to.join(entry.file_name());

        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path)?;
        } else {
            std::fs::copy(&entry_path, &dest_path)
                .with_context(|| format!("Copying {:?} to {dest_path:?}", entry_path))?;
        }
    }

    Ok(())
}

/// Restores a previously moved directory back to its original location, if it exists.
fn restore_dir(backup_dir: impl AsRef<Path>, original_dir: impl AsRef<Path>, name: &str) -> Result<()> {
    let backup_dir = backup_dir.as_ref();
    let original_dir = original_dir.as_ref();
    if !backup_dir.exists() {
        return Ok(()); // Never moved in the first place.
    }

    if original_dir.exists() {
        // Should not happen, but avoid clobbering anything that already exists.
        std::fs::remove_dir_all(original_dir).context("Removing existing (partially restored) directory")?;
    }

    move_dir(backup_dir, original_dir).map(|_| ()).with_context(|| {
        format!(
            "Restoring {name} from {backup_dir:?} to {original_dir:?} failed. \
            The backup has been kept at {backup_dir:?}, so you may be able to restore it manually with a file manager."
        )
    })
}

pub fn kill_app() -> Result<()> {
    info!("Killing BONELAB");
    Command::new("am")
        .args(["force-stop", &PARAMETERS.apk_id])
        .output()?;
    Ok(())
}

fn patch_and_reinstall(
    temp_apk_path: &Path,
    res_cache: &ResCache,
    obb_backed_up: bool,
    app_data_backed_up: bool,
) -> Result<()> {
    info!("Patching APK");
    patch_apk_in_place(temp_apk_path, res_cache).context("Patching APK")?;

    reinstall_modded_app(temp_apk_path).context("Reinstalling modded APK")?;

    if app_data_backed_up {
        restore_dir(&PARAMETERS.app_data_backup_dir, &PARAMETERS.app_data_dir, "app data")
            .map_err(|err| warn!("{err:#}"))
            .ok();
    }

    if obb_backed_up {
        restore_dir(&PARAMETERS.obb_backup_dir, &PARAMETERS.obb_dir, "OBB files")
            .map_err(|err| warn!("{err:#}"))
            .ok();
    }

    Ok(())
}

fn reinstall_modded_app(temp_apk_path: &Path) -> Result<()> {
    info!("Reinstalling modded app");
    Command::new("pm")
        .args(["uninstall", &PARAMETERS.apk_id])
        .output()
        .context("Uninstalling vanilla APK")?;

    let is_pico = get_android_device_manufacturer()
        .map(|v| v.to_lowercase().contains("pico"))
        .unwrap_or(false);

    if is_pico {
        Command::new("pm")
            .args([
                "install",
                "-i", "com.picovr.store", // needed to display game icon in the app launcher
                temp_apk_path.to_string_lossy().as_ref(),
            ])
            .output()
            .context("Installing modded APK")?;
    } else {
        Command::new("pm")
            .args(["install", temp_apk_path.to_string_lossy().as_ref()])
            .output()
            .context("Installing modded APK")?;
    }

    info!("Granting external storage permission");
    Command::new("appops")
        .args(["set", "--uid", &PARAMETERS.apk_id, "MANAGE_EXTERNAL_STORAGE", "allow"])
        .output()?;

    Ok(())
}

/// Reads the Unity version that the given APK was built with.
///
/// This mirrors the detection logic of the LemonLoader installer, which reads the version from
/// `assets/bin/Data/globalgamemanagers`, falling back to `assets/bin/Data/data.unity3d`.
pub fn get_unity_version(apk: &mut ZipFile<std::fs::File>) -> Result<String> {
    if apk.contains_file(GLOBALGAMEMANAGERS_PATH) {
        let data = apk
            .read_file(GLOBALGAMEMANAGERS_PATH)
            .context("Reading globalgamemanagers")?;
        return parse_unity_version_from_serialized_file(&data)
            .context("Parsing Unity version from globalgamemanagers");
    }

    if apk.contains_file(DATA_UNITY3D_PATH) {
        let data = apk
            .read_file(DATA_UNITY3D_PATH)
            .context("Reading data.unity3d")?;
        return parse_unity_version_from_bundle(&data)
            .context("Parsing Unity version from data.unity3d");
    }

    Err(anyhow!("The APK contained neither globalgamemanagers nor data.unity3d, so the Unity version could not be detected"))
}

/// Parses the Unity version from a serialized Unity assets file
/// (such as `globalgamemanagers`), which starts with the header:
/// [metadata size: u32] [file size: u32] [version: u32] [data offset: u32] [unity version: C string]
/// based on: https://github.com/SamboyCoding/Cpp2IL/blob/development/LibCpp2IL/LibCpp2IlMain.cs#L101
fn parse_unity_version_from_serialized_file(data: &[u8]) -> Result<String> {
    let mut ver_string = String::default();
    let mut idx: usize = 0x14;
    while data[idx] != u8::from(0) {
        ver_string.push(data[idx] as char);
        idx += 1;
    }

    if !regex!(r"^[0-9]+\.[0-9]+\.[0-9]+[abcfxp][0-9]+$").is_match(&ver_string) {
        idx = 0x30;
        ver_string.clear();
        while data[idx] != u8::from(0) {
            ver_string.push(data[idx] as char);
            idx += 1;
        }
    }

    // valid?
    if !regex!(r"^[0-9]+\.[0-9]+\.[0-9]+[abcfxp][0-9]+$").is_match(&ver_string) {
        return Err(anyhow!("Unity version string in globalgamemanagers was invalid: {ver_string}"));
    }

    // remove extra version identifiers (e.g. `2022.3.22f1`) and whitespace
    ver_string = ver_string.trim().split('f').next().unwrap_or(&ver_string).to_string();

    Ok(ver_string)
}

/// Parses the Unity version from a Unity asset bundle (such as `data.unity3d`),
/// whose header is: [signature: C string] [bundle version: u32 big endian] [unity version: C string]
fn parse_unity_version_from_bundle(data: &[u8]) -> Result<String> {
    let signature_end = data
        .iter()
        .position(|&b| b == 0)
        .context("No null terminator after bundle signature")?;
    let signature = std::str::from_utf8(&data[..signature_end])
        .context("Bundle signature was not valid UTF-8")?;
    if !matches!(signature, "UnityFS" | "UnityRaw" | "UnityWeb") {
        return Err(anyhow!("Unexpected bundle signature {signature}"));
    }

    // Skip the null terminator of the signature and the 32 bit (big endian) bundle version.
    const BUNDLE_VERSION_SIZE: usize = 4;
    let version_start = signature_end + 1 + BUNDLE_VERSION_SIZE;
    if data.len() <= version_start {
        return Err(anyhow!("Bundle was too small to contain a Unity version"));
    }

    read_c_string(&data[version_start..], "data.unity3d")
}

/// Reads a null-terminated string from the start of the given byte slice.
fn read_c_string(data: &[u8], context: &str) -> Result<String> {
    let end = data
        .iter()
        .position(|&b| b == 0)
        .context("No null terminator after Unity version string")?;
    let string = std::str::from_utf8(&data[..end])
        .context("Unity version was not valid UTF-8")?
        .to_owned();

    validate_unity_version(string, context)
}

fn validate_unity_version(version: String, context: &str) -> Result<String> {
    if version.is_empty() {
        return Err(anyhow!("Unity version string in {context} was empty"));
    }
    // Unity versions look like `2022.3.22f1` or `2021.3.16c1` (China).
    if !version.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(anyhow!("'{version}' does not look like a Unity version"));
    }
    Ok(version)
}

/// Patches the APK with MelonLoader, in place.
fn patch_apk_in_place(path: impl AsRef<Path>, res_cache: &ResCache) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .context("Opening temporary APK for writing")?;

    let mut zip = ZipFile::open(file).context("Reading temporary APK as ZIP")?;
    zip.set_store_alignment(STORE_ALIGNMENT);

    let unity_version = get_unity_version(&mut zip).context("Detecting Unity version")?;
    info!("Game was built with Unity {unity_version}");

    info!("Fetching latest LemonLoader release info");
    let release = external_res::get_latest_melonloader_release(res_cache)
        .context("Fetching the latest LemonLoader release info")?;
    info!("Latest LemonLoader version is {}", release.tag);

    // Check that an unstripped libunity.so exists for this Unity version before
    // downloading the (large) MelonLoader data.
    let libunity_url = external_res::get_unity_deps_url(&unity_version);
    let libunity = match res_cache.get_cached(&libunity_url, &format!("libunity-{unity_version}.so")) {
        Ok(libunity) => libunity,
        Err(err) => {
            return Err(err).with_context(|| format!(
                "There is no unstripped libunity.so available for Unity {unity_version}. \
                It may not have been generated yet, or this version of the game may not be moddable."
            ))
        }
    };

    info!("Downloading MelonLoader data (this could take a minute)");
    let melon_data_file = res_cache
        .get_cached(&release.melon_data_url, "melon_data.zip")
        .context("Downloading MelonLoader data")?;

    info!("Applying manifest mods");
    patch_manifest(&mut zip).context("Patching manifest")?;

    let (priv_key, cert) = signing::load_cert_and_priv_key(DEBUG_CERT_PEM);

    // Remove any old MelonLoader data (e.g. if we are updating an existing patch) before
    // copying the new files in.
    remove_prefixed_files(&mut zip, MELON_DATA_ASSETS_DIR);
    remove_prefixed_files(&mut zip, DOTNET_ASSETS_DIR);

    let mut melon_data_zip =
        ZipFile::open(melon_data_file).context("Reading MelonLoader data as ZIP")?;
    copy_melon_data_to_apk(&mut melon_data_zip, &mut zip)
        .context("Copying MelonLoader data into APK")?;

    info!("Replacing libunity.so with the unstripped version (this may take up to a minute)");
    zip.delete_file(LIBUNITY_PATH);
    let mut libunity_handle = libunity;
    zip.write_file(LIBUNITY_PATH, &mut libunity_handle, FileCompression::Deflate)?;

    write_patch_metadata(&mut zip, &release.tag)?;

    info!("Signing");
    zip.save_and_sign_v2(&cert, &priv_key)
        .context("Saving/signing APK")?;

    Ok(())
}

/// Copies the relevant files from the downloaded melon_data.zip into the APK:
/// - `MelonLoader/**` -> `assets/MelonLoader/**`
/// - `dotnet/**` -> `assets/dotnet/**`
/// - `native/*.so` -> `lib/arm64-v8a/*.so` (this includes MelonLoader's own `libmain.so`)
///
/// Entries are copied as-is (no decompression/recompression), which is a lot faster.
fn copy_melon_data_to_apk(
    melon_data: &mut ZipFile<std::fs::File>,
    apk: &mut ZipFile<std::fs::File>,
) -> Result<()> {
    let entry_names = melon_data
        .iter_entry_names()
        .map(|s| s.to_string())
        .collect::<Vec<String>>();

    for entry_name in entry_names {
        // Skip the folder entries within the ZIP.
        if entry_name.ends_with('/') {
            continue;
        }

        let dest_name = if let Some(rest) = entry_name.strip_prefix("MelonLoader/") {
            format!("{MELON_DATA_ASSETS_DIR}/{rest}")
        } else if let Some(rest) = entry_name.strip_prefix("dotnet/") {
            format!("{DOTNET_ASSETS_DIR}/{rest}")
        } else if let Some(rest) = entry_name.strip_prefix("native/") {
            if !rest.ends_with(".so") {
                continue;
            }
            format!("{LIB_DIR}/{rest}")
        } else {
            debug!("Skipping unexpected MelonLoader data entry {entry_name}");
            continue;
        };

        debug!("Copying {entry_name} to {dest_name}");
        apk.delete_file(&dest_name);
        melon_data.copy_entry(&entry_name, apk, dest_name)?;
    }

    Ok(())
}

/// Deletes every entry within the APK which is the given file/directory, or within it.
fn remove_prefixed_files(zip: &mut ZipFile<std::fs::File>, prefix: &str) {
    let dir_prefix = format!("{prefix}/");
    let to_remove = zip
        .iter_entry_names()
        .filter(|name| name == &prefix || name.starts_with(&dir_prefix))
        .map(|s| s.to_string())
        .collect::<Vec<String>>();
    for name in to_remove {
        zip.delete_file(&name);
    }
}

/// Adds the metadata files that LemonLoader and this patcher write into patched APKs.
fn write_patch_metadata(zip: &mut ZipFile<std::fs::File>, melonloader_version: &str) -> Result<()> {
    // The date the APK was patched, in the same format as the official installer.
    // This is used by MelonLoader to check when the game was last patched.
    let now = std::time::SystemTime::now();
    let rfc3339 = humantime::format_rfc3339(now).to_string();

    zip.delete_file(PATCH_DATE_ASSET);
    zip.write_file(
        PATCH_DATE_ASSET,
        &mut Cursor::new(rfc3339.into_bytes()),
        FileCompression::Deflate,
    )?;

    // Additional info written by this patcher, used to detect when the installed
    // MelonLoader version is out of date.
    let patch_info = serde_json::json!({
        "patcher": PATCHER_NAME,
        "melonloader_version": melonloader_version,
    });

    zip.delete_file(PATCH_INFO_ASSET);
    zip.write_file(
        PATCH_INFO_ASSET,
        &mut Cursor::new(patch_info.to_string().into_bytes()),
        FileCompression::Deflate,
    )?;

    Ok(())
}

/// Detects the state of the MelonLoader installation within the given APK.
pub fn get_patch_status(apk: &mut ZipFile<std::fs::File>) -> Result<PatchStatus> {
    let is_patched = apk.contains_file(LIB_BOOTSTRAP_PATH) && apk.contains_file(LIB_DOBBY_PATH);

    let loader = if is_patched {
        Some(ModLoader::MelonLoader)
    } else if apk.contains_file("modded.json") {
        // The APK was patched with a different (non-MelonLoader) modding scheme.
        Some(ModLoader::Unknown)
    } else {
        None
    };

    let installed_loader_version = if is_patched && apk.contains_file(PATCH_INFO_ASSET) {
        match serde_json::from_slice::<serde_json::Value>(&apk.read_file(PATCH_INFO_ASSET)?) {
            Ok(json) => json
                .get("melonloader_version")
                .and_then(|version| version.as_str())
                .map(|version| version.to_owned()),
            Err(err) => {
                warn!("Patch info was invalid JSON: {err}");
                None
            }
        }
    } else {
        None
    };

    let unity_version = match get_unity_version(apk) {
        Ok(version) => Some(version),
        Err(err) => {
            warn!("Failed to detect Unity version: {err:#}");
            None
        }
    };

    Ok(PatchStatus {
        loader,
        installed_loader_version,
        unity_version,
    })
}

/// Checks whether an unstripped libunity.so exists for the given Unity version.
/// Returns false (and logs a warning) if the check could not be performed due to connectivity issues.
pub fn check_unity_deps_available(_res_cache: &ResCache, unity_version: &str) -> bool {
    let url = external_res::get_unity_deps_url(unity_version);
    match downloads::check_url_exists(&crate::get_dl_cfg(), &url) {
        Ok(exists) => exists,
        Err(err) => {
            warn!("Failed to check for Unity dependencies: {err:#}");
            false
        }
    }
}

/// Works out the install status of the MelonLoader installation,
/// comparing the version within the APK with the latest available release.
pub fn get_loader_install_status(
    is_patched: bool,
    installed_version: Option<&str>,
    latest_version: Option<&str>,
) -> InstallStatus {
    if !is_patched {
        return InstallStatus::Missing;
    }

    match (installed_version, latest_version) {
        // Can't tell whether the installation is up to date (e.g. it was patched with the
        // official LemonLoader installer), so don't bother the user about it.
        (None, _) => InstallStatus::Ready,
        (Some(installed), Some(latest)) => {
            if installed == latest {
                InstallStatus::Ready
            } else {
                InstallStatus::NeedUpdate
            }
        }
        // Latest version unknown (probably no internet connection).
        (Some(_), None) => InstallStatus::Ready,
    }
}

const STANDARD_PERMISSIONS: [&str; 5] = [
    "android.permission.READ_EXTERNAL_STORAGE",
    "android.permission.WRITE_EXTERNAL_STORAGE",
    "android.permission.MANAGE_EXTERNAL_STORAGE",
    "android.permission.RECORD_AUDIO", /* used for voice chat in some mods; requires the user to allow it via a prompt */
    "android.permission.INTERNET",
];

/// Applies the LemonLoader manifest modifications directly to the (binary) AXML,
/// preserving the typed values and resource IDs of every existing attribute.
///
/// NB: This deliberately does NOT round trip the manifest through text: converting
/// attribute values to strings and back would lose their original types (e.g. the
/// string-typed `compileSdkVersionCodename` would be re-encoded as an integer), which
/// Android's manifest parsing rejects at runtime.
fn patch_manifest(zip: &mut ZipFile<std::fs::File>) -> Result<()> {
    use mbf_axml::{AxmlWriter, Event};

    let manifest_data = zip
        .read_file(MANIFEST_PATH)
        .context("Reading manifest file from APK")?;

    let mut manifest_cursor = Cursor::new(&manifest_data);
    let mut axml_reader =
        mbf_axml::AxmlReader::new(&mut manifest_cursor).context("Manifest was not valid AXML")?;

    // Collect the manifest events so that modifications can be made to the tree.
    let mut events: Vec<Event> = Vec::new();
    let mut application_index: Option<usize> = None;
    let mut manifest_close_index: Option<usize> = None;

    while let Some(event) = axml_reader.read_next_event().context("Reading manifest events")? {
        match &event {
            Event::StartElement { name, .. } if name == "application" => {
                application_index = Some(events.len());
            }
            Event::EndElement { name, .. } if name == "manifest" => {
                manifest_close_index = Some(events.len());
            }
            _ => {}
        }
        events.push(event);
    }

    let application_index = application_index.ok_or(anyhow!("No <application> element in the manifest"))?;
    let manifest_close_index = manifest_close_index
        .ok_or(anyhow!("No closing </manifest> element in the manifest"))?;

    // Add the standard permissions at the end of the manifest element, mirroring the
    // behaviour of the LemonLoader installer.
    let mut existing_permissions: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            Event::StartElement { name, attributes, .. } if name == "uses-permission" => attributes
                .iter()
                .find(|attr| attr.name == "name" && attr.namespace.as_deref() == Some(mbf_axml::ANDROID_NS_URI))
                .and_then(|attr| match attr.value {
                    mbf_axml::AttributeValue::String(ref value) => Some(value.clone()),
                    _ => None,
                }),
            _ => None,
        })
        .collect();

    let resource_ids = mbf_axml::ResourceIds::load().context("Loading resource IDs")?;
    let name_res_id = resource_ids
        .get_res_id_or_none("name")
        .ok_or(anyhow!("No resource ID for the `name` attribute"))?;

    let mut permission_events: Vec<Event> = Vec::new();
    for permission in STANDARD_PERMISSIONS {
        if existing_permissions.iter().any(|existing| existing == permission) {
            debug!("Permission {permission} already present, skipping");
            continue;
        }

        info!("Adding permission {permission}");
        permission_events.push(Event::StartElement {
            attributes: vec![mbf_axml::Attribute {
                name: "name".to_owned(),
                namespace: Some(mbf_axml::ANDROID_NS_URI.to_owned()),
                resource_id: Some(name_res_id),
                value: mbf_axml::AttributeValue::String(permission.to_owned()),
            }],
            name: "uses-permission".to_owned(),
            namespace: None,
            line_num: 0,
        });
        permission_events.push(Event::EndElement {
            line_num: 0,
            namespace: None,
            name: "uses-permission".to_owned(),
        });
        existing_permissions.push(permission.to_owned());
    }

    events.splice(manifest_close_index..manifest_close_index, permission_events);

    // Recompute the manifest close index (permissions were inserted before it).
    let manifest_close_index = events
        .iter()
        .position(|event| matches!(event, Event::EndElement { name, .. } if name == "manifest"))
        .ok_or(anyhow!("No closing </manifest> element in the manifest"))?;

    // Add the application flags required by MelonLoader, if not already present.
    if let Some(&index) = events
        .iter()
        .position(|event| matches!(event, Event::StartElement { name, .. } if name == "application"))
        .as_ref()
    {
        if let Event::StartElement { attributes, .. } = &mut events[index] {
            for (attr_name, value) in [
                ("debuggable", mbf_axml::AttributeValue::Boolean(true)),
                (
                    "requestLegacyExternalStorage",
                    mbf_axml::AttributeValue::Boolean(true),
                ),
            ] {
                if attributes.iter().any(|attr| attr.name == attr_name) {
                    debug!("Application flag {attr_name} already present, skipping");
                    continue;
                }

                info!("Adding application flag {attr_name}");
                let resource_id = resource_ids.get_res_id_or_none(attr_name);
                attributes.push(mbf_axml::Attribute {
                    name: attr_name.to_owned(),
                    namespace: Some(mbf_axml::ANDROID_NS_URI.to_owned()),
                    resource_id,
                    value,
                });
            }
        }
    }

    let _ = manifest_close_index;

    // Write the modified events back into (binary) AXML.
    let mut data_output = Cursor::new(Vec::new());
    let mut axml_writer = AxmlWriter::new(&mut data_output);
    for event in events {
        axml_writer.write_event(event);
    }
    axml_writer
        .finish()
        .context("Saving AXML (binary) manifest")?;

    zip.delete_file(MANIFEST_PATH);
    zip.write_file(MANIFEST_PATH, &mut data_output, FileCompression::Deflate)
        .context("Writing modified manifest")?;

    Ok(())
}

/// Performs the LemonLoader manifest modifications on the given (readable) manifest XML.
fn modify_manifest_xml(manifest_xml: String) -> Result<String> {
    use std::collections::HashMap;
    use xml::reader::XmlEvent as ReadEvent;
    use xml::writer::XmlEvent as WriteEvent;

    const ANDROID_NS: &str = mbf_axml::ANDROID_NS_URI;

    let mut reader = xml::EventReader::new(Cursor::new(manifest_xml.as_bytes()));
    let mut output = Vec::new();
    let mut writer = xml::EmitterConfig::new()
        .perform_indent(true)
        .create_writer(Cursor::new(&mut output));

    // Map of namespace prefix -> URI, representing the namespaces currently in scope.
    let mut current_ns: HashMap<String, String> = HashMap::new();
    // The prefix used for the `android` namespace, once the manifest element declares it.
    let mut android_prefix: Option<String> = None;
    // Permissions that already exist within the manifest.
    let mut existing_permissions: HashSet<String> = HashSet::new();

    loop {
        match reader.next()? {
            ReadEvent::StartElement { name, attributes, namespace } => {
                // Work out which namespaces are newly declared with this element, so that
                // they can be re-declared on the way out.
                let newly_declared: Vec<(String, String)> = namespace
                    .0
                    .iter()
                    .map(|(prefix, uri)| (prefix.as_str(), uri.as_str()))
                    .filter(|(prefix, uri)| {
                        !uri.is_empty()
                            && *prefix != "xml"
                            && current_ns.get(*prefix).map(|v| v.as_str()) != Some(*uri)
                    })
                    .map(|(prefix, uri)| (prefix.to_owned(), uri.to_owned()))
                    .collect();

                let mut builder = WriteEvent::start_element(name.borrow());
                for (prefix, uri) in &newly_declared {
                    builder = builder.ns(prefix.as_str(), uri.as_str());
                    current_ns.insert(prefix.clone(), uri.clone());
                }
                for attribute in &attributes {
                    builder = builder.attr(attribute.name.borrow(), attribute.value.as_str());
                }

                let local_name = name.local_name.as_str();
                if local_name == "manifest" {
                    android_prefix = Some(namespace
                        .0
                        .iter()
                        .find_map(|(prefix, uri)| (uri.as_str() == ANDROID_NS).then(|| prefix.clone()))
                        .ok_or_else(|| anyhow!("Manifest did not declare the android namespace"))?);
                } else if local_name == "uses-permission" {
                    if let Some(permission) = get_android_attr(&attributes, "name") {
                        existing_permissions.insert(permission.to_owned());
                    }
                } else if local_name == "application" {
                    let prefix = android_prefix
                        .as_deref()
                        .ok_or_else(|| anyhow!("Application element came before the manifest element declared the android namespace"))?;
                    let has_flag = |flag: &str| attributes.iter().any(|a| a.name.local_name == flag);
                    if !has_flag("debuggable") {
                        info!("Adding debuggable flag");
                        builder = builder.attr(
                            xml::name::Name::qualified("debuggable", ANDROID_NS, Some(prefix)),
                            "true",
                        );
                    }
                    if !has_flag("requestLegacyExternalStorage") {
                        info!("Adding legacy external storage flag");
                        builder = builder.attr(
                            xml::name::Name::qualified(
                                "requestLegacyExternalStorage",
                                ANDROID_NS,
                                Some(prefix),
                            ),
                            "true",
                        );
                    }
                }

                writer.write(builder)?;
            }
            ReadEvent::EndElement { name } => {
                if name.local_name == "manifest" {
                    // Add the standard permissions at the end of the manifest element,
                    // mirroring the behaviour of the LemonLoader installer.
                    let prefix = android_prefix.as_deref().ok_or_else(|| {
                        anyhow!("Manifest element did not declare the android namespace")
                    })?;

                    for permission in STANDARD_PERMISSIONS {
                        if existing_permissions.contains(permission) {
                            debug!("Permission {permission} already present, skipping");
                            continue;
                        }

                        info!("Adding permission {permission}");
                        writer.write(
                            WriteEvent::start_element("uses-permission").attr(
                                xml::name::Name::qualified("name", ANDROID_NS, Some(prefix)),
                                permission,
                            ),
                        )?;
                        writer.write(WriteEvent::end_element())?;
                    }
                }

                writer.write(WriteEvent::end_element())?;
            }
            ReadEvent::EndDocument => break,
            _ => {}
        }
    }

    Ok(String::from_utf8(output)?)
}

/// Gets the value of the attribute with the given name in the android namespace.
fn get_android_attr<'a>(
    attributes: &'a [xml::attribute::OwnedAttribute],
    name: &str,
) -> Option<&'a str> {
    attributes
        .iter()
        .find(|attribute| {
            attribute.name.namespace.as_deref() == Some(mbf_axml::ANDROID_NS_URI)
                && attribute.name.local_name == name
        })
        .map(|attribute| attribute.value.as_str())
}

fn get_android_device_manufacturer() -> Result<String> {
    let output = Command::new("getprop")
        .arg("ro.product.manufacturer")
        .output()
        .context("Getting Android device manufacturer")?;

    let manufacturer = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(manufacturer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unity_version_from_serialized_file() {
        // [metadata size][file size][version][data offset][unknown (serialized v22+)] then the version string.
        // NB: Unity 2021.3.5+ (BONELAB's engine version) writes serialized file version
        // 22, which has the extra field, putting the version string at offset 20.
        let mut data = vec![0u8; 20];
        data.extend_from_slice(b"2022.3.22f1\0");
        let version = parse_unity_version_from_serialized_file(&data).unwrap();
        assert_eq!(version, "2022.3.22");
    }

    #[test]
    fn test_parse_unity_version_from_bundle() {
        // [signature]\0 [version: BE u32] [unity version]\0 [revision]\0 ...
        let mut data = b"UnityFS\0".to_vec();
        data.extend_from_slice(&7u32.to_be_bytes());
        data.extend_from_slice(b"2021.3.16c1\0");
        data.extend_from_slice(b"abcdef123456\0");
        let version = parse_unity_version_from_bundle(&data).unwrap();
        assert_eq!(version, "2021.3.16c1");
    }

    #[test]
    fn test_parse_unity_version_rejects_garbage() {
        let mut data = vec![0u8; 20];
        data.extend_from_slice(b"not-a-version\0");
        data.extend_from_slice(&[0u8; 64]);
        assert!(parse_unity_version_from_serialized_file(&data).is_err());
    }

    #[test]
    fn test_modify_manifest_xml() {
        let input = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.StressLevelZero.BONELAB">
    <uses-permission android:name="android.permission.INTERNET" />
    <application android:label="BONELAB" />
</manifest>"#;

        let output = modify_manifest_xml(input.to_owned()).unwrap();

        // All of the standard permissions should be added (with INTERNET not duplicated).
        assert_eq!(output.matches("uses-permission").count(), 5);
        for permission in STANDARD_PERMISSIONS {
            assert!(output.contains(&format!("android:name=\"{permission}\"")));
        }

        // The application flags should be added.
        assert!(output.contains("android:debuggable=\"true\""));
        assert!(output.contains("android:requestLegacyExternalStorage=\"true\""));

        // Existing attributes should be retained.
        assert!(output.contains("android:label=\"BONELAB\""));
        assert!(output.contains("package=\"com.StressLevelZero.BONELAB\""));

        // The output should also compile back into (binary) AXML, as it will be
        // put back into the APK.
        let mut xml_reader = xml::EventReader::new(Cursor::new(output.as_bytes()));
        let mut axml_output = Cursor::new(Vec::new());
        let mut axml_writer = mbf_axml::AxmlWriter::new(&mut axml_output);
        mbf_axml::xml_to_axml(&mut axml_writer, &mut xml_reader)
            .expect("Modified manifest should compile back into AXML");
        axml_writer.finish().expect("AXML should finish");

        // Re-parse the AXML to check that the added attributes survived the round trip.
        let axml_bytes = axml_output.into_inner();
        let mut axml_cursor = Cursor::new(&axml_bytes);
        let mut axml_reader = mbf_axml::AxmlReader::new(&mut axml_cursor).unwrap();
        let mut permissions = 0;
        let mut has_debuggable = false;
        while let Some(event) = axml_reader.read_next_event().unwrap() {
            match event {
                mbf_axml::Event::StartElement { name, attributes, .. } => {
                    if name == "uses-permission" {
                        permissions += 1;
                    } else if name == "application" {
                        has_debuggable = attributes.iter().any(|attr| {
                            attr.name == "debuggable"
                                && attr.value == mbf_axml::AttributeValue::Boolean(true)
                        });
                    }
                }
                _ => {}
            }
        }
        assert_eq!(permissions, 5);
        assert!(has_debuggable);
    }

    #[test]
    fn test_modify_manifest_xml_keeps_existing_permissions_and_flags() {
        let input = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.test">
    <uses-permission android:name="android.permission.INTERNET" />
    <uses-permission android:name="android.permission.RECORD_AUDIO" />
    <application android:debuggable="true" />
</manifest>"#;

        let output = modify_manifest_xml(input.to_owned()).unwrap();
        // 2 existing permissions are retained, the 3 missing standard permissions are added.
        assert_eq!(output.matches("uses-permission").count(), 5);
        assert_eq!(output.matches("uses-permission").count(), STANDARD_PERMISSIONS.len());
        for permission in STANDARD_PERMISSIONS {
            assert_eq!(
                output.matches(&format!("android:name=\"{permission}\"")).count(),
                1,
                "permission {permission} should appear exactly once"
            );
        }
        assert_eq!(output.matches("debuggable").count(), 1);
    }

    #[test]
    fn test_loader_install_status() {
        use crate::models::response::InstallStatus;

        // Unpatched APKs are missing the loader.
        assert_eq!(
            get_loader_install_status(false, Some("0.6.5.1"), Some("0.6.5.1")),
            InstallStatus::Missing
        );
        // Matching versions are ready.
        assert_eq!(
            get_loader_install_status(true, Some("0.6.5.1"), Some("0.6.5.1")),
            InstallStatus::Ready
        );
        // Older installed versions need an update.
        assert_eq!(
            get_loader_install_status(true, Some("0.6.4"), Some("0.6.5.1")),
            InstallStatus::NeedUpdate
        );
        // Unknown installed versions (e.g. patched with the official installer) don't nag.
        assert_eq!(
            get_loader_install_status(true, None, Some("0.6.5.1")),
            InstallStatus::Ready
        );
    }

    #[test]
    fn test_thunderstore_zip_parses() -> anyhow::Result<()> {
        // Gated on a real Thunderstore package zip being present, so that this test
        // can be run against real-world archives:
        // curl -L "https://thunderstore.io/package/download/bonelib/BoneLib/3.2.2/" -o %TEMP%/bonelib.zip
        let zip_path = std::env::temp_dir().join("bonelib.zip");
        if !zip_path.exists() {
            return Ok(()); // Only runs when the real archive has been downloaded
        }

        let handle = std::fs::File::open(&zip_path)?;
        let mut zip = ZipFile::open(handle).context("Thunderstore zip failed to open")?;

        let entries = zip
            .iter_entry_names()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();
        info!("Thunderstore zip entries: {entries:?}");

        let dlls = entries
            .iter()
            .filter(|name| name.to_lowercase().ends_with(".dll"))
            .count();
        assert!(dlls > 0, "Thunderstore zip should contain at least one DLL");
        Ok(())
    }

    #[test]
    fn test_copy_dir_recursive() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mbf_copy_dir_test");
        std::fs::create_dir_all(&temp_dir)?;

        let from = temp_dir.join("from");
        let to = temp_dir.join("to");
        std::fs::create_dir_all(from.join("sub/dir"))?;
        std::fs::write(from.join("root.txt"), b"root contents")?;
        std::fs::write(from.join("sub/dir/nested.txt"), b"nested contents")?;

        copy_dir_recursive(&from, &to)?;

        assert_eq!(std::fs::read(to.join("root.txt"))?, b"root contents");
        assert_eq!(std::fs::read(to.join("sub/dir/nested.txt"))?, b"nested contents");

        std::fs::remove_dir_all(&temp_dir)?;
        Ok(())
    }

    // Creates a ZIP file at the given path with the given entries, in the same way that
    // MBF writes APKs.
    // NB: ZipFile::open cannot open empty files, so a minimal placeholder entry is
    // hand-crafted to bootstrap the archive.
    fn write_test_zip(path: &Path, entries: &[(&str, &[u8])]) -> anyhow::Result<()> {
        use std::io::Cursor;
        use std::io::Write as _;

        let placeholder_name = b"placeholder.txt";

        // Local file header + (empty) data for the placeholder entry.
        let mut bootstrap: Vec<u8> = Vec::new();
        bootstrap.write_all(b"PK\x03\x04")?;
        bootstrap.write_all(&20u16.to_le_bytes())?; // version needed
        bootstrap.write_all(&0u16.to_le_bytes())?; // flags
        bootstrap.write_all(&0u16.to_le_bytes())?; // method: store
        bootstrap.write_all(&0u32.to_le_bytes())?; // last modified
        bootstrap.write_all(&0u32.to_le_bytes())?; // crc32
        bootstrap.write_all(&0u32.to_le_bytes())?; // compressed len
        bootstrap.write_all(&0u32.to_le_bytes())?; // uncompressed len
        bootstrap.write_all(&(placeholder_name.len() as u16).to_le_bytes())?;
        bootstrap.write_all(&0u16.to_le_bytes())?; // extra len
        bootstrap.write_all(placeholder_name)?;
        let cd_offset = bootstrap.len() as u32;

        // Central directory header for the placeholder entry.
        let mut central_dir: Vec<u8> = Vec::new();
        central_dir.write_all(b"PK\x01\x02")?;
        central_dir.write_all(&20u16.to_le_bytes())?; // version made by
        central_dir.write_all(&20u16.to_le_bytes())?; // version needed
        central_dir.write_all(&0u16.to_le_bytes())?; // flags
        central_dir.write_all(&0u16.to_le_bytes())?; // method: store
        central_dir.write_all(&0u32.to_le_bytes())?; // last modified
        central_dir.write_all(&0u32.to_le_bytes())?; // crc32
        central_dir.write_all(&0u32.to_le_bytes())?; // compressed len
        central_dir.write_all(&0u32.to_le_bytes())?; // uncompressed len
        central_dir.write_all(&(placeholder_name.len() as u16).to_le_bytes())?;
        central_dir.write_all(&0u16.to_le_bytes())?; // extra len
        central_dir.write_all(&0u16.to_le_bytes())?; // comment len
        central_dir.write_all(&0u16.to_le_bytes())?; // disk number
        central_dir.write_all(&0u16.to_le_bytes())?; // internal attrs
        central_dir.write_all(&0u32.to_le_bytes())?; // external attrs
        central_dir.write_all(&0u32.to_le_bytes())?; // local header offset
        central_dir.write_all(placeholder_name)?;

        // End of central directory record.
        bootstrap.extend_from_slice(&central_dir);
        bootstrap.write_all(b"PK\x05\x06")?;
        bootstrap.write_all(&0u16.to_le_bytes())?; // disk number
        bootstrap.write_all(&0u16.to_le_bytes())?; // start of CD disk
        bootstrap.write_all(&1u16.to_le_bytes())?; // CD records on disk
        bootstrap.write_all(&1u16.to_le_bytes())?; // total CD records
        bootstrap.write_all(&(central_dir.len() as u32).to_le_bytes())?; // CD size
        bootstrap.write_all(&cd_offset.to_le_bytes())?;
        bootstrap.write_all(&0u16.to_le_bytes())?; // comment len

        std::fs::write(path, &bootstrap)?;

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        let mut zip = ZipFile::open(file)?;
        for (name, contents) in entries {
            zip.write_file(name, &mut Cursor::new(contents.to_vec()), FileCompression::Deflate)?;
        }
        zip.save()?;
        Ok(())
    }

    #[test]
    fn test_copy_melon_data_to_apk() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mbf_copy_melon_data_test");
        std::fs::create_dir_all(&temp_dir)?;

        let melon_data_path = temp_dir.join("melon_data.zip");
        write_test_zip(&melon_data_path, &[
            ("MelonLoader/net8/MelonLoader.dll", b"melon loader assembly"),
            ("MelonLoader/net8/runtimes/win/lib/net8.0/System.Diagnostics.EventLog.dll", b"win only assembly"),
            ("dotnet/host/fxr/8.0.6/libhostfxr.so", b"hostfxr"),
            ("native/libBootstrap.so", b"bootstrap"),
            ("native/libmain.so", b"melon libmain"),
            ("native/not_a_library.txt", b"should not be copied"),
            ("LICENSE", b"should not be copied"),
        ])?;

        let apk_path = temp_dir.join("test.apk");
        let ggm_data: Vec<u8> = {
            // Header (serialized v22, 20 bytes) + a Unity version string, to check that
            // the version can still be read.
            let mut data = vec![0u8; 20];
            data.extend_from_slice(b"2022.3.22f1\0");
            data
        };
        write_test_zip(&apk_path, &[
            ("AndroidManifest.xml", b"<manifest/>"),
            ("lib/arm64-v8a/libmain.so", b"game's original libmain"),
            ("lib/arm64-v8a/libunity.so", b"stripped libunity"),
            ("assets/bin/Data/globalgamemanagers", &ggm_data),
        ])?;

        // Copy the melon data into the APK, in the same way as the patcher.
        let melon_data_file = std::fs::File::open(&melon_data_path)?;
        let mut melon_data_zip = ZipFile::open(melon_data_file)?;
        let apk_file = std::fs::OpenOptions::new().read(true).write(true).open(&apk_path)?;
        let mut apk_zip = ZipFile::open(apk_file)?;
        copy_melon_data_to_apk(&mut melon_data_zip, &mut apk_zip)?;
        apk_zip.save()?;

        // Check the resulting APK contents.
        let apk_file = std::fs::File::open(&apk_path)?;
        let mut apk_zip = ZipFile::open(apk_file)?;

        assert!(apk_zip.contains_file("assets/MelonLoader/net8/MelonLoader.dll"));
        assert!(apk_zip.contains_file("assets/MelonLoader/net8/runtimes/win/lib/net8.0/System.Diagnostics.EventLog.dll"));
        assert!(apk_zip.contains_file("assets/dotnet/host/fxr/8.0.6/libhostfxr.so"));
        assert!(apk_zip.contains_file("lib/arm64-v8a/libBootstrap.so"));

        // MelonLoader's libmain should have replaced the game's original one.
        assert_eq!(apk_zip.read_file("lib/arm64-v8a/libmain.so")?, b"melon libmain");

        // Non-.so files from the native folder should not have been copied.
        assert!(!apk_zip.contains_file("lib/arm64-v8a/not_a_library.txt"));
        // Entries that are not part of the three known folders should not have been copied.
        assert!(!apk_zip.contains_file("assets/LICENSE"));

        // The Unity version should still be readable from the patched APK.
        // (truncated to major.minor.patch, matching the UnityDependencies release tags)
        assert_eq!(get_unity_version(&mut apk_zip)?, "2022.3.22");

        std::fs::remove_dir_all(&temp_dir)?;
        Ok(())
    }
}
