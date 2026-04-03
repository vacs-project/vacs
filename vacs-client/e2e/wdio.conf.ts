import path from "path";
import {type ChildProcess, spawn, spawnSync, execFileSync} from "child_process";
import {createConnection} from "net";
import {fileURLToPath} from "url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const VACS_ROOT = path.resolve(__dirname, "..", "..");
const VACS_CLIENT_ROOT = path.resolve(VACS_ROOT, "vacs-client");

const isWindows = process.platform === "win32";
const binaryExt = isWindows ? ".exe" : "";

const MOCK_VATSIM_PORT = 4567;
const VACS_SERVER_PORT = 4568;

// keep track of child processes for cleanup
let tauriDriver: ChildProcess | undefined;
let mockVatsimServer: ChildProcess | undefined;
let vacsServer: ChildProcess | undefined;
let exit = false;

export const config: WebdriverIO.Config = {
    hostname: "127.0.0.1",
    port: 4444,
    specs: ["./specs/**/*.ts"],
    maxInstances: 1,
    capabilities: [
        // See https://tauri.app/develop/tests/webdriver/example/webdriverio/#config
        {
            maxInstances: 1,
            "tauri:options": {
                application: path.resolve(VACS_ROOT, "target", "debug", `vacs-client${binaryExt}`),
            },
        } as WebdriverIO.Capabilities,
    ],
    reporters: ["spec"],
    framework: "mocha",
    mochaOpts: {
        ui: "bdd",
        timeout: 60_000,
    },
    logLevel: "warn",

    onPrepare() {
        // Build vatsim-mock from source if VATSIM_API_ROOT is set,
        // otherwise expect it on PATH (e.g. via cargo install).
        if (process.env.VATSIM_API_ROOT) {
            console.log("Building vatsim-mock binary...");
            spawnSync("cargo", ["build", "--bin", "vatsim-mock", "--features", "mock-bin"], {
                cwd: process.env.VATSIM_API_ROOT,
                stdio: "inherit",
            });
        }

        console.log("Building vacs-client with e2e feature...");
        spawnSync(
            "npm",
            ["run", "tauri", "build", "--", "--features", "e2e", "--debug", "--no-bundle"],
            {
                cwd: VACS_CLIENT_ROOT,
                stdio: "inherit",
            },
        );

        console.log("Building vacs-server...");
        spawnSync("cargo", ["build", "-p", "vacs-server"], {
            cwd: VACS_ROOT,
            stdio: "inherit",
        });
    },

    async beforeSession() {
        // Prefer locally-built binary, fall back to PATH (cargo install)
        const mockBin = process.env.VATSIM_API_ROOT
            ? path.resolve(
                  process.env.VATSIM_API_ROOT,
                  "target",
                  "debug",
                  `vatsim-mock${binaryExt}`,
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

        const serverBin = path.resolve(VACS_ROOT, "target", "debug", `vacs-server${binaryExt}`);
        vacsServer = spawn(serverBin, [], {
            cwd: VACS_ROOT,
            stdio: ["ignore", process.stdout, process.stderr],
            env: {
                ...process.env,
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
                "VACS-SERVER-BIND_ADDR": `127.0.0.1:${VACS_SERVER_PORT}`,
            },
        });
        vacsServer.on("error", error => {
            console.error("vacs-server error:", error);
            process.exit(1);
        });
        vacsServer.on("exit", code => {
            if (!exit) {
                console.error("vacs-server exited with code:", code);
            }
        });

        await waitForPort(VACS_SERVER_PORT, 15_000);
        console.log(`vacs-server listening on port ${VACS_SERVER_PORT}`);

        tauriDriver = spawn(findBinary("tauri-driver"), [], {
            stdio: [null, process.stdout, process.stderr],
        });
        tauriDriver.on("error", error => {
            console.error("tauri-driver error:", error);
            process.exit(1);
        });
        tauriDriver.on("exit", code => {
            if (!exit) {
                console.error("tauri-driver exited with code:", code);
            }
        });

        await waitForPort(4444, 10_000);
        console.log("tauri-driver listening on port 4444");
    },

    afterSession() {
        cleanup();
    },
};

function cleanup() {
    exit = true;
    tauriDriver?.kill();
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
    const cmd = isWindows ? "where" : "which";
    try {
        return execFileSync(cmd, [name], {encoding: "utf-8"}).trim().split("\n")[0];
    } catch {
        throw new Error(`Binary "${name}" not found on PATH.`);
    }
}
