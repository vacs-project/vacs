/**
 * Returns a single browser instance from the multiremote session.
 * Defaults to "clientA", which is convenient for tests that only need one client.
 * For multi-client tests, pass the instance name explicitly (e.g., "clientB").
 */
export function getClient(instanceName: string = "clientA"): WebdriverIO.Browser {
    return multiRemoteBrowser.getInstance(instanceName);
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
