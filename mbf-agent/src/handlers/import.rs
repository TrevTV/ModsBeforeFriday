use std::path::{Path, PathBuf};

use crate::{
    downloads,
    models::response::{self, ImportResultType, Response},
    parameters::PARAMETERS,
};
use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};

/// Handles `ImportUrl` [Requests](crate::models::request::Request).
///
/// Downloads the file from the given URL, then imports it as a mod.
/// If the download is from a mod repository, the details of the package are saved alongside
/// the mod so that update checks can be performed later.
///
/// # Returns
/// The [Response](crate::models::response::Response) to the request (variant `ImportResult`)
pub(super) fn handle_import_url(
    from_url: String,
    id: Option<String>,
    name: Option<String>,
    version: Option<String>,
) -> Result<Response> {
    std::fs::create_dir_all(&PARAMETERS.mbf_downloads)?;
    let download_path = Path::new(&PARAMETERS.mbf_downloads).join("import_from_url");

    info!("Downloading {}", from_url);
    let downloaded_name: Option<String> =
        downloads::download_file_with_attempts(&crate::get_dl_cfg(), &download_path, &from_url)?;

    // Work out a usable file name for the download: prefer a name from the server, but
    // fall back to sniffing the file contents, since some CDNs (e.g. Thunderstore's) do
    // not send a Content-Disposition header at all.
    let filename = match &downloaded_name {
        Some(filename) if has_mod_extension(filename) => filename.clone(),
        _ => match sniff_mod_type(&download_path) {
            Some(mod_type) => {
                info!("Download had no usable file name; detected type `{mod_type}` from its contents");
                format!("import_from_url.{mod_type}")
            }
            None => {
                return Err(anyhow!(
                    "The downloaded file is neither a mod DLL nor a ZIP archive"
                ))
            }
        },
    };

    // NB: The file is renamed so that the mod import (which derives the file type from
    // the file name) recognises it.
    let final_path = Path::new(&PARAMETERS.mbf_downloads).join(&filename);
    if final_path != download_path {
        std::fs::rename(&download_path, &final_path)
            .context("Renaming downloaded file for import")?;
    }

    let meta = super::make_mod_meta(id, name, version, Some(from_url));

    // Attempt to import the renamed file as a mod, removing the temporary file if this fails.
    handle_import(
        &final_path,
        Some(filename),
        meta.as_ref().map(|meta| meta.id.clone()),
        meta.as_ref().map(|meta| meta.name.clone()),
        meta.as_ref().and_then(|meta| meta.version.clone()),
    )
}

/// Checks whether the given file name has an extension that MBF knows how to import as a mod.
fn has_mod_extension(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    lower.ends_with(".dll") || lower.ends_with(".zip")
}

/// Sniffs the contents of the file at the given path to determine whether it is a mod DLL
/// or a ZIP archive, returning the file extension it should be treated as.
/// Returns `None` if the file is neither.
fn sniff_mod_type(path: &Path) -> Option<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;

    // PK\x03\x04 = ZIP archive local file header.
    if magic[0..2] == [0x50, 0x4B] {
        return Some("zip".to_owned());
    }

    // MZ = DOS/PE executable header (i.e. a mod DLL).
    if magic[0..2] == [0x4D, 0x5A] {
        return Some("dll".to_owned());
    }

    None
}

/// Handles `Import` [Requests](crate::models::request::Request).
///
/// # Returns
/// The [Response](crate::models::response::Response) to the request (variant `ImportResult`)
pub(super) fn handle_import(
    from_path: impl AsRef<Path> + std::fmt::Debug,
    override_filename: Option<String>,
    meta_id: Option<String>,
    meta_name: Option<String>,
    meta_version: Option<String>,
) -> Result<Response> {
    let mut mod_manager = super::load_mod_manager()?;

    let filename = match override_filename {
        Some(filename) => filename,
        None => from_path
            .as_ref()
            .file_name()
            .ok_or(anyhow!("No filename in {from_path:?}"))?
            .to_string_lossy()
            .to_string(),
    };

    let path = from_path.as_ref().to_owned();
    info!("Attempting to import from {filename}");

    let import_result = import_as_mod(&mut mod_manager, &path, meta_id, meta_name, meta_version);

    // Make sure to remove the temporary file in the case that importing the file failed.
    match import_result {
        Ok(result) => Ok(Response::ImportResult {
            result,
            used_filename: filename,
        }),
        Err(err) => {
            match std::fs::remove_file(path) {
                Ok(_) => {}
                Err(err) => warn!("Failed to remove temporary file: {err}"),
            }

            Err(err)
        }
    }
}

/// Attempts to import the file at the given path as a mod (either a DLL or a ZIP archive).
///
/// # Arguments
/// * `mod_manager` - A mod manager that has all existing mods loaded already.
/// * `from_path` - The path to the file to import.
/// * `meta_id`/`meta_name`/`meta_version` - The details of the repository package the mod came
///   from, if it was downloaded from one.
///
/// # Returns
/// If successful, an [ImportResultType] of variant `ImportedMod` detailing the file name of the
/// imported mod and the new full list of installed mods.
fn import_as_mod(
    mod_manager: &mut crate::mod_man::ModManager,
    from_path: &PathBuf,
    meta_id: Option<String>,
    meta_name: Option<String>,
    meta_version: Option<String>,
) -> Result<ImportResultType> {
    debug!("Loading {from_path:?} as a mod");
    let meta = super::make_mod_meta(meta_id, meta_name, meta_version, None);
    let imported = mod_manager
        .import_mod(from_path, meta)
        .context("Importing mod")?;

    let imported_id = imported
        .first()
        .cloned()
        .unwrap_or_else(|| "unknown".to_owned());

    Ok(ImportResultType::ImportedMod {
        installed_mods: get_installed_mods(mod_manager)?,
        imported_id,
    })
}

fn get_installed_mods(
    mod_manager: &crate::mod_man::ModManager,
) -> Result<Vec<response::ModModel>> {
    Ok(mod_manager
        .get_mods()
        .iter()
        .map(response::ModModel::from)
        .collect())
}
