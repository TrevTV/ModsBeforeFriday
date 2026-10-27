// Mod repository support for BONELAB via Thunderstore.
//
// Thunderstore is the main host of BONELAB MelonLoader mods: https://thunderstore.io/c/bonelab/
// The `/api/v1/package/` endpoint provides a complete list of packages (including all of their
// versions) as a single JSON document, with permissive CORS headers so that it can be fetched
// directly from the browser.

export interface ThunderstoreVersion {
    name: string,
    full_name: string,
    description: string,
    icon: string,
    version_number: string,
    dependencies: string[],
    download_url: string,
    downloads: number,
    date_created: string,
    website_url: string,
    is_active: boolean,
    file_size: number
}

export interface ThunderstorePackage {
    name: string,
    full_name: string,
    owner: string,
    package_url: string,
    date_created: string,
    date_updated: string,
    uuid4: string,
    rating_score: number,
    is_pinned: boolean,
    is_deprecated: boolean,
    has_nsfw_content: boolean,
    categories: string[],
    versions: ThunderstoreVersion[]
}

// A single installable mod within the repo browser, i.e. one version of a package.
export interface RepoMod {
    // The full name of the package, e.g. `bonelib-BoneLib`. Unique within the repo.
    id: string,
    name: string,
    author: string,
    version: string,
    download: string,
    website: string,
    icon: string | null,
    description: string,
    downloads: number,
    rating: number,
    categories: string[]
}

const repoUrl: string = "https://thunderstore.io/c/bonelab/api/v1/package/"

// Loads the full BONELAB mod repository from Thunderstore.
// Only the latest active version of each package is included.
export async function loadRepo(): Promise<RepoMod[]> {
    const response = await fetch(repoUrl);
    if (!response.ok) {
        throw new Error("Thunderstore returned status " + response.status);
    }

    const packages = await response.json() as ThunderstorePackage[];
    return packages
        .map(toLatestRepoMod)
        .filter((mod): mod is RepoMod => mod !== null);
}

// Converts a Thunderstore package into a single RepoMod for its latest active version,
// or None if the package has no active versions (e.g. it is deprecated).
function toLatestRepoMod(pkg: ThunderstorePackage): RepoMod | null {
    // Skip deprecated packages: there is no reason to install these.
    if (pkg.is_deprecated) {
        return null;
    }

    const latest = [...pkg.versions]
        .filter(version => version.is_active)
        .sort((a, b) => b.date_created.localeCompare(a.date_created))[0];

    if (latest === undefined) {
        return null;
    }

    return {
        id: pkg.full_name,
        name: pkg.name,
        author: pkg.owner,
        version: latest.version_number,
        download: latest.download_url,
        website: latest.website_url || pkg.package_url,
        icon: latest.icon || null,
        description: latest.description,
        downloads: latest.downloads,
        rating: pkg.rating_score,
        categories: pkg.categories
    };
}
