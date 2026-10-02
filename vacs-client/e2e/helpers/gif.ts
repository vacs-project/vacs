import {writeFileSync} from "node:fs";
import gifenc from "gifenc";
import type {PNG} from "pngjs";
import {outputPath} from "./screenshot.ts";

/** One image of an animation and how long it stays on screen, in milliseconds. */
export type GifFrame = {png: PNG; delay: number};

/**
 * Encodes frames into a looping GIF under SCREENSHOT_DIR.
 *
 * One palette is quantized from all frames together, so a color keeps its
 * index from frame to frame: per-frame palettes would make the flat UI
 * surfaces flicker between near-identical shades. Every frame after the first
 * stores only the pixels that changed and leaves the rest transparent over the
 * previous frame, which is what keeps a recording of a mostly static window
 * small; gifenc cannot write partial frames, so this is the size lever.
 */
export function writeGif(name: string, frames: GifFrame[]): string {
    if (frames.length === 0) throw new Error(`No frames for ${name}`);
    const {width, height} = frames[0].png;
    for (const frame of frames) {
        if (frame.png.width !== width || frame.png.height !== height) {
            throw new Error(`Frame size mismatch in ${name}`);
        }
    }

    const combined = new Uint8Array(width * height * 4 * frames.length);
    frames.forEach((frame, index) => combined.set(frame.png.data, index * width * height * 4));
    // One entry stays free for the transparent index.
    const colors = gifenc.quantize(combined, 255);
    const transparentIndex = colors.length;
    const palette = [...colors, [0, 0, 0]];

    const encoder = gifenc.GIFEncoder();
    let previous: Uint8Array | undefined;
    frames.forEach((frame, index) => {
        const indexed = gifenc.applyPalette(new Uint8Array(frame.png.data), colors);
        const stored = indexed.slice();
        if (previous !== undefined) {
            for (let pixel = 0; pixel < stored.length; pixel++) {
                if (indexed[pixel] === previous[pixel]) stored[pixel] = transparentIndex;
            }
        }
        encoder.writeFrame(stored, width, height, {
            palette: index === 0 ? palette : undefined,
            delay: frame.delay,
            repeat: 0,
            transparent: previous !== undefined,
            transparentIndex,
            // Keep the previous frame on screen under the transparent pixels.
            dispose: 1,
        });
        previous = indexed;
    });
    encoder.finish();

    const target = outputPath(name);
    writeFileSync(target, encoder.bytes());
    console.log(`screenshot: ${target} (${width}x${height}, ${frames.length} frames)`);
    return target;
}
