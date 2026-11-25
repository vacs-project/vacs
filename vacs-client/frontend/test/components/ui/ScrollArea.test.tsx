import {afterEach, describe, expect, it, vi} from "vitest";
import {act, cleanup, fireEvent, render, waitFor} from "@testing-library/preact";
import {TargetedWheelEvent} from "preact";
import ScrollArea from "../../../src/components/ui/ScrollArea.tsx";

afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
});

type Metrics = Partial<
    Record<
        | "scrollTop"
        | "scrollLeft"
        | "scrollHeight"
        | "scrollWidth"
        | "clientHeight"
        | "clientWidth"
        | "offsetLeft"
        | "offsetWidth",
        number
    >
>;

function setMetrics(element: Element, metrics: Metrics) {
    for (const [key, value] of Object.entries(metrics)) {
        Object.defineProperty(element, key, {value, configurable: true});
    }
}

const SIZE: Metrics = {scrollHeight: 300, clientHeight: 300, scrollWidth: 400, clientWidth: 400};

function stubResizeObserver() {
    const observed = new Set<Element>();
    let callback: () => void = () => {};
    vi.stubGlobal(
        "ResizeObserver",
        class {
            constructor(cb: () => void) {
                callback = cb;
            }
            observe(target: Element) {
                observed.add(target);
            }
            unobserve(target: Element) {
                observed.delete(target);
            }
            disconnect() {
                observed.clear();
            }
        },
    );
    return {
        resize: (target: Element) => {
            if (observed.has(target)) void act(() => callback());
        },
    };
}

function area(reversed: boolean, children: number, width = 0) {
    return (
        <ScrollArea reversed={reversed}>
            {Array.from({length: children}, (_, i) => (
                <div key={i} style={{width: `${width}px`}} />
            ))}
        </ScrollArea>
    );
}

function renderArea(reversed = false) {
    const {container, rerender} = render(area(reversed, 2));
    const scroller = container.firstElementChild!.firstElementChild as HTMLElement;
    const scrollTo = (metrics: Metrics) => {
        setMetrics(scroller, {...SIZE, ...metrics});
        fireEvent.scroll(scroller);
    };
    const tab = (edge: string) =>
        container.querySelector(`[data-overflow-edge="${edge}"]`) as HTMLElement;
    const visibleEdges = () =>
        ["top", "bottom", "left", "right"].filter(edge =>
            tab(edge).classList.contains("opacity-100"),
        );
    return {scroller, scrollTo, tab, visibleEdges, rerender};
}

describe("ScrollArea", () => {
    it("hides the scrollbar", () => {
        const {scroller} = renderArea();

        expect(scroller.classList.contains("overflow-auto")).toBe(true);
        expect(scroller.classList.contains("scrollbar-none")).toBe(true);
    });

    it("shows no cue while the content fits", () => {
        const {scrollTo, visibleEdges} = renderArea();

        scrollTo({});

        expect(visibleEdges()).toEqual([]);
    });

    it("ignores an overflow of a single pixel", () => {
        const {scrollTo, visibleEdges} = renderArea();

        scrollTo({scrollHeight: 301, scrollWidth: 401});

        expect(visibleEdges()).toEqual([]);
    });

    it("marks the edges with hidden content while scrolling down", () => {
        const {scrollTo, visibleEdges} = renderArea();

        scrollTo({scrollHeight: 600, scrollTop: 0});
        expect(visibleEdges()).toEqual(["bottom"]);

        scrollTo({scrollHeight: 600, scrollTop: 150});
        expect(visibleEdges()).toEqual(["top", "bottom"]);

        scrollTo({scrollHeight: 600, scrollTop: 300});
        expect(visibleEdges()).toEqual(["top"]);
    });

    it("reads a reversed area from the bottom up", () => {
        const {scrollTo, visibleEdges} = renderArea(true);

        scrollTo({scrollHeight: 600, scrollTop: 0});
        expect(visibleEdges()).toEqual(["top"]);

        scrollTo({scrollHeight: 600, scrollTop: -150});
        expect(visibleEdges()).toEqual(["top", "bottom"]);

        scrollTo({scrollHeight: 600, scrollTop: -300});
        expect(visibleEdges()).toEqual(["bottom"]);
    });

    it("marks the edges with hidden content while scrolling sideways", () => {
        const {scrollTo, visibleEdges} = renderArea();

        scrollTo({scrollWidth: 800, scrollLeft: 0});
        expect(visibleEdges()).toEqual(["right"]);

        scrollTo({scrollWidth: 800, scrollLeft: 400});
        expect(visibleEdges()).toEqual(["left"]);
    });

    it("centers the top and bottom tabs on the content", () => {
        const {scroller, scrollTo, tab} = renderArea();
        const [first, second] = Array.from(scroller.children);
        setMetrics(first, {offsetLeft: 6, offsetWidth: 160});
        setMetrics(second, {offsetLeft: 174, offsetWidth: 160});

        scrollTo({scrollHeight: 600});

        expect(tab("top").style.left).toBe("170px");
        expect(tab("bottom").style.left).toBe("170px");
    });

    it("centers the top and bottom tabs on the area while it scrolls sideways", () => {
        const {scroller, scrollTo, tab} = renderArea();
        setMetrics(scroller.children[0], {offsetLeft: 0, offsetWidth: 800});

        scrollTo({scrollHeight: 600, scrollWidth: 800, scrollLeft: 100});

        expect(tab("top").style.left).toBe("200px");
    });

    it("marks an edge when content is added without a scroll event", async () => {
        const {scroller, rerender, visibleEdges} = renderArea();
        setMetrics(scroller, {...SIZE, scrollHeight: 600});

        rerender(area(false, 3));

        await waitFor(() => expect(visibleEdges()).toEqual(["bottom"]));
    });

    it("marks an edge when the area is resized", () => {
        const {resize} = stubResizeObserver();
        const {scroller, visibleEdges} = renderArea();
        setMetrics(scroller, {...SIZE, scrollWidth: 800});

        resize(scroller);

        expect(visibleEdges()).toEqual(["right"]);
    });

    it("marks an edge when the content resizes without adding children", () => {
        const {resize} = stubResizeObserver();
        const {scroller, rerender, visibleEdges} = renderArea();
        setMetrics(scroller, {...SIZE, scrollWidth: 800});

        rerender(area(false, 2, 400));
        resize(scroller.children[0]);

        expect(visibleEdges()).toEqual(["right"]);
    });

    it("hands wheel events of the scrolling element to onWheel", () => {
        const onWheel = vi.fn<(event: TargetedWheelEvent<HTMLDivElement>) => void>();
        let target: EventTarget | null = null;
        onWheel.mockImplementation(event => {
            target = event.currentTarget;
        });
        const {container} = render(
            <ScrollArea onWheel={onWheel}>
                <div />
            </ScrollArea>,
        );
        const scroller = container.firstElementChild!.firstElementChild as HTMLElement;

        fireEvent.wheel(scroller, {deltaY: 100});

        expect(onWheel).toHaveBeenCalledOnce();
        expect(target).toBe(scroller);
    });
});
