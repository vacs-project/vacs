/**
 * Automation handles for E2E builds.
 *
 * Store state and store actions live in module scope, which the WebDriver
 * page context cannot reach. The docs screenshot specs need two things from
 * it: the id of the active call (to emit call events that only apply to a
 * matching call), and a way to re-run the capability fetch after installing
 * an IPC mock, since it otherwise runs once on mount.
 *
 * Installed from `main.tsx` behind `import.meta.env.MODE === "e2e"`, which
 * drops this module from production bundles.
 */
import {fetchCapabilities} from "./stores/capabilities-store.ts";
import {useCallStore} from "./stores/call-store.ts";
import {useUpdateStore} from "./stores/update-store.ts";
import {CallId} from "./types/generic.ts";

export type E2eHooks = {
    /** Re-reads `app_platform_capabilities`, honoring any installed mock. */
    refetchCapabilities: () => Promise<void>;
    /** The active call's id, or null when no call is on the display. */
    activeCallId: () => CallId | null;
    /**
     * Overrides the version shown in the header. The update check that
     * normally fills it runs on mount, before a spec can mock its command.
     */
    setVersion: (version: string) => void;
};

export function installE2eHooks(): void {
    // eslint-disable-next-line no-underscore-dangle
    (window as Window & {__vacs_e2e__?: E2eHooks}).__vacs_e2e__ = {
        refetchCapabilities: fetchCapabilities,
        activeCallId: () => useCallStore.getState().callDisplay?.call.callId ?? null,
        setVersion: version => useUpdateStore.getState().actions.setVersions(version),
    };
}
