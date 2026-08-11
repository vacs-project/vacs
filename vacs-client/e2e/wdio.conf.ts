import path from "path";
import {type ChildProcess, spawn, spawnSync, execFileSync} from "child_process";
import {createConnection} from "net";
import {fileURLToPath} from "url";
import {
    clearPersistedAppState,
    configureInstances,
    ensureApps,
    reapRecordedApps,
} from "./helpers/app-control.ts";

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const VACS_ROOT = path.resolve(__dirname, "..", "..");
const VACS_CLIENT_ROOT = path.resolve(VACS_ROOT, "vacs-client");
const VACS_DATA_DIR = process.env.VACS_DATA_DIR || path.resolve(VACS_ROOT, "..", "vacs-data");

const IS_WINDOWS = process.platform === "win32";
const BINARY_EXT = IS_WINDOWS ? ".exe" : "";
const APP_BINARY = path.resolve(VACS_ROOT, "target", "debug", `vacs-client${BINARY_EXT}`);

const MOCK_VATSIM_PORT = 4567;
const VACS_SERVER_PORT = 4568;
// Embedded WebDriver ports (the app serves WebDriver in-process via
// tauri-plugin-wdio-webdriver). Deliberately not the plugin's 4445 default
// so a stale process from the old tauri-driver harness cannot masquerade
// as an instance. The service assigns base + i per multiremote instance.
const EMBEDDED_PORT_BASE = 4450;

configureInstances([
    {name: "clientA", port: EMBEDDED_PORT_BASE},
    {name: "clientB", port: EMBEDDED_PORT_BASE + 1},
]);

// keep track of child processes for cleanup
let mockVatsimServer: ChildProcess | undefined;
let vacsServer: ChildProcess | undefined;
let exit = false;
let serverStopRequested = false;

export const config: WebdriverIO.MultiremoteConfig = {
    hostname: "127.0.0.1",
    specs: ["./specs/**/*.ts"],
    maxInstances: 1,
    services: [
        [
            "@wdio/tauri-service",
            {
                driverProvider: "embedded",
                embeddedPort: EMBEDDED_PORT_BASE,
                appBinaryPath: APP_BINARY,
                startTimeout: 120_000,
                statusPollTimeout: 10_000,
                captureBackendLogs: Boolean(process.env.CI),
                captureFrontendLogs: Boolean(process.env.CI),
                backendLogLevel: "info",
                frontendLogLevel: "warn",
            },
        ],
    ],
    capabilities: {
        // enforceWebDriverClassic: the embedded server implements classic
        // W3C WebDriver only; requesting BiDi (the WebdriverIO 9 default)
        // would leave session negotiation to how the server treats an
        // unknown capability.
        clientA: {
            capabilities: {
                browserName: "tauri",
                "wdio:enforceWebDriverClassic": true,
                "tauri:options": {
                    application: APP_BINARY,
                },
            },
        },
        clientB: {
            capabilities: {
                browserName: "tauri",
                "wdio:enforceWebDriverClassic": true,
                "tauri:options": {
                    application: APP_BINARY,
                },
            },
        },
    },
    reporters: ["spec"],
    framework: "mocha",
    mochaOpts: {
        ui: "bdd",
        timeout: 60_000,
    },
    // Call establishment (invite -> accept -> ICE -> connected) can exceed 10s
    // on loaded CI runners; waits return as soon as their condition holds, so
    // a high ceiling does not slow passing tests.
    waitforTimeout: 20_000,
    // Retry a failed spec file once before failing the run: two app instances
    // plus live WebRTC negotiation leave a residual flake rate that would
    // otherwise fail CI randomly.
    specFileRetries: 1,
    // Healthy sessions are created within seconds; when an app instance is
    // broken (e.g. its webview failed to initialize), the defaults burn 20+
    // minutes of dead session attempts per run before the leg fails.
    connectionRetryTimeout: 60_000,
    connectionRetryCount: 1,
    logLevel: "warn",

    onPrepare() {
        // App processes from a previous crashed run would hold the embedded
        // ports and shadow this run's instances; leaked session state would
        // boot them already authenticated.
        reapRecordedApps();
        clearPersistedAppState();

        // Build vatsim-mock from source if VATSIM_API_ROOT is set,
        // otherwise expect it on PATH (e.g. via cargo install).
        if (process.env.VATSIM_API_ROOT) {
            console.log("Building vatsim-mock binary...");
            const mock = spawnSync(
                "cargo",
                ["build", "--bin", "vatsim-mock", "--features", "mock-bin"],
                {
                    cwd: process.env.VATSIM_API_ROOT,
                    stdio: "inherit",
                    shell: true,
                },
            );
            if (mock.status !== 0) throw new Error("vatsim-mock build failed");
        }

        console.log("Building vacs-client with e2e feature...");
        const client = spawnSync(
            "npm",
            [
                "run",
                "tauri",
                "build",
                "--",
                // Separate bundle identifier: keeps E2E instances from
                // writing settings into the real client's config directory.
                "--config",
                "tauri.e2e.conf.json",
                "--features",
                "e2e",
                "--debug",
                "--no-bundle",
            ],
            {
                cwd: VACS_CLIENT_ROOT,
                stdio: "inherit",
                shell: true,
            },
        );
        if (client.status !== 0) throw new Error("vacs-client build failed");

        console.log("Building vacs-server...");
        const server = spawnSync("cargo", ["build", "-p", "vacs-server"], {
            cwd: VACS_ROOT,
            stdio: "inherit",
            shell: true,
        });
        if (server.status !== 0) throw new Error("vacs-server build failed");
    },

    async beforeSession() {
        // Prefer locally-built binary, fall back to PATH (cargo install)
        const mockBin = process.env.VATSIM_API_ROOT
            ? path.resolve(
                  process.env.VATSIM_API_ROOT,
                  "target",
                  "debug",
                  `vatsim-mock${BINARY_EXT}`,
              )
            : findBinary("vatsim-mock");
        const seedPath = path.resolve(__dirname, "seed.json");

        mockVatsimServer = spawn(
            mockBin,
            ["--bind", `127.0.0.1:${MOCK_VATSIM_PORT}`, "--seed", seedPath],
            {
                stdio: ["ignore", process.stdout, process.stderr],
            },
        );
        mockVatsimServer.on("error", error => {
            console.error("vatsim-mock error:", error);
            process.exit(1);
        });
        mockVatsimServer.on("exit", code => {
            if (!exit) {
                console.error("vatsim-mock exited with code:", code);
            }
        });

        await waitForPort(MOCK_VATSIM_PORT, 10_000);
        console.log(`vatsim-mock listening on port ${MOCK_VATSIM_PORT}`);

        vacsServer = spawnVacsServer();
        await waitForPort(VACS_SERVER_PORT, 15_000);
        console.log(`vacs-server listening on port ${VACS_SERVER_PORT}`);

        // Expose server lifecycle control for outage/reconnect specs
        // (see helpers/server-control.ts).
        globalThis.__vacsServerControl = {
            stop: async () => {
                const proc = vacsServer;
                if (proc === undefined) return;
                serverStopRequested = true;
                const exited = new Promise<void>(resolve => proc.once("exit", () => resolve()));
                // SIGKILL: simulate an abrupt outage rather than a graceful
                // shutdown (which would notify clients cleanly).
                proc.kill("SIGKILL");
                await exited;
                vacsServer = undefined;
            },
            start: async () => {
                if (vacsServer !== undefined) return;
                serverStopRequested = false;
                vacsServer = spawnVacsServer();
                await waitForPort(VACS_SERVER_PORT, 15_000);
            },
        };

        // A previous worker's app instances are normally handed over alive,
        // but on Windows a worker's exit takes its child processes with it;
        // respawn whatever is missing before the session request goes out.
        await ensureApps();
    },

    afterSession() {
        // Servers only: the last generation of app processes stays alive
        // where possible so the next worker's session creation finds live
        // embedded WebDriver servers; that worker retires them at its first
        // restartApps(). beforeSession's ensureApps() covers platforms where
        // the processes die with the worker.
        cleanup();
    },

    onComplete() {
        reapRecordedApps();
    },
};

function spawnVacsServer(): ChildProcess {
    const serverBin = path.resolve(VACS_ROOT, "target", "debug", `vacs-server${BINARY_EXT}`);
    const proc = spawn(serverBin, [], {
        cwd: VACS_ROOT,
        stdio: ["ignore", process.stdout, process.stderr],
        env: {
            ...process.env,
            // The server's built-in default is all-trace. Cap the two
            // high-volume, low-signal targets (per-file dataset loading and
            // redis store round trips) at debug; an explicit RUST_LOG still
            // wins for full-trace debugging.
            RUST_LOG:
                process.env.RUST_LOG ??
                "vacs_server=trace,vacs_=trace,vacs_vatsim::coverage=debug," +
                    "vacs_server::store=debug,tower_http=debug,tower_sessions=debug," +
                    "axum::rejection=trace",
            "VACS-AUTH-OAUTH-AUTH_URL": `http://127.0.0.1:${MOCK_VATSIM_PORT}/oauth/authorize`,
            "VACS-AUTH-OAUTH-TOKEN_URL": `http://127.0.0.1:${MOCK_VATSIM_PORT}/oauth/token`,
            "VACS-AUTH-OAUTH-CLIENT_ID": "e2e-test-client",
            "VACS-AUTH-OAUTH-CLIENT_SECRET": "e2e-test-secret",
            "VACS-VATSIM-USER_SERVICE-USER_DETAILS_ENDPOINT_URL": `http://127.0.0.1:${MOCK_VATSIM_PORT}/api/user`,
            "VACS-VATSIM-SLURPER_BASE_URL": `http://127.0.0.1:${MOCK_VATSIM_PORT}`,
            "VACS-VATSIM-DATA_FEED_URL": `http://127.0.0.1:${MOCK_VATSIM_PORT}/v3/vatsim-data.json`,
            "VACS-VATSIM-REQUIRE_ACTIVE_CONNECTION": "false",
            "VACS-SESSION-SIGNING_KEY":
                "e2e-test-signing-key-at-least-64-chars-long-for-hmac-sha256-aaaa-bbbb-cccc-dddd-eeee-ffff-0000",
            "VACS-SESSION-SECURE": "false",
            // Tests trigger logins and calls far more frequently than the
            // production limits allow; rate limiting is not under test.
            "VACS-RATE_LIMITERS-ENABLED": "false",
            // Poll the mock datafeed every second and skip response
            // caching so station changes propagate quickly to clients.
            "VACS-VATSIM-CONTROLLER_UPDATE_INTERVAL-SECS": "1",
            "VACS-VATSIM-CONTROLLER_UPDATE_INTERVAL-NANOS": "0",
            "VACS-VATSIM-DATA_FEED_CACHE_TTL-SECS": "0",
            "VACS-VATSIM-DATA_FEED_CACHE_TTL-NANOS": "0",
            // Shorten the position grace period so datafeed-driven
            // position changes become testable without long waits.
            "VACS-VATSIM-DATA_FEED_POSITION_GRACE_PERIOD-SECS": "2",
            "VACS-VATSIM-DATA_FEED_POSITION_GRACE_PERIOD-NANOS": "0",
            "VACS-VATSIM-COVERAGE_DIR": path.resolve(VACS_DATA_DIR, "dataset"),
            "VACS-SERVER-BIND_ADDR": `127.0.0.1:${VACS_SERVER_PORT}`,
        },
    });
    proc.on("error", error => {
        console.error("vacs-server error:", error);
        process.exit(1);
    });
    proc.on("exit", code => {
        if (!exit && !serverStopRequested) {
            console.error("vacs-server exited with code:", code);
        }
    });
    return proc;
}

function cleanup() {
    exit = true;
    vacsServer?.kill();
    mockVatsimServer?.kill();
}

function onShutdown(fn: () => void) {
    const handler = () => {
        try {
            fn();
        } finally {
            process.exit();
        }
    };
    process.on("exit", handler);
    process.on("SIGINT", handler);
    process.on("SIGTERM", handler);
    process.on("SIGHUP", handler);
    process.on("SIGBREAK", handler);
}

onShutdown(cleanup);

async function waitForPort(port: number, timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs;

    while (Date.now() < deadline) {
        const connected = await new Promise<boolean>(resolve => {
            const socket = createConnection({host: "127.0.0.1", port}, () => {
                socket.destroy();
                resolve(true);
            });
            socket.on("error", () => {
                socket.destroy();
                resolve(false);
            });
        });
        if (connected) return;
        await new Promise(r => setTimeout(r, 200));
    }
    throw new Error(`Port ${port} did not become available within ${timeoutMs}ms`);
}

function findBinary(name: string): string {
    const cmd = IS_WINDOWS ? "where" : "which";
    try {
        return execFileSync(cmd, [name], {encoding: "utf-8"}).trim().split("\n")[0];
    } catch {
        throw new Error(`Binary "${name}" not found on PATH.`);
    }
}

declare global {
    namespace WebdriverIO {
        interface Capabilities {
            "tauri:options"?: {
                application: string;
            };
        }
    }
}
