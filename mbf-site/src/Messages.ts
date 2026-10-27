import { Mod, AppInfo, LoaderInfo, LogLevel } from "./Models";

export interface GetModStatus {
    type: 'GetModStatus',
}

export interface Patch {
    type: 'Patch',
    // If true, this is an update of an existing patched install rather than a first-time patch.
    repatch: boolean
}

export interface SetModsEnabled {
    type: 'SetModsEnabled',
    statuses: { [id: string]: boolean }
}

export interface RemoveMod {
    type: 'RemoveMod',
    id: string
}

export interface Import {
    type: 'Import',
    from_path: string
}

export interface ImportUrl {
    type: 'ImportUrl',
    from_url: string,
    // Details of the repository package this mod came from, if applicable.
    // These are stored alongside the mod so that update checks can be performed later.
    id: string | null,
    name: string | null,
    version: string | null
}

export interface AgentParameters {
    agent_parameters: {
        game_id: String,
        ignore_package_id: boolean
    }
}

export type Request = GetModStatus |
    Patch |
    SetModsEnabled |
    RemoveMod |
    Import |
    ImportUrl;

export interface Mods {
    type: 'Mods',
    installed_mods: Mod[]
}

export interface ModSyncResult {
    type: 'ModSyncResult',
    installed_mods: Mod[],
    failures: string | null
}

export interface ImportedMod {
    type: 'ImportedMod',
    installed_mods: Mod[],
    imported_id: string
}

export interface NonModFile {
    type: 'NonModFile'
}

export interface ImportResult {
    result: ImportResultType,
    used_filename: string,
    type: 'ImportResult'
}

export type ImportResultType = ImportedMod | NonModFile;

// Represents whether a particular part of the modded game is installed and up to date.
export type InstallStatus = "Ready" | "NeedUpdate" | "Missing";

export interface ModStatus {
    type: 'ModStatus',
    app_info: AppInfo | null,
    loader_info: LoaderInfo | null,
    installed_mods: Mod[],
}

export interface LogMsg {
    type: 'LogMsg',
    message: string,
    level: LogLevel
}

export type Response = LogMsg | ModStatus | Mods | ImportResult | ModSyncResult;
