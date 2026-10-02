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

/**
 * Installs the same mock as mockCommand through the session's own execute
 * rather than the service's direct eval channel. For the remote config, which
 * does not register @wdio/tauri-service and therefore has no browser.tauri on
 * its app instance.
 */
export async function mockCommandOn(
    browser: WebdriverIO.Browser,
    command: string,
    behavior: {resolve?: unknown; reject?: unknown},
): Promise<void> {
    await browser.execute(
        (cmd: string, spec: {resolve?: unknown; reject?: unknown}) => {
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
 * without a resolved position) on the client page. Client keys are the only
 * buttons whose name line is a `w-full truncate` paragraph carrying the full
 * display name in its title attribute.
 */
export function clientKey(
    browser: WebdriverIO.Browser,
    displayName: string,
): ChainablePromiseElement {
    return browser.$(`//button[.//p[@class="w-full truncate"][@title="${displayName}"]]`);
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
 * Returns the call display: the topmost call queue slot, which shows the
 * client's own current call. Matched structurally rather than by label,
 * because a conference call display is labeled "CONF" and carries no title
 * attribute for callQueueSlot to key off. The element exists exactly while
 * the client has a call display, so its absence means "no call".
 */
export function callDisplaySlot(browser: WebdriverIO.Browser): ChainablePromiseElement {
    return browser.$(
        '//div[contains(@class, "scrollbar-none")]' +
            '/div[contains(@class, "relative")]/button[contains(@class, "h-16")]',
    );
}

/**
 * Returns the CONF function key, which opens and closes conference modify
 * mode. Matched by its title attribute: a conference call display and a
 * conference answer key carry the same "CONF" text.
 */
export function conferenceKey(browser: WebdriverIO.Browser): ChainablePromiseElement {
    return browser.$('//button[@title="Conference Call"]');
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

/**
 * Brings the client key for the given display name into view, opening the
 * "OTHER" client group (all clients without a resolved VATSIM position) when
 * the page still shows the group keys. Safe to call repeatedly: the group key
 * is gone once the group is open, and END resets the page to the group keys.
 */
export async function showClientKey(
    browser: WebdriverIO.Browser,
    displayName: string,
): Promise<ChainablePromiseElement> {
    await browser.waitUntil(
        async () => {
            if (await clientKey(browser, displayName).isExisting()) return true;
            const group = await browser.$("button*=OTHER");
            if (await group.isDisplayed()) await click(browser, group);
            return false;
        },
        {timeoutMsg: `Client key for ${displayName} did not appear in the OTHER group`},
    );
    const key = clientKey(browser, displayName);
    await key.waitForDisplayed();
    return key;
}

/**
 * Starts a call to the client with the given display name by clicking its
 * client key. Only valid while the client is idle: on a client that already
 * has a call display the same click accepts, drops or ends instead.
 */
export async function startCallTo(
    browser: WebdriverIO.Browser,
    displayName: string,
): Promise<void> {
    const key = await showClientKey(browser, displayName);
    await click(browser, key);
}

/**
 * The green highlight the call display carries while the call is outgoing or
 * rejected. It is an inner div, distinct from the key's own background: an
 * accepted call turns the key itself green and has no highlight.
 */
function callDisplayHighlight(browser: WebdriverIO.Browser): ChainablePromiseElement {
    return callDisplaySlot(browser).$('.//div[contains(@class, "bg-[#4b8747]")]');
}

/**
 * Waits until the call display shows a ringing outgoing call: a steady gray
 * key with the green inner highlight. The sampling window is what separates
 * it from the rejected and errored displays, which carry the same highlight
 * but blink their key color with a 500ms period.
 */
export async function waitForOutgoingCall(browser: WebdriverIO.Browser): Promise<void> {
    await callDisplayHighlight(browser).waitForDisplayed();
    await browser.waitUntil(
        async () => {
            // Longer than two blink periods, so no blink can hide in the gaps.
            for (let sample = 0; sample < 12; sample++) {
                const classes = (await callDisplaySlot(browser).getAttribute("class")) ?? "";
                if (classes.includes("bg-[#4b8747]") || classes.includes("bg-red-500")) {
                    return false;
                }
                await browser.pause(100);
            }
            return true;
        },
        {timeoutMsg: "Call display did not stay a steady outgoing call"},
    );
}

/**
 * Waits until the call display shows a rejected call: the key blinks green
 * while keeping the green inner highlight. Seeing the key green at all is
 * what separates it from the steady-gray outgoing display.
 */
export async function waitForRejectedCall(browser: WebdriverIO.Browser): Promise<void> {
    await callDisplayHighlight(browser).waitForDisplayed();
    await browser.waitUntil(
        async () => {
            const classes = (await callDisplaySlot(browser).getAttribute("class")) ?? "";
            return classes.includes("bg-[#4b8747]");
        },
        {interval: 150, timeoutMsg: "Call display did not blink green for the rejected call"},
    );
}

/**
 * Invites another target into the app instance's current call through the
 * same signaling command a client key invokes. Needed only where the UI
 * offers no affordance: the CONF key stays locked until a call is
 * established and its media connected, so a call whose peers never
 * negotiate (raw signaling clients) can never be grown from the UI, and a
 * fresh call cannot be given a second target at all.
 *
 * Runs the invoke in the page, so it only works on an app instance; the
 * remote browser has no __TAURI_INTERNALS__.
 */
export async function inviteTarget(
    browser: WebdriverIO.Browser,
    ownCid: string,
    targetCid: string,
): Promise<void> {
    const result = await browser.execute(
        async (own: string, target: string) => {
            try {
                await window.__TAURI_INTERNALS__.invoke("signaling_invite_to_call", {
                    source: {clientId: own},
                    targets: [{client: target}],
                    prio: false,
                });
                return {ok: true as const};
            } catch (e) {
                return {ok: false as const, error: String(e)};
            }
        },
        ownCid,
        targetCid,
    );

    if (!result.ok) {
        throw new Error(`signaling_invite_to_call failed for ${targetCid}: ${result.error}`);
    }
}

/**
 * Waits until the given key carries a call error annotation: an errored key
 * blinks red with the 500ms blink period, so this samples until it catches
 * the red half instead of reading the class list once.
 */
export async function waitForErroredKey(
    browser: WebdriverIO.Browser,
    element: ChainablePromiseElement,
): Promise<void> {
    await browser.waitUntil(
        async () => {
            const classes = (await element.getAttribute("class")) ?? "";
            return classes.includes("bg-red-500");
        },
        {interval: 150, timeoutMsg: "Key did not blink red for the errored target"},
    );
}

/**
 * Returns one of the page tabs a split-view profile shows in the bottom row
 * ("Phone" or "Radio"). Matched on the tab button's rounded-b-lg shape so the
 * ordinary Radio/Phone buttons, whose label is the same text, cannot match.
 */
export function pageTab(browser: WebdriverIO.Browser, label: string): ChainablePromiseElement {
    return browser.$(`//button[contains(@class, "rounded-b-lg")][.//p[@title="${label}"]]`);
}

/**
 * Returns the ordinary Radio or Phone page button of the bottom row. Its label
 * sits in a plain paragraph, which is what separates it from the page tab of a
 * split-view profile and from a direct access key carrying the same text.
 */
export function pageButton(browser: WebdriverIO.Browser, label: string): ChainablePromiseElement {
    return browser.$(`//button[./p[not(@title) and text()="${label}"]]`);
}

/** Returns the Page button a cycle-view profile shows instead of the tabs. */
export function pageCycleButton(browser: WebdriverIO.Browser): ChainablePromiseElement {
    return browser.$('//button[contains(@class, "w-22")][./div/p[text()="Page"]]');
}

/**
 * Returns one of the Page button's R/P/M cells. The active cell is the one
 * carrying bg-gray-400.
 */
export function pageCycleCell(
    browser: WebdriverIO.Browser,
    letter: "R" | "P" | "M",
): ChainablePromiseElement {
    return pageCycleButton(browser).$(`.//p[text()="${letter}"]`);
}

/**
 * Returns the drag handle between the radio and phone panes of the mixed page.
 * Its existence is what distinguishes the mixed page from the single-page
 * radio and phone layouts. Assert on existence, never on displayedness: the
 * handle is opacity-0 until hovered, and a zero-opacity element is reported as
 * not displayed in both directions.
 */
export function splitResizeHandle(browser: WebdriverIO.Browser): ChainablePromiseElement {
    return browser.$('//div[contains(@class, "cursor-ew-resize")]');
}

/**
 * Returns the phone pane of the mixed page: the only page container with an
 * explicit width, keyed off the minimum width that clamps the drag.
 */
export function splitPhonePane(browser: WebdriverIO.Browser): ChainablePromiseElement {
    return browser.$('//div[contains(@class, "min-w-[calc(5.5rem+0.875rem+3px)]")]');
}

/**
 * Returns the frequency objects of the radio page. Keyed off the tile's fixed
 * height, which is the one class it keeps in both the full and the split radio
 * page; its width and column template change with the layout.
 */
export function frequencyObjects(browser: WebdriverIO.Browser): ChainablePromiseArray {
    return browser.$$('//div[contains(@class, "h-[6.188rem]")]');
}

/**
 * Drags an element horizontally by dispatching pointer events on it.
 *
 * Two reasons this is not a WebDriver pointer action: the embedded driver
 * synthesizes MouseEvents, which never produce the pointerdown a pointer-event
 * handler listens for, and a synthetic pointer id has no active pointer, so
 * the dragged element's setPointerCapture would throw NotFoundError and abort
 * the handler. The capture is therefore stubbed on the element for the
 * duration of the drag; delivery does not need it, because every event is
 * dispatched on the element itself.
 *
 * `afterMove` runs between the move and the release, which is where a caller
 * waits for the app to re-render: a release handler reading state from its
 * closure would otherwise still see the pre-drag value.
 */
export async function dragHorizontally(
    browser: WebdriverIO.Browser,
    element: ChainablePromiseElement,
    deltaX: number,
    afterMove?: () => Promise<void>,
): Promise<void> {
    const el = await element;

    const target = await browser.execute(
        (node: HTMLElement, dx: number) => {
            const rect = node.getBoundingClientRect();
            const x = rect.x + rect.width / 2;
            const y = rect.y + rect.height / 2;
            const capture = node as HTMLElement & {setPointerCapture: (id: number) => void};
            capture.setPointerCapture = () => {};
            const event = (type: string, clientX: number, buttons: number) =>
                new PointerEvent(type, {
                    bubbles: true,
                    cancelable: true,
                    pointerId: 1,
                    isPrimary: true,
                    button: 0,
                    buttons,
                    clientX,
                    clientY: y,
                });
            node.dispatchEvent(event("pointerdown", x, 1));
            node.dispatchEvent(event("pointermove", x + dx, 1));
            return {x: x + dx, y};
        },
        el,
        deltaX,
    );

    if (afterMove !== undefined) await afterMove();

    await browser.execute(
        (node: HTMLElement, x: number, y: number) => {
            node.dispatchEvent(
                new PointerEvent("pointerup", {
                    bubbles: true,
                    cancelable: true,
                    pointerId: 1,
                    isPrimary: true,
                    button: 0,
                    buttons: 0,
                    clientX: x,
                    clientY: y,
                }),
            );
            delete (node as Partial<HTMLElement>).setPointerCapture;
        },
        el,
        target.x,
        target.y,
    );
}

/** Double-clicks an element, for the same reason click() dispatches in the page. */
export async function doubleClick(
    browser: WebdriverIO.Browser,
    element: ChainablePromiseElement,
): Promise<void> {
    const el = await element;
    await browser.execute((node: HTMLElement) => {
        node.dispatchEvent(new MouseEvent("dblclick", {bubbles: true, cancelable: true}));
    }, el);
}

/**
 * Clicks an <svg> element. HTMLElement.click() does not exist on SVGElement,
 * so click() cannot be used on the icon buttons the settings pages render
 * (the ring sound reset).
 */
export async function clickSvg(
    browser: WebdriverIO.Browser,
    element: ChainablePromiseElement,
): Promise<void> {
    const el = await element;
    await browser.execute((node: Element) => {
        node.dispatchEvent(new MouseEvent("click", {bubbles: true, cancelable: true}));
    }, el);
}

/**
 * Returns the ring sound field of the "Ring" or "Priority ring" row in
 * Settings > Call: the field is the first div following its label in the ring
 * sounds grid, and carries the file name or "Built-in chime" as its text.
 */
export function ringSoundField(
    browser: WebdriverIO.Browser,
    label: RingSoundLabel,
): ChainablePromiseElement {
    return browser.$(`${ringSoundRow(label)}/div[1]`);
}

/**
 * Returns the X that resets a ring sound to the built-in chime: an <svg>
 * sibling of the field, so it needs local-name() to match and clickSvg() to be
 * clicked.
 */
export function ringSoundReset(
    browser: WebdriverIO.Browser,
    label: RingSoundLabel,
): ChainablePromiseElement {
    return browser.$(`${ringSoundRow(label)}/*[local-name()="svg"]`);
}

/** Waits until a ring sound field shows the given file name or "Built-in chime". */
export async function waitForRingSound(
    browser: WebdriverIO.Browser,
    label: RingSoundLabel,
    expected: string,
): Promise<void> {
    await browser.waitUntil(
        async () => (await ringSoundField(browser, label).getText()) === expected,
        {timeoutMsg: `The ${label} field did not show "${expected}"`},
    );
}

type RingSoundLabel = "Ring" | "Priority ring";

// While the ring sounds are still being fetched both rows render a paragraph
// instead, so this matches nothing rather than the wrong row.
const ringSoundRow = (label: RingSoundLabel) => `//p[text()="${label}"]/following-sibling::div[1]`;
