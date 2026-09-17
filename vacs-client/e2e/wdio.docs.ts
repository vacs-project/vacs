import path from "node:path";
import {fileURLToPath} from "node:url";
import {config as baseConfig} from "./wdio.conf.ts";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// App instances inherit this environment, and the settings page images show
// the device selects, so the mock backend gets presentable device names here
// rather than the defaults the behavioral suite asserts on.
process.env.VACS_MOCK_AUDIO_CONFIG = path.resolve(__dirname, "fixtures", "mock-audio-docs.toml");

/**
 * Documentation screenshot run.
 *
 * Same two app instances, servers and mock VATSIM backend as the regular
 * suite (importing wdio.conf.ts also registers its instance layout and
 * process cleanup); only the specs differ. Kept out of `npm test` because
 * these specs produce artifacts rather than assert behavior.
 *
 * Images land in e2e/screenshots/, or in VACS_SCREENSHOT_DIR when set.
 */
export const config: WebdriverIO.MultiremoteConfig = {
    ...baseConfig,
    specs: ["./specs-docs/**/*.ts"],
    // A retry would re-capture and overwrite; a failed capture should be
    // looked at rather than silently repeated.
    specFileRetries: 0,
};
