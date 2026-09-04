import {getClient} from "./browser.ts";

/**
 * Address the app's embedded remote control server listens on in the remote
 * specs. The served frontend is reachable at http://REMOTE_ADDR/.
 */
export const REMOTE_ADDR = "127.0.0.1:9610";

/** Enables or disables the remote control server on the given app instance. */
export async function setRemoteEnabled(
    browser: WebdriverIO.Browser,
    enabled: boolean,
): Promise<void> {
    const result = await browser.execute(
        async (addr: string, on: boolean) => {
            try {
                await window.__TAURI_INTERNALS__.invoke("remote_set_config", {
                    remoteConfig: {enabled: on, listenAddr: addr, serveFrontend: true},
                });
                return {ok: true as const};
            } catch (e) {
                return {ok: false as const, error: String(e)};
            }
        },
        REMOTE_ADDR,
        enabled,
    );
    if (!result.ok) {
        throw new Error(`remote_set_config failed: ${result.error}`);
    }
}

/**
 * Points the remote browser at the app's served frontend and waits until it
 * has hydrated into the connected session: END is only rendered past the
 * login and connect pages.
 */
export async function openRemoteBrowser(): Promise<WebdriverIO.Browser> {
    const remoteBrowser = getClient("remoteBrowser");
    await remoteBrowser.url(`http://${REMOTE_ADDR}/`);
    await remoteBrowser.$("button=END").waitForDisplayed();
    return remoteBrowser;
}

/**
 * Parks the remote browser on a blank page. Required before the app instance
 * is restarted: a page left on the remote frontend keeps reconnecting and
 * would hydrate itself into the next test's app behind its back.
 */
export async function closeRemoteBrowser(): Promise<void> {
    await getClient("remoteBrowser").url("about:blank");
}
