import {restartApps} from "../helpers/app-control.ts";
import {authenticate, loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {getClient, tauriApi} from "../helpers/browser.ts";

const CID_A = "10000004";

describe("Session Expiry", () => {
    beforeEach(async () => {
        await resetMockState();
        await restartApps();

        await loginAndConnect(getClient("clientA"), CID_A);
    });

    it("should drop to the login page when the session expires", async () => {
        const clientA = getClient("clientA");

        // The backend announces an expired or revoked session with the
        // auth:unauthenticated event. browser.tauri.emitEvent cannot be used
        // here: the embedded provider's eval wrapper exposes only the core
        // API, so it never finds the event API. The global from
        // withGlobalTauri routes through the same event system.
        await tauriApi("clientA").execute(() => {
            const emit = window.__TAURI__?.event?.emit;
            if (emit === undefined) {
                throw new Error("Global Tauri event API unavailable");
            }
            return emit("auth:unauthenticated", null);
        });

        const loginButton = () => clientA.$("button=Login via VATSIM");
        await loginButton().waitForDisplayed();

        // Re-authenticating returns to the session view (the signaling
        // connection itself was never dropped).
        await authenticate(clientA, CID_A);
        await loginButton().waitForDisplayed({reverse: true});
    });
});
