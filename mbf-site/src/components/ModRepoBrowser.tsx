import { useEffect, useMemo, useState } from "react";
import { RepoMod, loadRepo } from "../ModsRepo";
import { ModRepoCard } from "./ModRepoCard";
import { gt as semverGt, valid as semverValid } from "semver";
import { Mod } from "../Models";
import { Log } from "../Logging";
import '../css/ModRepoBrowser.css';
import DownloadIcon from '../icons/download-icon.svg';
import UpdateIcon from '../icons/update-icon.svg';

interface ModRepoBrowserProps {
    onDownload: (mods: RepoMod[]) => void,
    existingMods: Mod[]
    visible?: boolean
}

interface ModDisplayInfo {
    mod: RepoMod,
    alreadyInstalled: boolean,
    needUpdate: boolean
}

export function ModRepoBrowser(props: ModRepoBrowserProps) {
    const { onDownload } = props;
    const [allMods, setAllMods] = useState(null as RepoMod[] | null);
    const [failedToLoad, setFailedToLoad] = useState(false);
    const [attempt, setAttempt] = useState(0);
    const [flagged, setFlagged] = useState([] as RepoMod[]);
    const [search, setSearch] = useState("");

    // Removes a mod from the list of flagged mods
    function unflag(mod: RepoMod) {
        setFlagged(flagged.filter(flaggedMod => flaggedMod.id != mod.id || flaggedMod.version != mod.version))
    }

    useEffect(() => {
        (async () => {
            Log.debug("Loading mods from Thunderstore");

            try {
                setAllMods(await loadRepo());
            } catch (e) {
                Log.warn("Failed to load mod repo: " + e);
                setFailedToLoad(true);
            }
        })();
    }, [attempt]);

    // Matches the repo mods up with the installed mods to work out which are installed
    // and which have updates available.
    const displayMods: ModDisplayInfo[] | null = useMemo(() => {
        if (allMods === null) {
            return null;
        }

        return allMods.map(mod => {
            const existingInstall = props.existingMods.find(existing => existing.repo_id === mod.id);

            return {
                alreadyInstalled: existingInstall !== undefined,
                needUpdate: existingInstall !== undefined
                    && existingInstall.version !== null
                    && semverValid(mod.version) !== null
                    && semverValid(existingInstall.version) !== null
                    && semverGt(mod.version, existingInstall.version),
                mod
            };
        });
    }, [allMods, props.existingMods]);

    // Applies the search filter to the mods to display.
    const filteredMods = useMemo(() => {
        if (displayMods === null) {
            return null;
        }

        const searchLower = search.toLowerCase().trim();
        const matching = displayMods.filter(displayInfo => {
            if (searchLower === "") {
                return true;
            }

            return displayInfo.mod.name.toLowerCase().includes(searchLower)
                || displayInfo.mod.author.toLowerCase().includes(searchLower)
                || displayInfo.mod.description.toLowerCase().includes(searchLower);
        });

        // Updates first, then most downloads, then alphabetical.
        return matching.sort((a, b) => {
            if (a.needUpdate !== b.needUpdate) {
                return a.needUpdate ? -1 : 1;
            }

            if (a.mod.downloads !== b.mod.downloads) {
                return b.mod.downloads - a.mod.downloads;
            }

            const nameA = a.mod.name.toLowerCase().trim();
            const nameB = b.mod.name.toLowerCase().trim();
            if (nameA > nameB) {
                return 1;
            } else if (nameA < nameB) {
                return -1;
            } else {
                return 0;
            }
        });
    }, [displayMods, search]);

    // The list of mods shown in the browser. Installed and up-to-date mods are hidden
    // unless there is an update available or the user is searching for them.
    const shownMods = useMemo(() => {
        if (filteredMods === null) {
            return null;
        }

        const searching = search.toLowerCase().trim() !== "";
        return filteredMods
            .filter(displayInfo => searching || displayInfo.needUpdate || !displayInfo.alreadyInstalled)
            .slice(0, 100); // Avoid rendering thousands of cards at once.
    }, [filteredMods, search]);

    if (allMods === null || shownMods === null) {
        if (failedToLoad) {
            return <div className="container">
                <h1>Failed to load mods</h1>
                <p>Please check that your internet is working.</p>
                <button onClick={() => {
                    setAttempt(attempt + 1);
                    setFailedToLoad(false);
                }}>Try again</button>
            </div>
        } else {
            return <h1>Loading mods...</h1>
        }
    }

    const hasFlaggedModsToUpdate = flagged.some(mod => displayMods!.find(info => info.mod.id === mod.id)?.needUpdate);
    const hasFlaggedNewMods = flagged.length > 0 && !hasFlaggedModsToUpdate;

    return <>
        <input className="repoSearch" type="text" placeholder="Search Thunderstore mods..."
            value={search} onChange={event => setSearch(event.target.value)} />
        <p className="repoCount">{shownMods.length} of {filteredMods!.length} mods shown</p>

        {flagged.length > 0 &&
            <button className={`installMarked fadeIn ${props.visible ? "" : "hidden"}`} onClick={() => {
                onDownload(flagged);
                setFlagged([]);
            }}>
                {hasFlaggedModsToUpdate && hasFlaggedNewMods && "Install/Update "}
                {hasFlaggedModsToUpdate && !hasFlaggedNewMods && "Update "}
                {!hasFlaggedModsToUpdate && hasFlaggedNewMods && "Install "}
                {flagged.length} {flagged.length > 1 ? "mods" : "mod"}
                <img src={hasFlaggedNewMods ? DownloadIcon : UpdateIcon} alt="A download icon" />
            </button>}

        <div className="mod-list fadeIn">
            {shownMods.map(displayInfo =>
                <ModRepoCard
                        mod={displayInfo.mod}
                        key={displayInfo.mod.id + "-" + displayInfo.mod.version}
                        update={displayInfo.needUpdate}
                        onInstall={() => {
                            onDownload([displayInfo.mod]);
                            unflag(displayInfo.mod);
                        }}
                        isFlagged={flagged.find(mod => mod.id === displayInfo.mod.id && mod.version === displayInfo.mod.version) !== undefined}
                        setFlagged={isFlagged => {
                            if(isFlagged) {
                                setFlagged([...flagged, displayInfo.mod]);
                            }   else    {
                                unflag(displayInfo.mod);
                            }
                        }} />
            )}
        </div>
    </>
}
