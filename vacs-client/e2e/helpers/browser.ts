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
    await browser.execute((el: HTMLElement) => el.click(), element);
}
