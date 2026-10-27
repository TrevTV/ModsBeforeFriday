import { Adb } from '@yume-chan/adb';
import { loadModStatus, patchApp } from "./Agent";
import { ReactNode, useEffect, useState } from 'react';
import { ModLoader } from './Models';
import { ModStatus } from './Messages';
import './css/DeviceModder.css';
import { LogWindow, LogWindowControls } from './components/LogWindow';
import { ErrorModal, Modal } from './components/Modal';
import { ModManager } from './components/ModManager';
import { OpenLogsButton } from './components/OpenLogsButton';
import { wrapOperation } from './SyncStore';
import { useDeviceStore } from './DeviceStore';
import { gameId } from './game_info';

interface DeviceModderProps {
    device: Adb,
    // Quits back to the main menu, optionally giving an error that caused the quit.
    quit: (err: unknown | null) => void
}

export async function uninstallGame(device: Adb) {
    await device.subprocess.noneProtocol.spawnWait(`pm uninstall ${gameId}`);
}

export function DeviceModder(props: DeviceModderProps) {
    const [modStatus, setModStatus] = useState(null as ModStatus | null);
    const { quit } = props;
    const { device } = useDeviceStore((state) => ({ device: state.device }));

    useEffect(() => {
        if (!device) { return; } // If the device is not set, do not attempt to load mod status.
        loadModStatus(device)
            .then(loadedModStatus => setModStatus(loadedModStatus))
            .catch(err => quit(err));
    }, [device]);

    // Fun "ocean" of IF statements, hopefully covering every possible state of an installation!
    if (modStatus === null) {
        return <div className='container mainContainer fadeIn'>
            <h2>Checking BONELAB installation</h2>
            <span className="floatRight"><LogWindowControls/></span>
            <p>This might take a minute or so the first few times.</p>
            <LogWindow />
        </div>
    } else if (modStatus.app_info === null) {
        return <div className='container mainContainer'>
            <h1>BONELAB is not installed</h1>
            <p>Please install BONELAB from the store and then refresh the page.</p>
            <h3>Think you have BONELAB installed?</h3>
            <p>Sometimes, it looks like a game is installed in your headset, when it actually isn't (a bug in the Meta software).</p>
            <p>This can be fixed by going to the main <b>Applications</b> menu inside your Quest, clicking the 3 dots next to BONELAB, and clicking <b>Uninstall</b>. Finally, reinstall BONELAB from the Meta store and refresh this page to try again.</p>
        </div>
    } else if (!modStatus.app_info.obb_present) {
        // BONELAB keeps the vast majority of its assets in an OBB file, so a missing OBB
        // means the installation is corrupt.
        return <NoObb quit={() => quit(undefined)}/>
    } else if (modStatus.app_info.loader_installed === null) {
        // App is vanilla: check that we have everything we need to patch it.
        if (modStatus.loader_info === null) {
            return <NoInternet />
        } else if (!modStatus.app_info.unity_deps_available) {
            return <NoUnityDeps version={modStatus.app_info.version} unityVersion={modStatus.app_info.unity_version} />
        } else {
            return <PatchingMenu
                quit={quit}
                modStatus={modStatus}
                onCompleted={status => setModStatus(status)}
            />
        }
    } else if (modStatus.app_info.loader_installed === 'MelonLoader') {
        return <ValidModLoaderMenu modStatus={modStatus} setModStatus={setModStatus} quit={() => quit(null)}/>
    } else {
        return <IncompatibleLoader loader={modStatus.app_info.loader_installed} quit={() => quit(null)} />
    }
}

function NoObb({ quit }: { quit: () => void }) {
    const { device } = useDeviceStore((state) => ({ device: state.device }));

     return <div className="container mainContainer">
        <h1>OBB not present</h1>
        <p>BONELAB keeps most of its assets in an OBB file, which has not been found in your installation.</p>
        <p>This means your installation is corrupt. You will need to uninstall BONELAB with the button below, and reinstall the latest version from the Meta store.</p>
        <button onClick={async () => {
            if (!device) return;

            await uninstallGame(device);
            quit();
        }}>Uninstall BONELAB</button>
     </div>
}

function NoInternet() {
    return <div className='container mainContainer'>
        <OpenLogsButton />
        <h1>No internet</h1>
        <p>It seems as though <b>your Quest</b> has no internet connection.</p>
        <p>To mod BONELAB, this tool needs to download LemonLoader and an unstripped copy of the game's libunity.so.
            <br />This occurs on your Quest's connection. Please make sure that WiFi is enabled, then refresh the page.</p>
    </div>
}

function NoUnityDeps({ version, unityVersion }: { version: string, unityVersion: string | null }) {
    return <div className='container mainContainer'>
        <h1>Unsupported game version</h1>
        <p>You have BONELAB v{version} installed (built with Unity {unityVersion ?? "UNKNOWN"}), but there is no unstripped
            <code> libunity.so </code>available for this Unity version yet.</p>
        <p>Patching the game requires an unstripped copy of libunity.so for the exact Unity version the game was built with,
            which is generated automatically whenever BONELAB updates. If the game has only just updated, check back in a day or two!</p>
        <p className="warning">If a new BONELAB version has been out for a while but you still see this message, the latest version of the game
            may not be moddable yet. In the meantime, you can try reinstalling an older version of the game.</p>
    </div>
}

function ValidModLoaderMenu({ modStatus, setModStatus, quit }: {
    modStatus: ModStatus,
    setModStatus: (status: ModStatus) => void
    quit: () => void}) {
    const { device } = useDeviceStore((state) => ({ device: state.device }));

    return <>
        <div className='container mainContainer'>
            <OpenLogsButton />
            <h1>App is modded</h1>
            <UpdateInfo modStatus={modStatus} quit={quit}/>

            {modStatus.loader_info === null && <p className="warning">Could not fetch LemonLoader release info - update checks are unavailable until your headset has an internet connection.</p>}
            <InstallStatus
                    modStatus={modStatus}
                    onFixed={status => setModStatus(status)}/>
            <h4>Not sure what to do next?</h4>
            <NextSteps />
        </div>

        <ModManager modStatus={modStatus}
            setMods={mods => {
                setModStatus({ ...modStatus, installed_mods: mods });
            }}
            setModStatus={status => setModStatus(status)}
            quit={quit}
        />
    </>
}

interface InstallStatusProps {
    modStatus: ModStatus
    onFixed: (newStatus: ModStatus) => void
}

function InstallStatus(props: InstallStatusProps) {
    const { modStatus, onFixed } = props;

    const loaderStatus = modStatus.loader_info?.install_status;
    const { device } = useDeviceStore((state) => ({ device: state.device }));

    if (loaderStatus === "Ready" || loaderStatus === undefined) {
        return <p>Everything should be ready to go! &#9989;</p>
    } else {
        return <div>
            <h3 className="warning">Problems found with your install:</h3>
            <p>These can be easily fixed by clicking the button below.</p>
            <ul>
                {loaderStatus === "Missing" &&
                    <li>Modloader not found &#10060;</li>}
                {loaderStatus === "NeedUpdate" &&
                    <li>An update for LemonLoader is available</li>}
            </ul>
            <button onClick={async () => {
                if (!device) return;

                await wrapOperation("Updating LemonLoader", "Failed to update LemonLoader", async () =>
                    onFixed(await patchApp(device, true)));
            }}>Update LemonLoader</button>
        </div>
    }
}

function UpdateInfo({ modStatus, quit }: { modStatus: ModStatus, quit: () => void }) {
    const { device } = useDeviceStore((state) => ({ device: state.device }));

    const [updateWindowOpen, setUpdateWindowOpen] = useState(false);

    return <>
        <p>Your BONELAB install is modded with LemonLoader
            {modStatus.app_info?.installed_loader_version !== null && modStatus.app_info?.installed_loader_version !== undefined
                ? <> (version {modStatus.app_info.installed_loader_version})</> : null}.</p>

        <Modal isVisible={updateWindowOpen}>
            <h2>Update BONELAB</h2>
            <p>To update to the latest version of the game, simply:</p>
            <ol>
                <li>Uninstall BONELAB with the button below.</li>
                <li>Reinstall BONELAB in your headset.</li>
                <li>Open back up this tool to mod the version you just installed.</li>
            </ol>
            <button onClick={async () => {
                if (!device) return;
                await uninstallGame(device);
                quit();
            }}>Uninstall BONELAB</button>
            <button onClick={() => setUpdateWindowOpen(false)} className="discreetButton">Cancel</button>
            <br/><br/>
            <h3>What about my mods/saves?</h3>
            <ul>
                <li><em>Mods are stored outside of the game</em> (in /sdcard/MelonLoader), so they will still be installed once you re-patch the new version.</li>
                <li><em>Saves may be lost</em> when uninstalling: the game keeps its saves inside its own data folder, which is removed on uninstall. The tool will attempt to preserve them while patching, but cannot restore them when you uninstall manually.</li>
            </ul>
        </Modal>
    </>
}

interface PatchingMenuProps {
    modStatus: ModStatus,
    onCompleted: (newStatus: ModStatus) => void,
    quit: (err: unknown) => void
}

function PatchingMenu(props: PatchingMenuProps) {
    const [isPatching, setIsPatching] = useState(false);
    const [patchingError, setPatchingError] = useState(null as string | null);

    const { onCompleted, modStatus } = props;
    const { device } = useDeviceStore((state) => ({ device: state.device }));

    if(!isPatching) {
        return <div className='container mainContainer'>
            <OpenLogsButton />

            <h1>Mod BONELAB with LemonLoader</h1>
            <p>Your app has version {modStatus.app_info!.version}, built with Unity {modStatus.app_info!.unity_version ?? "UNKNOWN"}, which is supported by mods!</p>
            <p>BONELAB is modded with <b>LemonLoader</b> (a fork of MelonLoader for standalone VR headsets). To set this up, this tool will patch
                your BONELAB app by adding the LemonLoader runtime to it, then reinstall the game.</p>
            <p>Once this is done, you will be able to manage your mods below, and in-game once LemonLoader has initialised (the first launch may take a few minutes).</p>

            <h2 className='warning'>READ CAREFULLY</h2>
            <p>Mods are not supported by Stress Level Zero. You may experience bugs and crashes that you wouldn't in a vanilla game.</p>
            <p className='warning'>During patching, the game will be uninstalled and reinstalled, which means your connection to the headset must stay stable.
                Do not disconnect your device during this process.</p>
            <div>
                <button disabled={!device} className="largeCenteredButton" onClick={async () => {
                    if (!device) return;

                    setIsPatching(true);
                    try {
                        onCompleted(await patchApp(device, false));
                    } catch (e) {
                        setPatchingError(String(e));
                        setIsPatching(false);
                    }
                }}>Mod the app</button>
            </div>

            <ErrorModal
                isVisible={patchingError != null}
                title={"Failed to install mods"}
                description={'An error occured while patching ' + patchingError}
                onClose={() => setPatchingError(null)} />
        </div>
    } else {
        return <div className='container mainContainer'>
            <h1>App is being patched</h1>
            <p>This should only take a few minutes, but could take much, much longer if your internet connection is slow.</p>
            <span className="floatRight"><LogWindowControls/></span>
            <p className='warning'>You must not disconnect your device during this process.</p>
            <LogWindow />
        </div>
    }
}

function ClickableLink({ onClick, children }: { onClick: () => void, children: ReactNode }) {
    return <a className="clickableLink" onClick={onClick}>{children}</a>
}

interface IncompatibleLoaderProps {
    loader: ModLoader,
    quit: () => void
}

function IncompatibleLoader(props: IncompatibleLoaderProps) {
    const { loader, quit } = props;
    const { device } = useDeviceStore((state) => ({ device: state.device }));

    return <div className='container mainContainer'>
        <h1>Incompatible Modloader</h1>
        <p>Your app is patched with {loader === 'Unknown' ? "an unknown" : "an unsupported"} modloader, which isn't compatible with this tool.</p>
        <p>You will need to uninstall your app and reinstall the latest vanilla version so that the app can be re-patched with LemonLoader.</p>
        <p>Do not be alarmed! Your mods are stored outside of the game, so they will not be lost.</p>

        <button onClick={async () => {
            if (!device) return;

            await uninstallGame(device);
            quit();
        }}>Uninstall BONELAB</button>
    </div>
}

function NextSteps() {
    return <ul>
        <li>Load up the game. The first launch after patching can take a while: LemonLoader needs to generate interop assemblies.</li>
        <li>Once loaded, a LemonLoader menu should be available in-game that shows your mods.</li>
        <li>Download additional mods below!</li>
    </ul>
}
