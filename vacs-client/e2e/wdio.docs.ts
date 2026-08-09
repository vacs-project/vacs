import {config as baseConfig} from "./wdio.conf.ts";

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
