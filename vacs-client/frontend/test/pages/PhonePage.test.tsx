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

import PhonePage from "../../src/pages/PhonePage.tsx";
import {useAuthStore} from "../../src/stores/auth-store.ts";
import {useConnectionStore} from "../../src/stores/connection-store.ts";
import {useProfileStore} from "../../src/stores/profile-store.ts";
import {ClientId} from "../../src/types/generic.ts";

afterEach(() => {
    useAuthStore.setState({cid: undefined, status: "loading"});
    useConnectionStore.setState({connectionState: "disconnected"});
    useProfileStore.getState().reset();
    vi.clearAllMocks();
    cleanup();
});

describe("PhonePage", () => {
    it("scrolls the fallback client page sideways with the wheel and marks hidden content", () => {
        useAuthStore.getState().setAuthenticated("1000000" as ClientId);
        useConnectionStore.setState({connectionState: "connected"});
        const {container} = render(<PhonePage />);
        const scroller = container.firstElementChild!.firstElementChild as HTMLElement;
        const size = {scrollWidth: 800, clientWidth: 400, scrollHeight: 300, clientHeight: 300};
        for (const [key, value] of Object.entries(size)) {
            Object.defineProperty(scroller, key, {value});
        }
        const scrollBy = vi.fn<(options: ScrollToOptions) => void>();
        Object.defineProperty(scroller, "scrollBy", {value: scrollBy});

        fireEvent.scroll(scroller);
        fireEvent.wheel(scroller, {deltaY: 100});

        expect(
            container
                .querySelector('[data-overflow-edge="right"]')
                ?.classList.contains("opacity-100"),
        ).toBe(true);
        expect(scrollBy).toHaveBeenCalledExactlyOnceWith({left: 100, behavior: "smooth"});
    });
});
