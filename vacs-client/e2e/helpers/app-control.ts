import {type ChildProcess, spawn, spawnSync} from "child_process";
import {appendFileSync, existsSync, readFileSync, rmSync} from "fs";
import os from "os";
import path from "path";
import {fileURLToPath} from "url";

/**
 * App-instance lifecycle for the embedded WebDriver.
 *
 * The embedded driver serves WebDriver from inside the app process, so
 * neither the wdio session lifecycle nor @wdio/tauri-service restarts the
 * app between tests: reloadSession() alone would hand out a fresh session
 * id against the same, still-logged-in process. The suite's isolation
 * model is one fresh app process per test, so restartApps() replaces every
 * instance with a new process before re-creating the sessions.
 *
 * Ownership model: instances are always retired with SIGKILL, never
 * gracefully - a graceful close (window close -> CloseRequested) persists
 * the cookie store and would leak an authenticated session into the next
 * process. Instances spawned by this worker are killed via their child
 * handle; instances handed over by a previous worker are killed via the
 * port:pid ledger every spawn appends to; instances the service spawned at
 * run start are asked for their pid (the e2e-only app_process_id command)
 * and killed with it. afterSession deliberately leaves the last generation
 * running so the next worker's session creation finds a live server;
 * stragglers are reaped from the ledger by onPrepare/onComplete in the
 * launcher. That handover is best-effort: on Windows a worker's exit takes
 * its child processes with it, so beforeSession runs ensureApps() to respawn
 * whatever is missing.
 */

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const VACS_ROOT = path.resolve(__dirname, "..", "..", "..");
const BINARY_EXT = process.platform === "win32" ? ".exe" : "";
const APP_BINARY = path.resolve(VACS_ROOT, "target", "debug", `vacs-client${BINARY_EXT}`);
const PID_FILE = path.resolve(__dirname, "..", ".app-pids");
const E2E_IDENTIFIER = "app.vacs.vacs-client-e2e";

export interface AppInstance {
    name: string;
    port: number;
}

let instances: AppInstance[] = [];
const owned = new Map<string, ChildProcess>();

// The E2E bundle identifier's local data dir; both instances share it. The
// cookie store in it must never survive into a fresh instance: a leaked
// authenticated session would skip the login page and break every spec
// that starts logged out.
const APP_DATA_DIR = (() => {
    switch (process.platform) {
        case "darwin":
            return path.join(os.homedir(), "Library", "Application Support", E2E_IDENTIFIER);
        case "win32":
            return path.join(
                process.env.LOCALAPPDATA ?? path.join(os.homedir(), "AppData", "Local"),
                E2E_IDENTIFIER,
            );
        default:
            return path.join(
                process.env.XDG_DATA_HOME ?? path.join(os.homedir(), ".local", "share"),
                E2E_IDENTIFIER,
            );
    }
})();

export function configureInstances(list: AppInstance[]): void {
    instances = list;
}

/**
 * Force-kills a process and, on Windows, its entire tree. WebView2 runs as
 * child processes that survive TerminateProcess of their host and keep the
 * shared user data folder locked, which fails the next generation's webview
 * creation with E_INVALIDARG. Returns whether anything was killed.
 */
function killTree(pid: number): boolean {
    if (process.platform === "win32") {
        const result = spawnSync("taskkill", ["/pid", String(pid), "/T", "/F"], {
            stdio: "ignore",
        });
        return result.status === 0;
    }
    try {
        process.kill(pid, "SIGKILL");
        return true;
    } catch {
        // already gone
        return false;
    }
}

/**
 * Replaces every configured app instance with a fresh process and
 * re-creates the WebDriver sessions. Call from a spec's beforeEach in
 * place of multiRemoteBrowser.reloadSession().
 */
export async function restartApps(): Promise<void> {
    await Promise.all(instances.map(instance => restartInstance(instance)));
    await multiRemoteBrowser.reloadSession();
    // The embedded server accepts sessions before the webview finishes
    // navigating to the app page. An execute issued mid-navigation loses its
    // result variable and burns the full 30s script timeout, so wait for the
    // app document (element finds poll safely) before handing control back.
    await Promise.all(
        instances.map(instance =>
            multiRemoteBrowser.getInstance(instance.name).$("#root").waitForExist(),
        ),
    );
}

/**
 * Ensures every configured port has a listening app without touching ones
 * that are already up. For beforeSession in configs that do not use
 * @wdio/tauri-service (no session exists yet, so nothing can be retired).
 */
export async function ensureApps(): Promise<void> {
    for (const instance of instances) {
        if (
            await webdriverReady(instance.port, 1_000).then(
                () => true,
                () => false,
            )
        ) {
            continue;
        }
        spawnInstance(instance);
        await webdriverReady(instance.port, 60_000);
    }
}

/**
 * Removes persisted app state that must not survive into a test run. The
 * cookie store would otherwise re-authenticate the first generation of app
 * instances against a still-valid server-side session. Launcher-side, from
 * onPrepare, before the service spawns anything.
 */
export function clearPersistedAppState(): void {
    // SecureCookieStore silently restores from the .bak copy whenever the
    // primary file is missing, so deleting only .cookies resurrects the old
    // authenticated store on the next boot.
    rmSync(path.join(APP_DATA_DIR, ".cookies"), {force: true});
    rmSync(path.join(APP_DATA_DIR, ".cookies.bak"), {force: true});
}

/** Kills every pid recorded in the pid file. Launcher-side (onPrepare/onComplete). */
export function reapRecordedApps(): void {
    if (!existsSync(PID_FILE)) return;
    for (const pid of recordedPids()) {
        killTree(pid);
    }
    rmSync(PID_FILE, {force: true});
}

async function restartInstance(instance: AppInstance): Promise<void> {
    const proc = owned.get(instance.name);
    if (proc !== undefined) {
        const exited = new Promise<void>(resolve => proc.once("exit", () => resolve()));
        if (proc.pid !== undefined) {
            killTree(proc.pid);
        } else {
            proc.kill("SIGKILL");
        }
        await exited;
        owned.delete(instance.name);
    } else {
        await retireAdopted(instance);
    }
    await portClosed(instance.port, 15_000);
    // A graceful close anywhere may have persisted session cookies; a fresh
    // instance must always boot logged out.
    clearPersistedAppState();
    spawnInstance(instance);
    await webdriverReady(instance.port, 60_000);
}

async function retireAdopted(instance: AppInstance): Promise<void> {
    // A previous worker's instance is in the ledger and possibly
    // authenticated: SIGKILL so nothing gets persisted. Stale entries for
    // long-dead generations are killed along the way (ESRCH is ignored).
    let killedFromLedger = false;
    for (const pid of recordedPids(instance.port)) {
        killedFromLedger = killTree(pid) || killedFromLedger;
    }
    if (killedFromLedger) return;

    // Not in the ledger: spawned by the service at run start. Ask the app
    // for its pid and SIGKILL it; a graceful close would persist whatever
    // session state it happens to hold.
    try {
        const client = multiRemoteBrowser.getInstance(instance.name);
        const pid = await client.execute(() =>
            (
                window as never as {
                    __TAURI_INTERNALS__: {invoke: (cmd: string) => Promise<number>};
                }
            ).__TAURI_INTERNALS__.invoke("app_process_id"),
        );
        killTree(pid);
    } catch {
        // no live session to ask; if the port stays open, portClosed() in
        // the caller surfaces it
    }
}

function recordedPids(port?: number): number[] {
    if (!existsSync(PID_FILE)) return [];
    const pids: number[] = [];
    for (const line of readFileSync(PID_FILE, "utf-8").split("\n")) {
        const [portPart, pidPart] = line.trim().split(":");
        const pid = Number(pidPart);
        if (!Number.isInteger(pid) || pid <= 0) continue;
        if (port !== undefined && Number(portPart) !== port) continue;
        pids.push(pid);
    }
    return pids;
}

function spawnInstance(instance: AppInstance): void {
    const proc = spawn(APP_BINARY, [], {
        env: {...process.env, TAURI_WEBDRIVER_PORT: String(instance.port)},
        stdio: ["ignore", process.stdout, process.stderr],
    });
    owned.set(instance.name, proc);
    if (proc.pid !== undefined) {
        appendFileSync(PID_FILE, `${instance.port}:${proc.pid}\n`);
    }
}

async function webdriverReady(port: number, timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        try {
            const res = await fetch(`http://127.0.0.1:${port}/status`, {
                signal: AbortSignal.timeout(1_000),
            });
            if (res.ok) return;
        } catch {
            // not up yet
        }
        await new Promise(r => setTimeout(r, 100));
    }
    throw new Error(`Embedded webdriver on port ${port} not ready within ${timeoutMs}ms`);
}

async function portClosed(port: number, timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        try {
            await fetch(`http://127.0.0.1:${port}/status`, {signal: AbortSignal.timeout(500)});
        } catch {
            return;
        }
        await new Promise(r => setTimeout(r, 100));
    }
    throw new Error(`Port ${port} still open ${timeoutMs}ms after app termination`);
}
