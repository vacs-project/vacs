import {restartApps} from "../helpers/app-control.ts";
import {loginAndConnect, resetMockState} from "../helpers/auth.ts";
import {click, getClient} from "../helpers/browser.ts";
import {startVacsServer, stopVacsServer} from "../helpers/server-control.ts";

const CID_A = "10000004";

describe("Reconnect", () => {
    beforeEach(async () => {
        await startVacsServer();
        await resetMockState();
        await restartApps();
    });

    afterEach(async () => {
        await startVacsServer();
    });

    it("should drop to the connect page during an outage and reconnect", async () => {
        const clientA = getClient("clientA");
        await loginAndConnect(clientA, CID_A);

        // An abrupt server outage drops the session: the phone tab falls
        // back to the connect page (either reconnecting or disconnected).
        await stopVacsServer();
        const connectButton = await clientA.$("button*=Connect");
        await connectButton.waitForDisplayed({timeout: 15000});

        // Once the server is back, the session is re-established: either
        // automatically by the bounded reconnect loop, or manually when
        // the retries were already exhausted.
        await startVacsServer();
        try {
            await connectButton.waitForDisplayed({reverse: true, timeout: 8000});
        } catch {
            const manualConnect = await clientA.$("button=Connect");
            await click(clientA, manualConnect);
            await connectButton.waitForDisplayed({reverse: true});
        }
    });

    it("should surface a network error when the server is down at startup", async () => {
        const clientA = getClient("clientA");

        await stopVacsServer();
        await restartApps();

        // The overlay unmounts on dismissal and remounts on the next error,
        // so a held element handle would go stale; query it fresh each time.
        const errorTitle = () => clientA.$("p=Network error");

        // The app lands on the login page with a network error overlay.
        await errorTitle().waitForDisplayed();
        await click(clientA, await errorTitle());
        await errorTitle().waitForDisplayed({reverse: true});

        // Attempting to log in fails with the same overlay and leaves the
        // login button usable.
        const loginButton = await clientA.$("button=Login via VATSIM");
        await loginButton.waitForDisplayed();
        await click(clientA, loginButton);
        await errorTitle().waitForDisplayed();
        await click(clientA, await errorTitle());

        // Once the server is up again, logging in and connecting works.
        await startVacsServer();
        await loginButton.waitForDisplayed();
        await loginAndConnect(clientA, CID_A);
    });
});
