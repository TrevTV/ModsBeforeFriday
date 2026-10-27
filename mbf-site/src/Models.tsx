export interface Mod {
    id: string,
    name: string,
    version: string | null,
    repo_id: string | null,
    is_enabled: boolean
}

export type ModLoader = "MelonLoader" | "Unknown";

export type InstallStatus = "Ready" | "NeedUpdate" | "Missing";

export interface AppInfo {
    version: string,
    obb_present: boolean,
    loader_installed: ModLoader | null,
    installed_loader_version: string | null,
    unity_version: string | null,
    unity_deps_available: boolean
}

export interface LoaderInfo {
    latest_version: string,
    install_status: InstallStatus
}

export type LogLevel = "Error" | "Warn" | "Info" | "Debug" | "Trace";

export {}
