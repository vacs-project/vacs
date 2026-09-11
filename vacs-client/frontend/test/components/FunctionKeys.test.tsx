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

afterEach(() => {
    vi.clearAllMocks();
    cleanup();
});

describe("FunctionKeys", () => {
    it("renders the SAY AGAIN key instead of the PLC LSP placeholder", () => {
        render(<FunctionKeys />);

        expect(screen.getByRole("button", {name: "SAYAGAIN"})).toBeDefined();
        expect(screen.queryByRole("button", {name: "PLCLSPon/off"})).toBeNull();
    });
});
