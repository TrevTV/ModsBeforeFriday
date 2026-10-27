//! Module for mod management within MBF.
//!
//! MelonLoader mods are simply `.dll` files that get placed within `/sdcard/MelonLoader/{apk_id}/Mods`,
//! so unlike the old QMOD-based system there is no mod manifest or dependency system.
//! Instead, this mod manager works directly with the DLL files within the mods folder:
//! - Importing a mod copies its DLL file(s) into the mods folder.
//! - "Disabling" a mod moves its DLL into the `disabled` subfolder, from which MelonLoader does not load it.
//! - Uninstalling a mod deletes its DLL file.
//!
//! If a mod was installed from a mod repository (e.g. Thunderstore), the details of the package it
//! came from are stored in a small `.mbf.json` file alongside the DLL, which allows update checks
//! to be performed later. Mods imported manually (i.e. a loose DLL) have no such metadata.

use std::{
    fs::OpenOptions,
    io::{Cursor, Seek},
    path::Path,
};

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};
use mbf_zip::ZipFile;
use serde::{Deserialize, Serialize};

use crate::parameters::PARAMETERS;

/// The file extension of the metadata files written alongside installed mod DLLs.
const META_EXTENSION: &str = ".mbf.json";

/// Metadata about the package a mod was installed from, stored in a `{mod file stem}.mbf.json`
/// file within the mods folder.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModMeta {
    /// The ID of the package the mod came from, e.g. `bonelib-BoneLib` for Thunderstore packages.
    pub id: String,
    /// The display name of the package the mod came from.
    pub name: String,
    /// The version of the package that was installed.
    pub version: Option<String>,
    /// The URL the package was downloaded from.
    pub download_url: Option<String>,
}

/// Represents an installed mod, i.e. a mod DLL within the mods folder
/// (or the disabled folder, if it is disabled).
#[derive(Clone, Debug)]
pub struct Mod {
    /// The file name of the mod DLL, e.g. `BoneLib.dll`. Unique within the mods folder.
    pub file_name: String,
    /// Whether the mod is currently enabled, i.e. present within the mods folder itself
    /// rather than the `disabled` subfolder.
    pub is_enabled: bool,
    /// Metadata about the repository package this mod was installed from, if applicable.
    pub meta: Option<ModMeta>,
}

impl Mod {
    /// The ID of the mod used by the frontend, which is simply its file name.
    pub fn id(&self) -> &str {
        &self.file_name
    }

    /// The display name of the mod: the package name if known, otherwise the DLL file name.
    pub fn name(&self) -> String {
        match &self.meta {
            Some(meta) => meta.name.clone(),
            None => self.file_name.trim_end_matches(".dll").to_owned(),
        }
    }
}

/// A structure to manage the mods installed for the game.
pub struct ModManager {
    mods: Vec<Mod>,
}

impl ModManager {
    /// Creates a new [ModManager]. The mod list is not loaded until [ModManager::load_mods] is called.
    pub fn new() -> Self {
        Self { mods: Vec::new() }
    }

    /// Gets all loaded mods.
    pub fn get_mods(&self) -> &[Mod] {
        &self.mods
    }

    /// Gets the mod with the given file name, if it is loaded.
    pub fn get_mod(&self, file_name: &str) -> Option<&Mod> {
        self.mods.iter().find(|m| m.file_name == file_name)
    }

    /// Loads the list of installed mods by scanning the mods folder
    /// (and the `disabled` subfolder within it).
    pub fn load_mods(&mut self) -> Result<()> {
        self.mods.clear();

        // Ensure that the mods folders exist.
        std::fs::create_dir_all(&PARAMETERS.mods).context("Creating mods directory")?;
        std::fs::create_dir_all(&PARAMETERS.disabled_mods)
            .context("Creating disabled mods directory")?;
        std::fs::create_dir_all(&PARAMETERS.plugins).context("Creating plugins directory")?;
        std::fs::create_dir_all(&PARAMETERS.user_data).context("Creating user data directory")?;

        let mut load_from = |dir: &str, is_enabled: bool| -> Result<()> {
            for entry in std::fs::read_dir(dir).with_context(|| format!("Reading {dir}"))? {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        warn!("Failed to read entry in {dir}: {err}");
                        continue;
                    }
                };

                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                let file_name = match path.file_name().and_then(|name| name.to_str()) {
                    Some(file_name) => file_name.to_owned(),
                    None => continue,
                };

                // Only .dll files are mods; anything else (e.g. metadata files) is skipped.
                if !file_name.to_lowercase().ends_with(".dll") {
                    continue;
                }

                let meta = Self::load_meta_for(&path).unwrap_or_else(|err| {
                    warn!("Failed to load metadata for {file_name}: {err}");
                    None
                });

                self.mods.push(Mod {
                    file_name,
                    is_enabled,
                    meta,
                });
            }
            Ok(())
        };

        load_from(&PARAMETERS.mods, true)?;
        load_from(&PARAMETERS.disabled_mods, false)?;

        // Sort by name for a consistent presentation.
        self.mods
            .sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()));
        Ok(())
    }

    /// Loads the `.mbf.json` metadata for the mod DLL at the given path, if present.
    fn load_meta_for(dll_path: &Path) -> Result<Option<ModMeta>> {
        let meta_path = meta_path_for(dll_path);
        if !meta_path.exists() {
            return Ok(None);
        }

        let json_data = std::fs::read(&meta_path).context("Reading mod metadata")?;
        let meta: ModMeta = serde_json::from_slice(&json_data).context("Parsing mod metadata")?;
        Ok(Some(meta))
    }

    /// Imports a mod from the given path, which may be either a mod DLL or a ZIP archive
    /// containing one or more mod DLLs (e.g. a Thunderstore package).
    ///
    /// # Arguments
    /// * `from_path` - The path to the file to import. This file is not deleted by this function.
    /// * `meta` - Optional metadata about the package this mod came from, which will be saved
    ///   alongside each imported DLL.
    ///
    /// # Returns
    /// The file names of the mods that were imported.
    pub fn import_mod(&mut self, from_path: &Path, meta: Option<ModMeta>) -> Result<Vec<String>> {
        let file_name = from_path
            .file_name()
            .ok_or(anyhow!("No file name in {from_path:?}"))?
            .to_string_lossy()
            .to_string();

        let extension = file_name
            .rsplit('.')
            .next()
            .map(|ext| ext.to_lowercase())
            .ok_or(anyhow!("No file extension in filename {file_name}"))?;

        match extension.as_str() {
            "dll" => {
                let imported = file_name.clone();
                self.copy_dll_into_mods(from_path, &file_name, meta.as_ref())?;
                self.load_mods()?;
                Ok(vec![imported])
            }
            "zip" => self.import_mod_zip(from_path, meta),
            _ => Err(anyhow!(
                "File extension `.{extension}` is not a mod; Mods are `.dll` files or `.zip` packages"
            )),
        }
    }

    /// Imports a ZIP archive (e.g. a Thunderstore package) by extracting all DLL files within it
    /// into the mods folder.
    fn import_mod_zip(&mut self, from_path: &Path, meta: Option<ModMeta>) -> Result<Vec<String>> {
        let zip_handle = std::fs::File::open(from_path).context("Opening mod ZIP")?;
        let mut zip = ZipFile::open(zip_handle).context("Mod was invalid ZIP archive")?;

        let entry_names = zip
            .iter_entry_names()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();

        let dll_entries = entry_names
            .iter()
            .filter(|name| {
                !name.ends_with('/') && name.to_lowercase().ends_with(".dll")
            })
            .cloned()
            .collect::<Vec<String>>();

        if dll_entries.is_empty() {
            return Err(anyhow!(
                "The ZIP archive did not contain any mod DLL files, so there was nothing to install"
            ));
        }

        let mut imported = Vec::new();
        for entry_name in dll_entries {
            // NB: DLLs are flattened into the mods folder, since MelonLoader only loads
            // DLLs directly within the mods folder.
            let file_name = entry_name
                .rsplit('/')
                .next()
                .expect("Entry names always contain a file name")
                .to_owned();

            debug!("Extracting {entry_name} as {file_name}");
            let mut contents = Cursor::new(zip.read_file(&entry_name).context("Reading DLL from ZIP")?);
            self.write_mod_file(&PARAMETERS.mods, &file_name, &mut contents)?;

            if let Some(meta) = &meta {
                Self::write_meta_file(&PARAMETERS.mods, &file_name, meta)?;
            }

            imported.push(file_name);
        }

        self.load_mods()?;
        Ok(imported)
    }

    /// Copies a DLL file into the mods folder, overwriting any existing mod with the same name.
    fn copy_dll_into_mods(
        &self,
        from_path: &Path,
        file_name: &str,
        meta: Option<&ModMeta>,
    ) -> Result<()> {
        let mut source = std::fs::File::open(from_path).context("Opening mod DLL")?;
        self.write_mod_file(&PARAMETERS.mods, file_name, &mut source)?;

        if let Some(meta) = meta {
            Self::write_meta_file(&PARAMETERS.mods, file_name, meta)?;
        } else {
            // Remove any stale metadata from a previous install.
            let stale_meta = Path::new(&PARAMETERS.mods).join(meta_file_name(file_name));
            if stale_meta.exists() {
                std::fs::remove_file(&stale_meta).ok();
            }
        }

        Ok(())
    }

    /// Writes the given contents to a file within the given mod folder, replacing any existing file.
    fn write_mod_file(
        &self,
        folder: &str,
        file_name: &str,
        contents: &mut (impl std::io::Read + Seek),
    ) -> Result<()> {
        let dest_path = Path::new(folder).join(file_name);
        let mut handle = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&dest_path)
            .context("Creating destination mod file")?;
        std::io::copy(contents, &mut handle).context("Writing mod file")?;
        Ok(())
    }

    /// Writes the metadata JSON for the mod with the given file name.
    fn write_meta_file(folder: &str, file_name: &str, meta: &ModMeta) -> Result<()> {
        let meta_path = Path::new(folder).join(meta_file_name(file_name));
        let json = serde_json::to_vec_pretty(meta).context("Serializing mod metadata")?;
        std::fs::write(meta_path, json).context("Writing mod metadata")?;
        Ok(())
    }

    /// Deletes the mod with the given file name, along with its metadata file if present.
    /// Does nothing if no mod exists with this file name.
    pub fn remove_mod(&mut self, file_name: &str) -> Result<()> {
        let folder = self.folder_of(file_name);
        let dll_path = Path::new(&folder).join(file_name);
        if dll_path.exists() {
            std::fs::remove_file(&dll_path).context("Deleting mod DLL")?;
        }
        let meta_path = Path::new(&folder).join(meta_file_name(file_name));
        if meta_path.exists() {
            std::fs::remove_file(&meta_path).context("Deleting mod metadata")?;
        }

        self.load_mods()?;
        Ok(())
    }

    /// Enables or disables the mod with the given file name by moving it between the mods
    /// folder and the `disabled` subfolder.
    /// Does nothing if the mod is already in the requested state.
    pub fn set_mod_enabled(&mut self, file_name: &str, enabled: bool) -> Result<()> {
        let current_folder = self.folder_of(file_name);
        let target_folder = if enabled {
            &PARAMETERS.mods
        } else {
            &PARAMETERS.disabled_mods
        };

        if current_folder == target_folder.as_str() {
            return Ok(());
        }

        let dll_path = Path::new(&current_folder).join(file_name);
        if !dll_path.exists() {
            return Err(anyhow!("Mod with file name {file_name} did not exist"));
        }

        let target_path = Path::new(target_folder).join(file_name);
        if target_path.exists() {
            std::fs::remove_file(&target_path).context("Removing existing mod file")?;
        }
        std::fs::rename(&dll_path, &target_path).context("Moving mod file")?;

        // Move the metadata file along with the DLL, if present.
        let meta_path = Path::new(&current_folder).join(meta_file_name(file_name));
        if meta_path.exists() {
            let target_meta_path = Path::new(target_folder).join(meta_file_name(file_name));
            if target_meta_path.exists() {
                std::fs::remove_file(&target_meta_path).ok();
            }
            std::fs::rename(&meta_path, &target_meta_path).context("Moving mod metadata")?;
        }

        info!(
            "{} mod {file_name}",
            if enabled { "Enabled" } else { "Disabled" }
        );

        self.load_mods()?;
        Ok(())
    }

    /// Gets the folder that the mod with the given file name is currently in.
    /// This works from the currently loaded mod list, and falls back to assuming the mod
    /// is enabled if it is unknown.
    fn folder_of(&self, file_name: &str) -> String {
        match self.get_mod(file_name) {
            Some(mod_info) if !mod_info.is_enabled => PARAMETERS.disabled_mods.clone(),
            _ => PARAMETERS.mods.clone(),
        }
    }
}

/// Gets the name of the metadata file that belongs to the mod DLL with the given file name.
fn meta_file_name(file_name: &str) -> String {
    match file_name.strip_suffix(".dll").or_else(|| file_name.strip_suffix(".DLL")) {
        Some(stem) => format!("{stem}{META_EXTENSION}"),
        None => format!("{file_name}{META_EXTENSION}"),
    }
}

/// Gets the expected path of the metadata file that belongs to the mod DLL at the given path.
fn meta_path_for(dll_path: &Path) -> std::path::PathBuf {
    match dll_path.file_name().and_then(|name| name.to_str()) {
        Some(file_name) => dll_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(meta_file_name(file_name)),
        None => dll_path.with_extension(META_EXTENSION),
    }
}
