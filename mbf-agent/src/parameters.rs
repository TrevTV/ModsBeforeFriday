//! Module containing all of the fixed file paths used by MBF, for easy changing throughout the project.
//!
//! NB: Unlike the old (Beat Saber/QMOD based) layout, most of the mod data lives inside the APK itself
//! when using MelonLoader (LemonLoader).
//! The only mod data stored on external storage is within `/sdcard/MelonLoader/{apk_id}`,
//! which is where LemonLoader itself loads mods and plugins from.
//! See: https://github.com/LemonLoader/MelonLoader/wiki/Mod-Installation-and-Preferences

use std::sync::{OnceLock, LazyLock};

/// A OnceLock that contains the default parameters for the agent.
static _PARAMETERS: OnceLock<AgentParameters> = OnceLock::new();

/// Initializes the parameters for the agent with the given game ID and ignore_package_id flag.
/// This function is to be called once at the start of the program to set up the parameters.
///
/// # Arguments
///
/// * `game_id` - The game ID to be used for the agent.
/// * `ignore_package_id` - A boolean flag indicating whether to skip the game ID check.
///
/// # Returns
///
/// Returns true if the parameters were successfully initialized, false if they were already initialized.
pub fn init_parameters(game_id: &str, ignore_package_id: bool) -> bool {
    _PARAMETERS.set(AgentParameters::new(game_id, ignore_package_id)).is_ok()
}

/// A static reference to the parameters for the agent.
pub static PARAMETERS: LazyLock<AgentParameters> = LazyLock::new(|| _PARAMETERS.get_or_init(|| AgentParameters::new("com.StressLevelZero.BONELAB", false)).clone());

/// A struct that contains the parameters for the agent, including various file paths and settings.
#[derive(Clone)]
pub struct AgentParameters {
    /// The APK ID of the app being modded.
    pub apk_id: String,

    /// If true, the game ID check is skipped during mod import.
    pub _ignore_package_id: bool,

    /// The folder that MelonLoader loads mods from.
    /// Mods that are not enabled are moved to the `disabled` subfolder within this directory.
    pub mods: String,

    /// The folder that disabled mod files are moved into.
    pub disabled_mods: String,

    /// The folder that MelonLoader loads plugins from.
    pub plugins: String,

    /// The folder that MelonLoader mods store their configuration files in.
    pub user_data: String,

    /// Directory containing OBBs for the app.
    pub obb_dir: String,

    /// The path that the OBB directory is moved to while the game is being patched,
    /// so that it isn't deleted when the app is uninstalled.
    pub obb_backup_dir: String,

    /// The app's external data directory, which is wiped when the app is uninstalled.
    pub app_data_dir: String,

    /// The path that the app's external data directory is moved to while patching.
    pub app_data_backup_dir: String,

    /// A folder that MBF uses to download temporary files.
    pub mbf_downloads: String,

    /// Temporary folder used by MBF during patching.
    pub temp: String,

    /// Path to the MBF resource cache.
    pub res_cache: String,
}

/// Implements the `AgentParameters` struct, which contains various paths and settings used by the agent.
impl AgentParameters {
    /// Creates a new instance of `AgentParameters` with paths initialized based on the provided APK ID.
    ///
    /// # Arguments
    ///
    /// * `apk_id` - A string slice that holds the APK ID, which is used to construct various paths.
    ///
    /// # Returns
    ///
    /// Returns an instance of `AgentParameters` with all fields populated based on the provided APK ID.
    ///
    /// # Example
    ///
    /// ```rust
    /// let apk_id = "com.example.app";
    /// let paths = AgentParameters::new(apk_id);
    /// println!("{}", paths.mods); // Outputs: /sdcard/MelonLoader/com.example.app/Mods
    /// ```
    pub fn new<'a>(apk_id: &str, ignore_package_id: bool) -> AgentParameters {
        let local_tmp = "/data/local/tmp";

        let melon_root = format!("/sdcard/MelonLoader/{apk_id}").to_string();
        let mods = format!("{melon_root}/Mods").to_string();
        let disabled_mods = format!("{mods}/disabled").to_string();
        let plugins = format!("{melon_root}/Plugins").to_string();
        let user_data = format!("{melon_root}/UserData").to_string();
        let obb_dir = format!("/sdcard/Android/obb/{apk_id}/").to_string();
        let obb_backup_dir = format!("/sdcard/Android/{apk_id}.obb.lemonify").to_string();
        let app_data_dir = format!("/sdcard/Android/data/{apk_id}").to_string();
        let app_data_backup_dir = format!("/sdcard/Android/data/{apk_id}.lemonify_backup").to_string();

        Self {
            apk_id: apk_id.to_string(),
            _ignore_package_id: ignore_package_id,
            mods,
            disabled_mods,
            plugins,
            user_data,
            obb_dir,
            obb_backup_dir,
            app_data_dir,
            app_data_backup_dir,
            mbf_downloads: format!("{local_tmp}/mbf/downloads"),
            temp: format!("{local_tmp}/mbf/tmp"),
            res_cache: format!("{local_tmp}/mbf/res-cache"),
        }
    }
}
