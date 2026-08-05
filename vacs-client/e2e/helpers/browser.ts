import {instancePort} from "./app-control.ts";

/**
 * Returns a single browser instance from the multiremote session.
 * Defaults to "clientA", which is convenient for tests that only need one client.
 * For multi-client tests, pass the instance name explicitly (e.g., "clientB").
 */
export function getClient(instanceName: string = "clientA"): WebdriverIO.Browser {
    return multiRemoteBrowser.getInstance(instanceName);
}

/**
 * Returns the browser.tauri API bound to an instance. The service's
 * direct-eval channel resolves its target port from TAURI_WEBDRIVER_PORT in
 * the worker process and has no per-instance notion in multiremote, so the
 * env var is pointed at the instance first. Do not interleave concurrent
 * tauri.* calls against different instances.
 */
export function tauriApi(instanceName: string = "clientA"): WebdriverIO.Browser["tauri"] {
    process.env.TAURI_WEBDRIVER_PORT = String(instancePort(instanceName));
    return getClient(instanceName).tauri;
}

/**
 * Installs an IPC command mock into the app's wdio mock registry, which the
 * transport consults in e2e builds. Deliberately not browser.tauri.mock:
 * the service's worker-side mock store reuses mock objects across sessions,
 * which silently breaks after restartApps() replaces the page. The registry
 * installed here dies with the page, matching the fresh-process-per-test
 * isolation model.
 */
export async function mockCommand(
    instanceName: string,
    command: string,
    behavior: {resolve?: unknown; reject?: unknown},
): Promise<void> {
    await tauriApi(instanceName).execute(
        (_tauri, cmd, spec) => {
            const w = window as Window & {
                __wdio_mocks__?: Record<string, () => Promise<unknown>>;
            };
            w.__wdio_mocks__ = w.__wdio_mocks__ ?? {};
            w.__wdio_mocks__[cmd] =
                spec.reject !== undefined
                    ? () => Promise.reject(spec.reject)
                    : () => Promise.resolve(spec.resolve);
        },
        command,
        behavior,
    );
}

/** Removes a command mock installed by mockCommand. */
export async function unmockCommand(instanceName: string, command: string): Promise<void> {
    await tauriApi(instanceName).execute((_tauri, cmd) => {
        const w = window as Window & {
            __wdio_mocks__?: Record<string, () => Promise<unknown>>;
        };
        delete w.__wdio_mocks__?.[cmd];
    }, command);
}

/**
 * Clicks an element by executing a JS click in the browser context.
 * This is a workaround for WebKitWebDriver (Linux) not supporting native
 * WebDriver element clicks. Works consistently across all platforms.
 */
export async function click(
    browser: WebdriverIO.Browser,
    element: ChainablePromiseElement,
): Promise<void> {
    // Resolve the chainable first: passing an unresolved chainable into
    // execute() serializes to something without a click method.
    const el = await element;
    await browser.execute((e: HTMLElement) => e.click(), el);
}

/**
 * Returns the client key button for the given display name (CID for clients
 * without a resolved position) on the client page. Client keys are w-25 sized
 * buttons whose name line carries the full display name in its title attribute.
 */
export function clientKey(
    browser: WebdriverIO.Browser,
    displayName: string,
): ChainablePromiseElement {
    return browser.$(`//button[contains(@class, "w-25")][.//p[@title="${displayName}"]]`);
}

/**
 * Returns the call queue slot (call display or incoming answer key) labeled
 * with the given display name. Queue slots are h-16 sized buttons in the
 * right-hand column.
 */
export function callQueueSlot(
    browser: WebdriverIO.Browser,
    displayName: string,
): ChainablePromiseElement {
    return browser.$(`//button[contains(@class, "h-16")][.//p[@title="${displayName}"]]`);
}

/**
 * Selects an option of a native select element by value. Uses a JS-dispatched
 * change event since WebKitWebDriver does not support native option clicks.
 */
export async function selectOption(
    browser: WebdriverIO.Browser,
    cssSelector: string,
    value: string,
): Promise<void> {
    await browser.execute(
        (sel: string, val: string) => {
            const el = document.querySelector<HTMLSelectElement>(sel);
            if (el === null) throw new Error(`Select not found: ${sel}`);
            el.value = val;
            el.dispatchEvent(new Event("change", {bubbles: true}));
        },
        cssSelector,
        value,
    );
}

/**
 * Waits until the given element's class list contains (or no longer contains)
 * the given class fragment.
 */
export async function waitForClass(
    browser: WebdriverIO.Browser,
    element: ChainablePromiseElement,
    cls: string,
    options: {present: boolean},
): Promise<void> {
    await browser.waitUntil(
        async () => {
            const classes = (await element.getAttribute("class")) ?? "";
            return classes.includes(cls) === options.present;
        },
        {
            timeoutMsg: `Element class list did not ${options.present ? "gain" : "lose"} "${cls}"`,
        },
    );
}

/**
 * Waits until the given element's class list contains (or no longer contains)
 * the marker for an active call (steady green key).
 */
export async function waitForCallColor(
    browser: WebdriverIO.Browser,
    element: ChainablePromiseElement,
    options: {active: boolean},
): Promise<void> {
    await browser.waitUntil(
        async () => {
            const classes = (await element.getAttribute("class")) ?? "";
            return classes.includes("bg-[#4b8747]") === options.active;
        },
        {
            timeoutMsg: `Element did not become ${options.active ? "active (green)" : "idle"}`,
        },
    );
}
