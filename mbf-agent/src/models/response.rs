//! Models used for communication *from the backend back to the frontend*

use serde::{Deserialize, Serialize};

use crate::mod_man;

#[derive(Serialize)]
pub struct AppInfo {
    pub loader_installed: Option<ModLoader>,
    /// The version of MelonLoader the game was patched with, if it was patched by this tool.
    pub installed_loader_version: Option<String>,
    pub obb_present: bool,
    #[serde(skip_serializing)]
    pub path: String,
    pub version: String,
    /// The Unity version the game was built with, if it could be detected.
    pub unity_version: Option<String>,
    /// True only if an unstripped libunity.so is known to be available for the detected
    /// Unity version, which is required to patch the game.
    pub unity_deps_available: bool,
}

/// Information about the MelonLoader installation and updates.
/// None if the latest release info could not be fetched (e.g. no internet connection).
#[derive(Serialize)]
pub struct LoaderInfo {
    /// The latest available LemonLoader (MelonLoader) release version.
    pub latest_version: String,
    pub install_status: InstallStatus,
}

/// An enum that represents whether a particular part of the modded game is:
#[derive(Copy, Clone, Serialize, Debug, PartialEq, Eq)]
pub enum InstallStatus {
    /// Installed and up to date
    Ready,
    /// Installed but not up to date
    NeedUpdate,
    /// Not installed
    Missing,
}

#[derive(Serialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Serialize)]
pub enum ModLoader {
    MelonLoader,
    Unknown,
}

/// What type of file a file was imported as, and details about the resulting file.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ImportResultType {
    ImportedMod {
        installed_mods: Vec<ModModel>,
        imported_id: String,
    },
    /// The file was not a mod (i.e. not a DLL or ZIP archive), so the import failed.
    NonModFile,
}

/// The trimmed version of the Mod type that is sent to the web client.
#[derive(Serialize, Deserialize)]
pub struct ModModel {
    /// The file name of the mod DLL, which is also the ID used to enable/disable/remove the mod.
    pub id: String,
    pub name: String,
    /// The version of the repository package the mod was installed from, if known.
    pub version: Option<String>,
    /// The ID of the repository package the mod was installed from, if known.
    pub repo_id: Option<String>,
    pub is_enabled: bool,
}

impl From<&mod_man::Mod> for ModModel {
    fn from(value: &mod_man::Mod) -> Self {
        Self {
            id: value.id().to_owned(),
            name: value.name(),
            version: value.meta.as_ref().and_then(|meta| meta.version.clone()),
            repo_id: value.meta.as_ref().map(|meta| meta.id.clone()),
            is_enabled: value.is_enabled,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum Response {
    ModStatus {
        // None if the game is not installed.
        app_info: Option<AppInfo>,

        // All mods currently found in the mods folder
        installed_mods: Vec<ModModel>,

        // Information about the status of the MelonLoader installation.
        // None if the latest release info could not be fetched (e.g. no internet connection).
        loader_info: Option<LoaderInfo>,
    },
    Mods {
        installed_mods: Vec<ModModel>,
    },
    ModSyncResult {
        // The new state of the installed mods after the operation
        installed_mods: Vec<ModModel>,
        // If any of the mods failed to install/uninstall, this will be Some with a string
        // containing a list of the errors generated.
        failures: Option<String>,
    },
    ImportResult {
        result: ImportResultType, // The result of importing the file.
        used_filename: String, // The filename that was actually used to determine how to import the mod.
    },
    // Sent to relay progress information during the modding process.
    // This will NOT be the final message sent.
    LogMsg {
        message: String,
        level: LogLevel,
    },
}
