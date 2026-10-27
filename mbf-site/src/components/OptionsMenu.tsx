import { Adb, decodeUtf8 } from '@yume-chan/adb';
import { uninstallGame } from '../DeviceModder';
import { useEffect, useState } from 'react';
import { patchApp } from '../Agent';
import { toast } from 'react-toastify';
import '../css/OptionsMenu.css'
import { Collapsible } from './Collapsible';
import { ModStatus } from '../Messages';
import { useSetError, wrapOperation } from '../SyncStore';
import { Log } from '../Logging';
import { confirm } from './Modal';
import { useDeviceStore } from '../DeviceStore';
import { gameId } from '../game_info';

interface OptionsMenuProps {
    setModStatus: (status: ModStatus) => void,
    quit: (err: unknown | null) => void
    modStatus: ModStatus
    visible?: boolean
}

export function OptionsMenu({ quit, modStatus, setModStatus, visible }: OptionsMenuProps) {
    return <div className={`container mainContainer ${visible ? '' : 'hidden'}`} id="toolsContainer">
        <Collapsible title="Mod tools" defaultOpen>
            <ModTools modStatus={modStatus} setModStatus={setModStatus} quit={() => quit(null)} />
        </Collapsible>
        <Collapsible title="ADB log" defaultOpen>
            <AdbLogger />
        </Collapsible>
        <Collapsible title="Update LemonLoader/Repatch">
            <RepatchMenu modStatus={modStatus} setModStatus={setModStatus}/>
        </Collapsible>
    </div>
}

function ModToolButton({ onClick, text, description }: { onClick: () => void, text: string, description: string }) {
    return <div>
      <div>
        <button onClick={onClick}>{text}</button>
      </div>
      <span>{description}</span>
    </div>
}

// Basic tools to do with managing the install.
function ModTools({ quit, modStatus, setModStatus }: {
    quit: () => void,
    modStatus: ModStatus,
    setModStatus: (status: ModStatus) => void}) {
    const { device } = useDeviceStore((store) => ({ device: store.device }));

    return (
      <div id="modTools">
        <ModToolButton
          text="Kill BONELAB"
          description="Immediately closes the game."
          onClick={async () => {
            if (!device) return;

            const setError = useSetError("Failed to kill BONELAB process");
            try {
                await device.subprocess.noneProtocol.spawnWait(`am force-stop ${gameId}`);
                toast.success("Successfully killed BONELAB");
            }   catch(e) {
                setError(e);
            }
          }}
        />
        <ModToolButton
          text="Restart BONELAB"
          description="Immediately closes and restarts the game."
          onClick={async () => {
            if (!device) return;

            const setError = useSetError("Failed to restart BONELAB");
            try {
              await device.subprocess.noneProtocol.spawnWait(`sh -c 'am force-stop ${gameId}; monkey -p ${gameId} -c android.intent.category.LAUNCHER 1'`);
              toast.success("Successfully restarted BONELAB");
            } catch (e) {
              setError(e);
            }
          }}
        />
        <ModToolButton
          text="Update LemonLoader"
          description="Re-patches the game with the latest version of LemonLoader. Your mods are kept."
          onClick={async () => {
            if (!device) return;
            if (
                !(await confirm(
                    "Update LemonLoader?",
                    <>
                        <p>This will re-patch your BONELAB app with the latest version of LemonLoader. The game will be reinstalled, and your mods will be kept.</p>
                        <p>Are you sure you want to continue?</p>
                    </>
                ))
            ) {
                return;
            }
            if (useDeviceStore.getState().device !== device) return;

            await wrapOperation(
              "Updating LemonLoader",
              "Failed to update LemonLoader",
              async () => {
                setModStatus(await patchApp(device, true));
                toast.success("Successfully updated LemonLoader!");
              }
            );
          }}
        />
        <ModToolButton
          text="Uninstall BONELAB"
          description="Uninstalls the game and quits this tool. Your mods are kept in /sdcard/MelonLoader."
          onClick={async () => {
            if (!device) return;
            if (
                !(await confirm(
                    "Uninstall BONELAB?",
                    <>
                        <p>This will uninstall BONELAB and quit this tool. You will need to reinstall the game from the Meta store.</p>
                        <p>Your mods will not be deleted, as they are stored outside of the game.</p>
                        <p>Are you sure you want to continue?</p>
                    </>
                ))
            ) {
                return;
            }
            if (useDeviceStore.getState().device !== device) return;

            const setError = useSetError("Failed to uninstall BONELAB");
            try {
              await uninstallGame(device);
              quit();
            } catch (e) {
              setError(e);
            }
          }}
        />
      </div>
    );
}

// Re-patches the game, which updates LemonLoader to the latest version.
function RepatchMenu({ modStatus, setModStatus }: {
    modStatus: ModStatus,
    setModStatus: (status: ModStatus) => void
}) {
    const { device } = useDeviceStore((store) => ({ device: store.device }));

    return <>
        <p>If your installation has issues (for example the modloader has an update available, or the game was patched with
            an outdated version of LemonLoader), you can re-patch the game with the latest version of LemonLoader.</p>
        <p>Mods are stored outside of the game, so they will not be affected.</p>
        <br/>
        <button onClick={async () => {
            if (!device) return;

            await wrapOperation("Repatching BONELAB", "Failed to repatch", async () => {
                setModStatus(await patchApp(device, true));
                toast.success("Successfully repatched");
            })
        }}>Repatch game</button>
    </>
}

// Starts recording log messages from `adb logcat`. The promise returned will not complete until `getCancelled` returns a `true` value.
// Returns a blob containing the logcat messages recorded.
async function logcatToBlob(device: Adb, getCancelled: () => boolean): Promise<Blob> {
    Log.debug("Starting `logcat` process");

    // First clear the logcat buffer - we only want logs from events happening after the "start logcat" button is pressed.
    await device.subprocess.noneProtocol.spawnWait("logcat -c");

    const process = await device.subprocess.noneProtocol.spawn("logcat");
    let killed = false;

    Log.debug("Generating logs");
    const stdout = process.output.getReader();
    const logs = [];

    while(true) {
        const bytesRead = (await stdout.read()).value;
        if(bytesRead != null) {
            logs.push(decodeUtf8(bytesRead));
        }   else    {
            break;
        }

        // NB: It is vital that, after we kill logcat, we read any messages that have not yet been read
        // before returning. Otherwise, the unread messages cause the ADB implementation to hang on all future requests!
        if(getCancelled() && !killed) {
            Log.debug("Killing `logcat` process");
            await process.kill();
            killed = true;
        }
    }

    Log.debug("Providing blob of logs");
    return new Blob(logs, { type: 'text/plain' })
}

function AdbLogger() {
    const [logging, setLogging] = useState(false);
    const [logFile, setLogFile] = useState(null as Blob | null);
    const [waitingForLog, setWaitingForLog] = useState(false);
    const { device } = useDeviceStore((store)=> ({device: store.device}));

    useEffect(() => {
        if(!logging || !device) {
            return () => {};
        }

        // Begin gathering logs, making sure to remove the previous log file/blob
        setWaitingForLog(false);
        setLogFile(null);
        let cancelled = false;
        logcatToBlob(device, () => cancelled)
            .then(log => {
                setLogFile(log);
                setWaitingForLog(false);
            })
            .catch(e => Log.error("Failed to get ADB log " + e));

        // When the value of `logging` changes to false, use the cleanup function to tell the `log` function to stop getting logs as soon as it can.
        return () => {
            cancelled = true;
            setWaitingForLog(true);
        };
    }, [logging]);

    return <>
        <p>This feature allows you to get a log of what's going on inside your Quest, useful for modders to fix bugs with their mods.</p>
        <p>Click the button below, <span className="warning">and keep your headset plugged in.</span> Open the game and do whatever it is that was causing you issues, then click the button again.</p>

        <p className="warning"></p>
        {!logging ?
            <button onClick={async () => setLogging(true)}>Start Logging</button> :
            <button onClick={() => setLogging(false)}>Stop Logging</button>}
            <br/>

        {waitingForLog && <p>Please wait while the log file generates . . .</p>}
        {logFile !== null && <a href={URL.createObjectURL(logFile)} download={"logcat.log"}><button>Download Log</button></a>}
    </>
}
