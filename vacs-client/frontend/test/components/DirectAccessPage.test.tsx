import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render} from "@testing-library/preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<
        (event: string, callback: (event: {payload: unknown}) => void) => Promise<() => void>
    >(() => Promise.resolve(() => {})),
}));

vi.mock("../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import DirectAccessPage from "../../src/components/DirectAccessPage.tsx";

afterEach(() => {
    vi.clearAllMocks();
    cleanup();
});

function renderPage() {
    const keys = [{label: ["A"]}, {label: ["B"]}];
    const {container} = render(<DirectAccessPage data={{rows: 1, keys}} />);
    const scroller = container.firstElementChild!.firstElementChild as HTMLElement;
    return {scroller};
}

describe("DirectAccessPage", () => {
    it("lays the keys out directly in the scroll content", () => {
        const {scroller} = renderPage();

        const content = scroller.firstElementChild as HTMLElement;

        expect(content.children).toHaveLength(2);
        expect(content.style.gridTemplateRows).toBe("repeat(1, 1fr)");
        expect(content.style.gap).toBe("0.5rem");
    });

    it("scrolls sideways with the mouse wheel", () => {
        const {scroller} = renderPage();
        const size = {scrollWidth: 800, clientWidth: 400, scrollHeight: 300, clientHeight: 300};
        for (const [key, value] of Object.entries(size)) {
            Object.defineProperty(scroller, key, {value});
        }
        const scrollBy = vi.fn<(options: ScrollToOptions) => void>();
        Object.defineProperty(scroller, "scrollBy", {value: scrollBy});

        fireEvent.wheel(scroller, {deltaY: 100});

        expect(scrollBy).toHaveBeenCalledExactlyOnceWith({left: 100, behavior: "smooth"});
    });
});
