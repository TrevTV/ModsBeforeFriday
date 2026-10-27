//! Handles requests related to the patching of the game.

use std::path::Path;

use log::info;

use crate::{models::response::Response, parameters::PARAMETERS, patching};
use anyhow::{anyhow, Context, Result};

/// Handles `Patch` [Requests](crate::requests::Request).
///
/// # Returns
/// The [Response](crate::models::response::Response) to the request (variant `ModStatus`)
pub(super) fn handle_patch(repatch: bool) -> Result<Response> {
    let app_info =
        super::mod_status::get_app_info()?.ok_or(anyhow!("Cannot patch when app not installed"))?;
    let res_cache = crate::load_res_cache()?;

    std::fs::create_dir_all(&PARAMETERS.temp)?;

    if repatch {
        info!("Repatching the game to update MelonLoader");
    } else {
        info!("Patching the game for the first time");
    }

    let result = patching::mod_game(Path::new(&PARAMETERS.temp), &app_info, &res_cache)
        .context("Modding the game");

    // No matter what, make sure that all temporary files are gone.
    std::fs::remove_dir_all(&PARAMETERS.temp).ok();

    result?;

    info!("Patching complete, checking the new state of the installation");
    super::mod_status::handle_get_mod_status()
}
