import os from "node:os";
import path from "node:path";
import {config as baseConfig} from "./wdio.conf.ts";
import {configureInstances, ensureApps, reapRecordedApps} from "./helpers/app-control.ts";

const {hostname: _hostname, services: _services, capabilities: _capabilities, ...base} = baseConfig;

// Embedded WebDriver port for this config's single app instance; distinct
// from the main config's 4450/4451 so a leftover instance from a preceding
// run of the other config cannot be mistaken for ours.
const REMOTE_APP_PORT = 4460;

configureInstances([{name: "clientA", port: REMOTE_APP_PORT}]);

/**
 * Runs the remote control specs with one real app instance and an actual
 * browser. The app's own webview cannot act as the remote browser: Tauri
 * injects its IPC globals into every page it loads, so the served frontend
 * would detect the native environment instead of using the remote transport.
 *
 * @wdio/tauri-service is deliberately NOT registered here: it rejects any
 * non-tauri capability in the session, and this config needs a real Chrome
 * instance. The app is spawned by app-control instead and its in-process
 * WebDriver server is addressed as a plain remote driver (hostname + port),
 * while WebdriverIO's own driver management handles the chromium instance.
 */
export const config: WebdriverIO.MultiremoteConfig = {
    ...base,
    specs: ["./specs-remote/**/*.ts"],
    // Chrome for Testing downloads default to os.tmpdir(); pin them to a
    // deterministic location so CI can cache the browser between runs.
    cacheDir: path.join(os.homedir(), ".cache", "wdio-browsers"),
    capabilities: {
        clientA: {
            hostname: "127.0.0.1",
            port: REMOTE_APP_PORT,
            capabilities: {
                // No browserName: a defined port marks this instance as a
                // remote driver, so WebdriverIO skips driver management. The
                // embedded server is classic W3C only.
                "wdio:enforceWebDriverClassic": true,
            },
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

    async beforeSession(...args) {
        // Mock VATSIM + vacs-server, from the base config.
        const baseHooks = baseConfig.beforeSession;
        for (const hook of Array.isArray(baseHooks) ? baseHooks : baseHooks ? [baseHooks] : []) {
            await hook.apply(this, args);
        }
        // The app instance the session connects to. A live instance handed
        // over by a previous worker is adopted as-is; the first
        // restartApps() replaces it either way.
        await ensureApps();
    },

    onComplete() {
        reapRecordedApps();
    },
};
