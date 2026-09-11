import {afterEach, beforeEach, describe, expect, it, vi} from "vitest";

const {invoke, state} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    state: {isTauri: true},
}));

vi.mock("../../src/transport", () => ({
    invoke,
    get isTauri() {
        return state.isTauri;
    },
}));

describe("openUrl", () => {
    beforeEach(() => {
        vi.resetModules();
    });

    afterEach(() => {
        vi.clearAllMocks();
    });

    it("invokes app_open_url with the url in desktop mode", async () => {
        state.isTauri = true;
        const windowOpen = vi.spyOn(window, "open").mockImplementation(() => null);

        const {openUrl} = await import("../../src/utils/tauri.ts");
        await openUrl("https://docs.vacs.network/x");

        expect(invoke).toHaveBeenCalledTimes(1);
        expect(invoke).toHaveBeenCalledWith("app_open_url", {
            url: "https://docs.vacs.network/x",
        });
        expect(windowOpen).not.toHaveBeenCalled();
    });

    it("opens a new tab without an opener reference in browser mode", async () => {
        state.isTauri = false;
        const windowOpen = vi.spyOn(window, "open").mockImplementation(() => null);

        const {openUrl} = await import("../../src/utils/tauri.ts");
        await openUrl("https://docs.vacs.network/x");

        expect(windowOpen).toHaveBeenCalledTimes(1);
        expect(windowOpen).toHaveBeenCalledWith(
            "https://docs.vacs.network/x",
            "_blank",
            "noopener,noreferrer",
        );
        expect(invoke).not.toHaveBeenCalled();
    });
});
