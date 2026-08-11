import os from "node:os";
import path from "node:path";
import {config as baseConfig} from "./wdio.conf.ts";

const {hostname: _hostname, ...baseWithoutHostname} = baseConfig;
const baseCapabilities = baseConfig.capabilities as Record<
    string,
    {port?: number; capabilities: unknown}
>;

/**
 * Runs the remote control specs with one real app instance and an actual
 * browser. The app's own webview cannot act as the remote browser: Tauri
 * injects its IPC globals into every page it loads, so the served frontend
 * would detect the native environment instead of using the remote transport.
 *
 * The global hostname is dropped so WebdriverIO's driver management can
 * handle the chromium instance; the tauri-driver instance pins it instead.
 */
export const config: WebdriverIO.MultiremoteConfig = {
    ...baseWithoutHostname,
    specs: ["./specs-remote/**/*.ts"],
    // Chrome for Testing downloads default to os.tmpdir(); pin them to a
    // deterministic location so CI can cache the browser between runs.
    cacheDir: path.join(os.homedir(), ".cache", "wdio-browsers"),
    capabilities: {
        clientA: {
            ...baseCapabilities.clientA,
            hostname: "127.0.0.1",
        },
        remoteBrowser: {
            capabilities: {
                browserName: "chrome",
                // Pin to a downloaded Chrome for Testing so the run does not
                // depend on locally installed browsers (CI runners and dev
                // machines differ).
                browserVersion: "stable",
                "goog:chromeOptions": {
                    args: ["--headless=new", "--no-sandbox", "--disable-gpu"],
                },
            },
        },
    } as WebdriverIO.MultiremoteConfig["capabilities"],
};
