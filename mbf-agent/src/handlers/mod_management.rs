//! This file contains the request handlers relating to mod management (i.e. toggling or removing mods).
//! Adding new mods is considered to be an "importing" operation - check the [Import Handlers](crate::handlers::import)

use std::collections::HashMap;

use crate::models::response::Response;
use anyhow::{Context, Result};
use log::info;

/// Handles `SetModsEnabled` [Requests](crate::requests::Request).
///
/// # Returns
/// The [Response] to the request (variant `ModSyncResult`)
pub(super) fn handle_set_mods_enabled(statuses: HashMap<String, bool>) -> Result<Response> {
    let mut mod_manager = super::load_mod_manager()?;

    let mut error = String::new();

    for (file_name, new_status) in statuses {
        let already_enabled = match mod_manager.get_mod(&file_name) {
            Some(m) => m.is_enabled,
            None => {
                error.push_str(&format!("Mod with ID {file_name} did not exist\n"));
                continue;
            }
        };

        if new_status && !already_enabled {
            match mod_manager.set_mod_enabled(&file_name, true) {
                Ok(_) => info!("Enabled {file_name}"),
                Err(err) => error.push_str(&format!("Failed to enable {file_name}: {err:#}\n")),
            }
        } else if !new_status && already_enabled {
            match mod_manager.set_mod_enabled(&file_name, false) {
                Ok(_) => info!("Disabled {file_name}"),
                Err(err) => error.push_str(&format!("Failed to disable {file_name}: {err:#}\n")),
            }
        }
    }

    Ok(Response::ModSyncResult {
        installed_mods: super::get_mod_models(mod_manager)?,
        failures: if !error.is_empty() {
            if error.ends_with('\n') {
                error.pop();
            }

            Some(error)
        } else {
            None
        },
    })
}

/// Handles `RemoveMod` [Requests](crate::requests::Request).
///
/// # Returns
/// The [Response] to the request (variant `Mods`)
pub(super) fn handle_remove_mod(id: String) -> Result<Response> {
    let mut mod_manager = super::load_mod_manager()?;
    mod_manager.remove_mod(&id).context("Removing mod")?;

    Ok(Response::Mods {
        installed_mods: super::get_mod_models(mod_manager)?,
    })
}
