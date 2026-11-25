import {afterEach, describe, expect, it, vi} from "vitest";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: false, isRemote: () => true}));

import {cleanup, render} from "@testing-library/preact";
import DirectAccessPage from "../../src/components/DirectAccessPage.tsx";
import type {DirectAccessPage as DirectAccessPageModel} from "../../src/types/profile.ts";

afterEach(cleanup);

describe("DirectAccessPage", () => {
    it("scrolls on overflow and keeps the scrollbar visible", () => {
        const page = {rows: 6, keys: []} as unknown as DirectAccessPageModel;

        const {container} = render(<DirectAccessPage data={page} />);
        const scroller = container.firstElementChild as HTMLElement;

        expect(scroller.classList.contains("overflow-auto")).toBe(true);
        expect(scroller.classList.contains("scrollbar-none")).toBe(false);
    });
});
