const MOCK_VATSIM_BASE = "http://127.0.0.1:4567";

declare global {
    interface Window {
        __TAURI_INTERNALS__: {
            invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
        };
    }
}

export type Controller = {
    cid: number;
    name: string;
    callsign: string;
    frequency: string;
    facility: number;
    rating: number;
    server: string;
    visual_range: number;
    text_atis?: string[];
    last_updated: string;
    logon_time: string;
};

/**
 * Authenticates a browser client via the mock OAuth flow by invoking the
 * `auth_login_test` Tauri command (only available with the `e2e` feature).
 * Throws if authentication fails for the given CID.
 */
export async function authenticate(browser: WebdriverIO.Browser, cid: string): Promise<void> {
    const result = await browser.execute(async (targetCid: string) => {
        try {
            await window.__TAURI_INTERNALS__.invoke("auth_login_test", {cid: targetCid});
            return {ok: true as const};
        } catch (e) {
            return {ok: false as const, error: String(e)};
        }
    }, cid);

    if (!result.ok) {
        throw new Error(`auth_login_test failed for CID ${cid}: ${result.error}`);
    }
}

/**
 * Resets the mock VATSIM server to its initial seed state, clearing any
 * controllers or users added during a test.
 */
export async function resetMockState(): Promise<void> {
    const resp = await fetch(`${MOCK_VATSIM_BASE}/api/reset`, {
        method: "POST",
    });
    if (!resp.ok) {
        throw new Error(`Failed to reset mock state: ${resp.status}`);
    }
}

/**
 * Adds a controller to the mock VATSIM data feed so it appears as online.
 */
export async function seedController(controller: Controller): Promise<void> {
    const resp = await fetch(`${MOCK_VATSIM_BASE}/api/controllers`, {
        method: "POST",
        headers: {"Content-Type": "application/json"},
        body: JSON.stringify(controller),
    });
    if (!resp.ok) {
        throw new Error(`Failed to seed controller: ${resp.status} ${await resp.text()}`);
    }
}

/**
 * Removes a controller from the mock VATSIM data feed by CID.
 * No-ops if the controller does not exist.
 */
export async function removeController(cid: string): Promise<void> {
    const resp = await fetch(`${MOCK_VATSIM_BASE}/api/controllers/${cid}`, {
        method: "DELETE",
    });
    if (!resp.ok && resp.status !== 404) {
        throw new Error(`Failed to remove controller: ${resp.status}`);
    }
}
