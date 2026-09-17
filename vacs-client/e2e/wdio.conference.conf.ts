import {config as baseConfig} from "./wdio.conf.ts";
import {clearAppLogs, configureInstances} from "./helpers/app-control.ts";

// Embedded WebDriver ports, continuing the main config's scheme (the service
// assigns base + i per multiremote instance, so the third instance lands on
// 4452). Sharing the base with wdio.conf.ts is safe: the configs never run at
// the same time, and onPrepare reaps every recorded app process first.
const EMBEDDED_PORT_BASE = 4450;

// The server's default limit is 8, which three app instances plus a raw
// signaling client cannot reach. Set here rather than in wdio.conf.ts so only
// the conference suite runs against it, and at module scope because the
// worker loads this config before beforeSession spawns vacs-server with the
// environment it inherits.
process.env["VACS-CALL-MAX_CONF_SIZE"] = "3";

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
 * Runs the conference specs with three real app instances. Everything else
 * (build, mock VATSIM, vacs-server, app process isolation) comes from the
 * main config; only the instance count and the spec directory differ, so the
 * two-instance suite keeps its own tuned lifecycle.
 */
export const config: WebdriverIO.MultiremoteConfig = {
    ...baseConfig,
    specs: ["./specs-conference/**/*.ts"],
    capabilities: {
        clientA: appCapability(),
        clientB: appCapability(),
        clientC: appCapability(),
    } as WebdriverIO.MultiremoteConfig["capabilities"],

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
