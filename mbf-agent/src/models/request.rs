//! Models used for communication *from the frontend to the backend*.

use serde::Deserialize;

/// The default value for the game_id parameter.
fn default_game_id() -> String {
    "com.StressLevelZero.BONELAB".to_string()
}

/// The default value for the ignore_package_id parameter.
fn default_ignore_package_id() -> bool {
    false
}

/// The default value for the agent_parameters parameter.
fn default_agent_parameters() -> AgentRequestParameters {
    AgentRequestParameters {
        game_id: default_game_id(),
        ignore_package_id: default_ignore_package_id(),
    }
}

/// A struct that contains the game ID to be used for the agent and a flag indicating whether to skip the game ID check.
#[derive(Deserialize)]
pub struct AgentRequestParameters {
    /// The ID of the game APK on the device.
    #[serde(default = "default_game_id")]
    pub game_id: String,

    /// If true, the game ID check is skipped during mod import.
    #[serde(default = "default_ignore_package_id")]
    pub ignore_package_id: bool,
}

/// A struct that represents a request from the frontend to the agent.
#[derive(Deserialize)]
pub struct Request {
    /// The parameters for the agent, including the game ID and whether to skip the game ID check.
    #[serde(default = "default_agent_parameters")]
    pub agent_parameters: AgentRequestParameters,

    #[serde(flatten)]
    pub request: RequestEnum,
}

/// An enum that represents the different types of requests that can be made to the agent.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum RequestEnum {
    /// Gathers several pieces of data to check that the installation is modded appropriately, including:
    /// - Whether the APK is patched with MelonLoader (LemonLoader)
    /// - The APK version and Unity version
    /// - The modloader installation status
    /// - The mods that are currently installed
    GetModStatus,
    /// Patches BONELAB to add MelonLoader to the APK, then reinstalls it.
    ///
    /// This includes downloading the latest MelonLoader data, an unstripped libunity.so for the
    /// game's Unity version, patching the manifest, signing and reinstalling the game.
    ///
    /// If `repatch` is true, this is an update to an existing patched install;
    /// otherwise this is a first-time patch. (The operation is the same either way.)
    ///
    /// Returns a `ModStatus` response reflecting the state of the installation after patching.
    Patch {
        repatch: bool,
    },
    /// Installs or uninstalls any number of mods, by moving their DLL files between the
    /// mods folder and the disabled subfolder.
    ///
    /// Returns a `ModSyncResult` response.
    SetModsEnabled {
        statuses: std::collections::HashMap<String, bool>,
    },
    /// Deletes the mod with the given file name.
    /// Returns a `Mods` message containing the mods now installed.
    RemoveMod {
        id: String,
    },
    /// Imports a mod from the given path on the quest.
    /// The file may either be a mod DLL, or a ZIP archive containing one or more mod DLLs.
    ///
    /// Returns an `ImportResult` message containing the mods now installed, and the file name of the one that was imported.
    Import {
        from_path: String,
    },
    /// Downloads the file from the given URL and then attempts to import it.
    ///
    /// If this is a mod from a repository (e.g. Thunderstore), the details of the package should
    /// be given, so that update checks can be performed later.
    ///
    /// Returns an `ImportResult` message.
    ImportUrl {
        from_url: String,
        id: Option<String>,
        name: Option<String>,
        version: Option<String>,
    },
}
