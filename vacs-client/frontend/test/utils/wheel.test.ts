import {describe, expect, it, vi} from "vitest";
import {scrollHorizontallyOnWheel} from "../../src/utils/wheel.ts";

type Size = {scrollWidth: number; clientWidth: number; scrollHeight: number; clientHeight: number};

function container(size: Size) {
    const element = document.createElement("div");
    for (const [key, value] of Object.entries(size)) {
        Object.defineProperty(element, key, {value});
    }
    const scrollBy = vi.fn<(options: ScrollToOptions) => void>();
    Object.defineProperty(element, "scrollBy", {value: scrollBy});
    return {element, scrollBy};
}

const horizontalOnly: Size = {
    scrollWidth: 800,
    clientWidth: 400,
    scrollHeight: 300,
    clientHeight: 300,
};

function wheel(init: WheelEventInit): WheelEvent {
    return new WheelEvent("wheel", {cancelable: true, ...init});
}

describe("scrollHorizontallyOnWheel", () => {
    it("smoothly scrolls a horizontally overflowing container with the vertical wheel", () => {
        const {element, scrollBy} = container(horizontalOnly);
        const event = wheel({deltaY: 100});

        expect(scrollHorizontallyOnWheel(element, event)).toBe(true);
        expect(scrollBy).toHaveBeenCalledExactlyOnceWith({left: 100, behavior: "smooth"});
        expect(event.defaultPrevented).toBe(true);
    });

    it("scrolls back with the wheel turned the other way", () => {
        const {element, scrollBy} = container(horizontalOnly);

        scrollHorizontallyOnWheel(element, wheel({deltaY: -50}));

        expect(scrollBy).toHaveBeenCalledExactlyOnceWith({left: -50, behavior: "smooth"});
    });

    it("converts line deltas to pixels", () => {
        const {element, scrollBy} = container(horizontalOnly);

        scrollHorizontallyOnWheel(
            element,
            wheel({deltaY: 3, deltaMode: WheelEvent.DOM_DELTA_LINE}),
        );

        expect(scrollBy).toHaveBeenCalledExactlyOnceWith({left: 120, behavior: "smooth"});
    });

    it("leaves a container that scrolls vertically to the browser", () => {
        const {element, scrollBy} = container({...horizontalOnly, scrollHeight: 600});
        const event = wheel({deltaY: 100});

        expect(scrollHorizontallyOnWheel(element, event)).toBe(false);
        expect(scrollBy).not.toHaveBeenCalled();
        expect(event.defaultPrevented).toBe(false);
    });

    it("ignores a container without horizontal overflow", () => {
        const {element, scrollBy} = container({...horizontalOnly, scrollWidth: 400});

        expect(scrollHorizontallyOnWheel(element, wheel({deltaY: 100}))).toBe(false);
        expect(scrollBy).not.toHaveBeenCalled();
    });

    it("leaves horizontal and shift wheel events to the browser", () => {
        const {element, scrollBy} = container(horizontalOnly);

        expect(scrollHorizontallyOnWheel(element, wheel({deltaX: 100}))).toBe(false);
        expect(scrollHorizontallyOnWheel(element, wheel({deltaY: 100, shiftKey: true}))).toBe(
            false,
        );
        expect(scrollBy).not.toHaveBeenCalled();
    });

    it("leaves ctrl and meta wheel events to the browser", () => {
        const {element, scrollBy} = container(horizontalOnly);
        const ctrl = wheel({deltaY: 100, ctrlKey: true});
        const meta = wheel({deltaY: 100, metaKey: true});

        expect(scrollHorizontallyOnWheel(element, ctrl)).toBe(false);
        expect(scrollHorizontallyOnWheel(element, meta)).toBe(false);
        expect(scrollBy).not.toHaveBeenCalled();
        expect(ctrl.defaultPrevented).toBe(false);
        expect(meta.defaultPrevented).toBe(false);
    });
});
