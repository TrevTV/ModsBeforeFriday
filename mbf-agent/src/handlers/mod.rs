//! Responsible for handling all requests sent to the backend (`mbf-agent`) from the frontend.

use crate::{
    mod_man::{ModManager, ModMeta},
    models::{
        request::Request,
        request::RequestEnum,
        response::{self, Response},
    },
};
use anyhow::{Context, Result};

mod import;
mod mod_management;
mod mod_status;
mod patching;

/// Handles a [Request] from the MBF frontend.
///
/// # Arguments
/// * `request` - The request to be handled.
///
/// # Returns
/// If successful, a [Response] to be sent back to the frontend.
pub fn handle_request(request: Request) -> Result<Response> {
    match request.request {
        RequestEnum::GetModStatus => mod_status::handle_get_mod_status(),
        RequestEnum::Patch { repatch } => patching::handle_patch(repatch),
        RequestEnum::RemoveMod { id } => mod_management::handle_remove_mod(id),
        RequestEnum::SetModsEnabled { statuses } => mod_management::handle_set_mods_enabled(statuses),
        RequestEnum::Import { from_path } => {
            import::handle_import(from_path, None, None, None, None)
        }
        RequestEnum::ImportUrl {
            from_url,
            id,
            name,
            version,
        } => import::handle_import_url(from_url, id, name, version),
    }
}

/// Loads the installed mods from the mods folder.
/// The game does not need to be installed for this to work, as mods are stored outside of it.
fn load_mod_manager() -> Result<ModManager> {
    let mut mod_manager = ModManager::new();
    mod_manager.load_mods().context("Loading installed mods")?;
    Ok(mod_manager)
}

/// Converts a mod manager into the list of mod models sent to the frontend.
fn get_mod_models(mod_manager: ModManager) -> Result<Vec<response::ModModel>> {
    Ok(mod_manager
        .get_mods()
        .iter()
        .map(response::ModModel::from)
        .collect())
}

/// Builds the metadata to store alongside an imported repository package.
pub(super) fn make_mod_meta(
    id: Option<String>,
    name: Option<String>,
    version: Option<String>,
    download_url: Option<String>,
) -> Option<ModMeta> {
    let id = id?;
    Some(ModMeta {
        name: name.unwrap_or_else(|| id.clone()),
        id,
        version,
        download_url,
    })
}
