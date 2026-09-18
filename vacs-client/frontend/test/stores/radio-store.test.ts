import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import {retryRadioConnection, useRadioStore} from "../../src/stores/radio-store.ts";
import {RadioState} from "../../src/types/radio.ts";

afterEach(() => {
    useRadioStore.setState({radioState: undefined});
    vi.clearAllMocks();
});

describe("retryRadioConnection", () => {
    it.each<RadioState["state"]>(["Disconnected", "Error"])(
        "reconnects when the radio state is %s",
        state => {
            useRadioStore.setState({radioState: {state}});

            retryRadioConnection();

            expect(invoke).toHaveBeenCalledWith("radio_reconnect", undefined);
        },
    );

    it.each<RadioState["state"] | undefined>(["Connected", "NotConfigured", undefined])(
        "does not reconnect when the radio state is %s",
        state => {
            useRadioStore.setState({radioState: state === undefined ? undefined : {state}});

            retryRadioConnection();

            expect(invoke.mock.calls.map(call => call[0])).not.toContain("radio_reconnect");
        },
    );
});
