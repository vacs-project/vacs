import {copyFileSync, mkdirSync, writeFileSync} from "node:fs";
import path from "node:path";
import {fileURLToPath} from "node:url";
import {PNG} from "pngjs";
import type {ChainablePromiseElement} from "webdriverio";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

/**
 * Output root for captured images. Point VACS_SCREENSHOT_DIR at the docs
 * repo's static/img to write straight into it.
 */
export const SCREENSHOT_DIR =
    process.env.VACS_SCREENSHOT_DIR ?? path.resolve(__dirname, "..", "screenshots");

/**
 * Captures the whole webview. The embedded driver snapshots webview content
 * only, so the result carries no window decorations.
 *
 * `settle` overrides the pause before the snapshot. Lower it only for a state
 * the UI does not hold still, such as a blinking key, where waiting out the
 * color transition would also wait out the phase being captured.
 */
export async function captureWindow(
    browser: WebdriverIO.Browser,
    name: string,
    options: {settle?: number} = {},
): Promise<string> {
    return writeImage(await frame(browser, options.settle), name);
}

/**
 * Captures the region covered by an element, optionally with some CSS pixels
 * of padding around it.
 *
 * The embedded driver's element screenshot only scrolls the element into view
 * and returns the full frame (see tauri-plugin-wdio-webdriver's platform
 * implementations), so the crop happens here instead.
 */
export async function captureElement(
    browser: WebdriverIO.Browser,
    element: ChainablePromiseElement,
    name: string,
    options: {padding?: number} = {},
): Promise<string> {
    const el = await element;
    const rect = await browser.execute((e: HTMLElement) => {
        const r = e.getBoundingClientRect();
        return {x: r.x, y: r.y, width: r.width, height: r.height};
    }, el);
    const padding = options.padding ?? 0;
    return captureRect(browser, name, {
        x: rect.x - padding,
        y: rect.y - padding,
        width: rect.width + 2 * padding,
        height: rect.height + 2 * padding,
    });
}

/** A region of the webview in CSS pixels. */
export type Rect = {x: number; y: number; width: number; height: number};

/**
 * Captures a region of the webview given in CSS pixels, for crops that follow
 * no single element (a strip of the window, a group of controls).
 */
export async function captureRect(
    browser: WebdriverIO.Browser,
    name: string,
    rect: Rect,
): Promise<string> {
    return writeImage(await captureFrame(browser, {rect}), name);
}

/**
 * Takes one snapshot of the webview, optionally cropped to a region in CSS
 * pixels, without writing it. The building block for animations.
 */
export async function captureFrame(
    browser: WebdriverIO.Browser,
    options: {rect?: Rect; settle?: number} = {},
): Promise<PNG> {
    const viewport = await browser.execute(() => window.innerWidth);
    const png = await frame(browser, options.settle);
    return options.rect === undefined ? png : crop(png, options.rect, png.width / viewport);
}

/**
 * The bounding rect, in CSS pixels, of everything the given selectors match
 * (the first match each). XPath when the selector starts with / or (.
 */
export async function unionRect(browser: WebdriverIO.Browser, selectors: string[]): Promise<Rect> {
    return browser.execute((list: string[]) => {
        const resolve = (selector: string): Element | null =>
            selector.startsWith("/") || selector.startsWith("(")
                ? (document.evaluate(
                      selector,
                      document,
                      null,
                      XPathResult.FIRST_ORDERED_NODE_TYPE,
                      null,
                  ).singleNodeValue as Element | null)
                : document.querySelector(selector);
        let left = Infinity;
        let top = Infinity;
        let right = -Infinity;
        let bottom = -Infinity;
        for (const selector of list) {
            const el = resolve(selector);
            if (el === null) throw new Error(`Crop target not found: ${selector}`);
            const r = el.getBoundingClientRect();
            left = Math.min(left, r.left);
            top = Math.min(top, r.top);
            right = Math.max(right, r.right);
            bottom = Math.max(bottom, r.bottom);
        }
        return {x: left, y: top, width: right - left, height: bottom - top};
    }, selectors);
}

/** The webview's size in CSS pixels, as a rect at the origin. */
export async function viewportRect(browser: WebdriverIO.Browser): Promise<Rect> {
    return browser.execute(() => ({
        x: 0,
        y: 0,
        width: window.innerWidth,
        height: window.innerHeight,
    }));
}

/**
 * Pins the page's clock to a fixed instant, so the header shows the same
 * time in every capture instead of whenever the suite happened to run.
 *
 * The clock reads `new Date()` on a one second timer, so the display follows
 * within a tick. Only the webview's Date is replaced; the backend keeps real
 * time.
 */
export async function freezeClock(browser: WebdriverIO.Browser, iso: string): Promise<void> {
    await browser.execute((fixedIso: string) => {
        const NativeDate = Date;
        const fixed = new NativeDate(fixedIso).getTime();

        function FixedDate(this: unknown, ...args: unknown[]) {
            if (!(this instanceof FixedDate)) return new NativeDate(fixed).toString();
            return args.length === 0
                ? new NativeDate(fixed)
                : new (NativeDate as new (...a: unknown[]) => Date)(...args);
        }
        FixedDate.prototype = NativeDate.prototype;
        FixedDate.now = () => fixed;
        FixedDate.parse = NativeDate.parse;
        FixedDate.UTC = NativeDate.UTC;

        window.Date = FixedDate as unknown as DateConstructor;
    }, iso);
}

/** Long enough for the UI's color transitions to finish (150ms in Tailwind). */
const SETTLE_MS = 300;

async function frame(browser: WebdriverIO.Browser, settle?: number): Promise<PNG> {
    // Without this, a capture taken right after a state change can land
    // mid-transition, which makes the same image differ between runs: the
    // clear buttons next to the key fields animate their stroke color.
    await browser.pause(settle ?? SETTLE_MS);
    const encoded = await browser.takeScreenshot();
    return PNG.sync.read(Buffer.from(encoded, "base64"));
}

// Row-wise copy rather than PNG.bitblt: what PNG.sync.read returns is a bare
// bitmap object without the prototype's blitting methods. The snapshot is in
// device pixels and the rect in CSS pixels; deriving the factor from the frame
// keeps this correct under a zoom level or HiDPI scaling.
function crop(png: PNG, rect: Rect, scale: number): PNG {
    const left = clamp(Math.round(rect.x * scale), 0, png.width - 1);
    const top = clamp(Math.round(rect.y * scale), 0, png.height - 1);
    const right = clamp(Math.round((rect.x + rect.width) * scale), left + 1, png.width);
    const bottom = clamp(Math.round((rect.y + rect.height) * scale), top + 1, png.height);

    const out = new PNG({width: right - left, height: bottom - top});
    for (let row = 0; row < out.height; row++) {
        const start = ((top + row) * png.width + left) * 4;
        png.data.copy(out.data, row * out.width * 4, start, start + out.width * 4);
    }
    return out;
}

/** Writes an already captured image under SCREENSHOT_DIR. */
export function writeImage(png: PNG, name: string): string {
    const target = outputPath(name);
    writeFileSync(target, PNG.sync.write(png));
    console.log(`screenshot: ${target} (${png.width}x${png.height})`);
    return target;
}

/**
 * Writes an already captured image under further names, for the manual pages
 * that each reference their own copy of the same picture.
 */
export function copyImage(target: string, ...names: string[]): void {
    for (const name of names) {
        const copy = outputPath(name);
        copyFileSync(target, copy);
        console.log(`screenshot: ${copy} (copy of ${path.basename(target)})`);
    }
}

/** Resolves an image name under SCREENSHOT_DIR and creates its directory. */
export function outputPath(name: string): string {
    const target = path.resolve(SCREENSHOT_DIR, name);
    mkdirSync(path.dirname(target), {recursive: true});
    return target;
}

function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), max);
}
