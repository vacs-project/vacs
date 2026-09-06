import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, render, screen} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import FunctionKeys from "../../src/components/FunctionKeys.tsx";
import {useSettingsStore} from "../../src/stores/settings-store.ts";

afterEach(() => {
    useSettingsStore.setState({sayAgainEnabled: false});
    vi.clearAllMocks();
    cleanup();
});

describe("FunctionKeys", () => {
    it("renders the SAY AGAIN button when the setting is enabled", () => {
        useSettingsStore.setState({sayAgainEnabled: true});
        render(<FunctionKeys />);

        expect(screen.getByRole("button", {name: "SAYAGAIN"})).toBeDefined();
    });

    it("renders the disabled PLC LSP placeholder when the setting is disabled", () => {
        useSettingsStore.setState({sayAgainEnabled: false});
        render(<FunctionKeys />);

        const btn = screen.getByRole("button", {name: "PLCLSPon/off"}) as HTMLButtonElement;
        expect(btn.disabled).toBe(true);
    });
});
