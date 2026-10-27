import { AdbSync, AdbSyncWriteOptions, Adb, encodeUtf8, AdbPacketData, AdbCommand, packetListeners } from "@yume-chan/adb";
import { Consumable, TextDecoderStream, MaybeConsumable, ReadableStream } from "@yume-chan/stream-extra";
import { Request, Response, LogMsg, ModStatus, Mods, ImportResult, ModSyncResult, AgentParameters } from "./Messages";
import { AGENT_SHA1 } from './agent_manifest';
import { Log } from './Logging';
import { gameId, ignorePackageId } from './game_info';

const AgentPath: string = "/data/local/tmp/mbf-agent";
const UploadsPath: string = "/data/local/tmp/mbf/uploads/";

// Converts the provided byte array into a ReadableStream that can be fed into ADB.
function readableStreamFromByteArray(array: Uint8Array): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      controller.enqueue(array);
      controller.close();
    },
  });
}

export async function prepareAgent(adb: Adb) {
  Log.info("Preparing agent: used to communicate with your Quest.");

  Log.debug("Latest agent SHA1 " + AGENT_SHA1);

  const existingSha1 = (await adb.subprocess.noneProtocol.spawnWaitText(`sha1sum ${AgentPath} | cut -f 1 -d " "`))
    .trim()
    .toUpperCase();
  Log.debug("Existing agent SHA1: " + existingSha1);
  const existingUpToDate = AGENT_SHA1 == existingSha1.trim().toUpperCase();
  if(existingUpToDate) {
    Log.info("Agent is up to date");
  } else  {
    await overwriteAgent(adb);
  }
}

function adbPacketToString(data: AdbPacketData): string {
  let commandString: string = "<unknown>";
  switch(data.command) {
    case AdbCommand.Auth:
      commandString = "AUTH";
      break;
    case AdbCommand.Close:
      commandString = "CLSE";
      break;
    case AdbCommand.Connect:
      commandString = "CNXN";
      break;
    case AdbCommand.Okay:
      commandString = "OKAY";
      break;
    case AdbCommand.Open:
      commandString = "OPEN";
      break;
    case AdbCommand.Write:
      commandString = "WRTE";
      break;
  }

  return `ADB command: ${commandString}, arg0: ${data.arg0}, arg1: ${data.arg1}, payload size: ${data.payload.length}`;
}

export function installLoggers() {
  packetListeners.onPacketRead = packet => Log.trace("READ: " + adbPacketToString(packet));
  packetListeners.onPacketWritten = packet => Log.trace("Sent: " + adbPacketToString(packet));
}

export async function overwriteAgent(adb: Adb) {
  const sync = await adb.sync();
  console.group("Downloading and overwriting agent on Quest");
  try {
    Log.debug("Removing existing agent");
    await adb.subprocess.noneProtocol.spawnWait("rm " + AgentPath)
    Log.debug("Downloading agent, this might take a minute")
    await saveAgent(sync);
    Log.debug("Making agent executable");
    await adb.subprocess.noneProtocol.spawnWait("chmod +x " + AgentPath);

    Log.info("Agent is ready");
  } finally {
    sync.dispose();
    console.groupEnd();
  }
}

async function saveAgent(sync: AdbSync) {
  // Timeout, in seconds, before the app will treat the agent upload as failed and terminate the connection.
  const AGENT_UPLOAD_TIMEOUT: number = 30;

  const agent: Uint8Array = await downloadAgent();
  const file: ReadableStream<MaybeConsumable<Uint8Array>> = readableStreamFromByteArray(agent);

  const options: AdbSyncWriteOptions = {
    filename: AgentPath,
    file
  };

  Log.info("Writing agent to quest!");
  const timeoutPromise = new Promise((_, reject) => {
    setTimeout(() => reject(new Error(`Did not finish pushing agent after ${AGENT_UPLOAD_TIMEOUT} seconds.\n`
        + `In practice, pushing the agent takes less than a second, so this is a bug. Please report this issue including information about `
        + `which web browser you are using.`
    )), AGENT_UPLOAD_TIMEOUT * 1000);
  });

  await Promise.race([timeoutPromise, sync.write(options)])
}

async function downloadAgent(): Promise<Uint8Array> {
  const MAX_ATTEMPTS: number = 3;
  const PROGRESS_UPDATE_INTERVAL = 1000; // Time between download progress updates, in milliseconds

  let ok = false;
  let attempt = 1;
  do {
    try {
      // Use XMLHttpRequest adapted to work with promises/async to fetch the agent
      // Previously this used the fetch API, and there was some suggestion that various issues regarding the download
      // "hanging" before data was received were caused by fetch.
      // So, to see if it fixes the problem, we have changed to XMLHttpRequest.
      const xhr = new XMLHttpRequest();
      await new Promise((resolve, reject) => {
        xhr.open('GET', "mbf-agent", true);
        xhr.responseType = "arraybuffer";

        xhr.onload = function() {
          if(xhr.status >= 200 && xhr.status < 300) {
            resolve(xhr.response);
          } else  {
            reject(xhr.status)
          }
        };
        xhr.onerror = function() {
          reject(xhr.status)
        }

        let lastReadTime = new Date().getTime();
        xhr.onprogress = function(event) {
          if(!event.lengthComputable) {
            return;
          }

          // Do not spam with progress updates: only every second or so
          const timeNow = new Date().getTime();
          if(timeNow - lastReadTime > PROGRESS_UPDATE_INTERVAL) {
            // Calculate the percentage of the download that has completed
            const percentComplete = (event.loaded / event.total) * 100.0;
            lastReadTime = timeNow;
            Log.info(`Download ${Math.round(percentComplete * 10) / 10}% complete`);
          }
        }

        xhr.send();
      })

      return new Uint8Array(xhr.response);
    } catch(e) {
      Log.info("Failed to fetch agent, status " + e);
    }

    attempt++;
    if(attempt <= MAX_ATTEMPTS) {
      Log.info(`Failed to download agent, trying again... (attempt ${attempt}/${MAX_ATTEMPTS})`);
    }
  } while(!ok && attempt <= MAX_ATTEMPTS);

  throw new Error("Failed to fetch agent after multiple attempts.\nDid you lose internet connection just after you loaded the site?\n\nIf not, then please report this issue, including a screenshot of the browser console window!");
}

async function sendRequest(adb: Adb, request: Request): Promise<Response> {
  let wrappedRequest: AgentParameters & Request = {
    agent_parameters: {
      game_id: gameId,
      ignore_package_id: ignorePackageId
    },
    ...request
  }
  let command_buffer = encodeUtf8(JSON.stringify(wrappedRequest) + "\n");

  let agentProcess = await adb.subprocess.noneProtocol.spawn(AgentPath);

  const stdin = agentProcess.stdin.getWriter();
  try {
    stdin.write(new Consumable(command_buffer));
  } finally {
    stdin.releaseLock();
  }

  let exited = false;
  agentProcess.exited.then(() => exited = true);
  adb.disconnected.then(() => exited = true);

  const reader = agentProcess.output
    // TODO: Not totally sure if this will handle non-ASCII correctly.
    // Doesn't seem to consider that a chunk might not be valid UTF-8 on its own
    .pipeThrough(new TextDecoderStream())
    .getReader();

  console.group("Agent Request");
  let buffer = "";
  let response: Response | null = null;
  while(!exited) {
    const result = await reader.read();
    const receivedStr = result.value;
    if(receivedStr === undefined) {
      continue;
    }

    // TODO: This is fairly inefficient in terms of memory usage
    // (although we aren't receiving a huge amount of data so this might be OK)
    buffer += receivedStr;
    const messages = buffer.split("\n");
    buffer = messages[messages.length - 1];

    for(let i = 0; i < messages.length - 1; i++) {
      // Parse each newline separated message as a Response
      let msg_obj: Response;
      try {
        msg_obj = JSON.parse(messages[i]) as Response;
      } catch(e) {
        throw new Error("Agent message " + messages[i] + " was not valid JSON");
      }
      if(msg_obj.type === "LogMsg") {
        const log_obj = msg_obj as LogMsg;
        Log.emitEvent(log_obj);

        // Errors need to be thrown later in the function
        if(msg_obj.level === 'Error') {
          response = msg_obj;
        }
      } else  {
        // The final message is the only one that isn't of type `log`.
        // This contains the actual response data
        response = msg_obj;
      }
    }
  }
  console.groupEnd();

  // "None" protocol is necessary as we pass input strings longer than one line in terminal sometimes
  // So using the shell protocol can cause messages to fail: particularly, when patching the game,
  // we need to send the whole manifest over.
  // Unfortunately, it seems yume chan ADB doesn't support getting the exit code from the none protocol, so we can't tell if the
  // agent quit prematurely.

  // Don't worry too much! The agent should always return 0, even if it encounters an error, since errors are sent via a JSON message.

  if(response === null) {
    throw new Error("Received error response from agent");
  } else if(response.type === 'LogMsg') {
    const log = response as LogMsg;
    throw new Error("`" + log.message + "`");
  } else  {
    return response;
  }
}

// Gets the status of mods from the quest, i.e. whether the app is patched, and what mods are currently installed.
export async function loadModStatus(device: Adb): Promise<ModStatus> {
  await prepareAgent(device);

  return await sendRequest(device, {
      type: 'GetModStatus'
  }) as ModStatus;
}

// Tells the backend to attempt to uninstall/install the given mods, depending on the new install status provided in `changesRequested`.
export async function setModStatuses(device: Adb,
  changesRequested: { [id: string]: boolean }): Promise<ModSyncResult> {
  let response = await sendRequest(device, {
      type: 'SetModsEnabled',
      statuses: changesRequested
  });

  return response as ModSyncResult;
}

export async function importFile(device: Adb,
    file: File): Promise<ImportResult> {
  const sync = await device.sync();
  const tempPath = UploadsPath + file.name;
  try {

    Log.debug("Uploading to " + tempPath);

    await sync.write({
      filename: tempPath,
      file: readableStreamFromByteArray(new Uint8Array(await file.arrayBuffer()))
    })

    const response = await sendRequest(device, {
      'type': 'Import',
      from_path: tempPath
    });

    return response as ImportResult;
  } finally {
    sync.dispose();
  }
}

// Downloads the file at the given URL on the quest, then attempts to import it as a mod.
// If the mod comes from a repository (e.g. Thunderstore), the details of the package should be
// given so that update checks can be performed later.
export async function importUrl(device: Adb,
  url: string,
  repoPackage?: { id: string, name: string, version: string }): Promise<ImportResult> {
  const response = await sendRequest(device, {
    type: 'ImportUrl',
    from_url: url,
    id: repoPackage?.id ?? null,
    name: repoPackage?.name ?? null,
    version: repoPackage?.version ?? null
  });

  return response as ImportResult;
}

export async function removeMod(device: Adb,
  mod_id: string) {
  let response = await sendRequest(device, {
      type: 'RemoveMod',
      id: mod_id
  });

  return (response as Mods).installed_mods;
}

// Instructs the agent to patch the app, adding MelonLoader (LemonLoader) to the APK.
// Returns the mod status of the installation after patching.
export async function patchApp(device: Adb, repatch: boolean): Promise<ModStatus> {
  let response = await sendRequest(device, {
      type: 'Patch',
      repatch
  }) as ModStatus;

  return response;
}
