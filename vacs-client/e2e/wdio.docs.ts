import path from "node:path";
import {fileURLToPath} from "node:url";
import {config as baseConfig} from "./wdio.conf.ts";
import {clearAppLogs, configureInstances} from "./helpers/app-control.ts";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// App instances inherit this environment, and the settings page images show
// the device selects, so the mock backend gets presentable device names here
// rather than the defaults the behavioral suite asserts on.
process.env.VACS_MOCK_AUDIO_CONFIG = path.resolve(__dirname, "fixtures", "mock-audio-docs.toml");

// Embedded WebDriver ports, continuing the main config's scheme (the service
// assigns base + i per multiremote instance, so the third instance lands on
// 4452). Sharing the base with wdio.conf.ts is safe: the configs never run at
// the same time, and onPrepare reaps every recorded app process first.
const EMBEDDED_PORT_BASE = 4450;

configureInstances([
    {name: "clientA", port: EMBEDDED_PORT_BASE},
    {name: "clientB", port: EMBEDDED_PORT_BASE + 1},
    {name: "clientC", port: EMBEDDED_PORT_BASE + 2},
]);

type InstanceCapability = {capabilities: WebdriverIO.Capabilities};

const baseCapabilities = baseConfig.capabilities as Record<string, InstanceCapability>;

/**
 * A fresh copy of the main config's app capability. The tauri service writes
 * the port it assigned back into each multiremote capability, so the three
 * instances must not share one object.
 */
function appCapability(): InstanceCapability {
    return structuredClone(baseCapabilities.clientA);
}

/**
 * Documentation screenshot run.
 *
 * Same servers and mock VATSIM backend as the regular suite (importing
 * wdio.conf.ts also registers its process cleanup); only the spec directory
 * and the instance count differ. Three instances, because the conference
 * images need a three-way call between real clients. Kept out of `npm test`
 * because these specs produce artifacts rather than assert behavior.
 *
 * Images land in e2e/screenshots/, or in VACS_SCREENSHOT_DIR when set.
 */
export const config: WebdriverIO.MultiremoteConfig = {
    ...baseConfig,
    specs: ["./specs-docs/**/*.ts"],
    capabilities: {
        clientA: appCapability(),
        clientB: appCapability(),
        clientC: appCapability(),
    } as WebdriverIO.MultiremoteConfig["capabilities"],
    // A retry would re-capture and overwrite; a failed capture should be
    // looked at rather than silently repeated.
    specFileRetries: 0,

    onPrepare(...args) {
        // Three instances boot in parallel per test, which makes the shared
        // log directory's rotation race (see clearAppLogs) likely enough to
        // hit within a single run.
        clearAppLogs();

        // Build and cleanup, from the base config.
        const baseHooks = baseConfig.onPrepare;
        for (const hook of Array.isArray(baseHooks) ? baseHooks : baseHooks ? [baseHooks] : []) {
            hook.apply(this, args);
        }
    },
};
