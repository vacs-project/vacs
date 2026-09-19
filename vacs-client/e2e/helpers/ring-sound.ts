import {mkdirSync, writeFileSync} from "node:fs";
import os from "node:os";
import path from "node:path";

const SAMPLE_RATE = 44_100;
const DURATION_SECONDS = 1;
const TONE_HZ = 440;
const PEAK = 16_000;

// Generated rather than committed: the suite keeps no binary fixtures, and the
// app reads the file from the local filesystem, so a temp directory does.
const FIXTURE_DIR = path.join(os.tmpdir(), "vacs-e2e-ring-sounds");

/** File name of the generated ring sound, which the settings field shows. */
export const RING_SOUND_NAME = "e2e-ring.wav";

/**
 * Writes a ring sound the client accepts (1 s of a mono 16-bit tone) and
 * returns its path. Its 44.1 kHz rate also makes the client resample it to the
 * 48 kHz the mock audio devices run at.
 */
export function writeRingSound(): string {
    return write(RING_SOUND_NAME, toneWav());
}

/**
 * Writes a file the client must reject as a ring sound: text rather than a
 * RIFF header, which fails in the duration probe before anything is decoded.
 */
export function writeInvalidRingSound(): string {
    return write("e2e-not-a-ring.txt", Buffer.from("this is not a wav file\n", "utf-8"));
}

function write(name: string, contents: Buffer): string {
    mkdirSync(FIXTURE_DIR, {recursive: true});
    const file = path.join(FIXTURE_DIR, name);
    writeFileSync(file, contents);
    return file;
}

function toneWav(): Buffer {
    const frames = SAMPLE_RATE * DURATION_SECONDS;
    const data = Buffer.alloc(frames * 2);
    for (let frame = 0; frame < frames; frame++) {
        const value = Math.sin((2 * Math.PI * TONE_HZ * frame) / SAMPLE_RATE) * PEAK;
        data.writeInt16LE(Math.round(value), frame * 2);
    }

    const header = Buffer.alloc(44);
    header.write("RIFF", 0);
    header.writeUInt32LE(36 + data.length, 4);
    header.write("WAVE", 8);
    header.write("fmt ", 12);
    header.writeUInt32LE(16, 16);
    header.writeUInt16LE(1, 20);
    header.writeUInt16LE(1, 22);
    header.writeUInt32LE(SAMPLE_RATE, 24);
    header.writeUInt32LE(SAMPLE_RATE * 2, 28);
    header.writeUInt16LE(2, 32);
    header.writeUInt16LE(16, 34);
    header.write("data", 36);
    header.writeUInt32LE(data.length, 40);

    return Buffer.concat([header, data]);
}
