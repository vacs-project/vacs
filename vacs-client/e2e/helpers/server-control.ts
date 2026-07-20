type ServerControl = {
    stop: () => Promise<void>;
    start: () => Promise<void>;
};

declare global {
    // eslint-disable-next-line no-var
    var __vacsServerControl: ServerControl | undefined;
}

function control(): ServerControl {
    if (globalThis.__vacsServerControl === undefined) {
        throw new Error("Server control is not available (set up in wdio.conf.ts beforeSession)");
    }
    return globalThis.__vacsServerControl;
}

/** Kills the vacs-server process, simulating an abrupt outage. */
export async function stopVacsServer(): Promise<void> {
    await control().stop();
}

/** Restarts the vacs-server process and waits until it accepts connections. */
export async function startVacsServer(): Promise<void> {
    await control().start();
}
