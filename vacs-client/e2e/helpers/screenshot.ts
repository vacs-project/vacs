import {mkdirSync, writeFileSync} from "node:fs";
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
 */
export async function captureWindow(browser: WebdriverIO.Browser, name: string): Promise<string> {
    return write(await frame(browser), name);
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
        return {x: r.x, y: r.y, width: r.width, height: r.height, viewport: window.innerWidth};
    }, el);

    const png = await frame(browser);
    // The snapshot is in device pixels, getBoundingClientRect in CSS pixels.
    // Deriving the factor from the frame keeps this correct under a zoom
    // level or HiDPI scaling instead of assuming devicePixelRatio 1.
    const scale = png.width / rect.viewport;
    const padding = (options.padding ?? 0) * scale;

    const left = clamp(Math.round(rect.x * scale - padding), 0, png.width);
    const top = clamp(Math.round(rect.y * scale - padding), 0, png.height);
    const right = clamp(Math.round((rect.x + rect.width) * scale + padding), left + 1, png.width);
    const bottom = clamp(Math.round((rect.y + rect.height) * scale + padding), top + 1, png.height);

    // Row-wise copy rather than PNG.bitblt: what PNG.sync.read returns is a
    // bare bitmap object without the prototype's blitting methods.
    const crop = new PNG({width: right - left, height: bottom - top});
    for (let row = 0; row < crop.height; row++) {
        const start = ((top + row) * png.width + left) * 4;
        png.data.copy(crop.data, row * crop.width * 4, start, start + crop.width * 4);
    }
    return write(crop, name);
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

async function frame(browser: WebdriverIO.Browser): Promise<PNG> {
    // Without this, a capture taken right after a state change can land
    // mid-transition, which makes the same image differ between runs: the
    // clear buttons next to the key fields animate their stroke color.
    await browser.pause(SETTLE_MS);
    const encoded = await browser.takeScreenshot();
    return PNG.sync.read(Buffer.from(encoded, "base64"));
}

function write(png: PNG, name: string): string {
    const target = path.resolve(SCREENSHOT_DIR, name);
    mkdirSync(path.dirname(target), {recursive: true});
    writeFileSync(target, PNG.sync.write(png));
    console.log(`screenshot: ${target} (${png.width}x${png.height})`);
    return target;
}

function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), max);
}
