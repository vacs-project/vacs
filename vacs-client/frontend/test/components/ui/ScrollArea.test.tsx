import {afterEach, describe, expect, it, vi} from "vitest";
import {act, cleanup, fireEvent, render, waitFor} from "@testing-library/preact";
import {TargetedWheelEvent} from "preact";
import ScrollArea from "../../../src/components/ui/ScrollArea.tsx";

afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
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
        | "offsetWidth"
        | "offsetHeight",
        number
    >
>;

function setMetrics(element: Element, metrics: Metrics) {
    for (const [key, value] of Object.entries(metrics)) {
        Object.defineProperty(element, key, {value, configurable: true});
    }
}

const {invoke} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string) => Promise<unknown>>(() => Promise.resolve(undefined)),
}));

vi.mock("../../../src/transport", () => ({invoke, isTauri: true, isRemote: () => false}));

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
        ["top", "bottom", "left", "right"].filter(edge => tab(edge).matches(":enabled"));
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

    it("ignores an overflow within the rounding of the scroll offsets", () => {
        const {scrollTo, visibleEdges} = renderArea();

        scrollTo({scrollHeight: 304, scrollWidth: 404});
        expect(visibleEdges()).toEqual([]);

        scrollTo({scrollHeight: 305, scrollWidth: 405});
        expect(visibleEdges()).toEqual(["bottom", "right"]);
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
        const [first, second] = Array.from(scroller.firstElementChild!.children);
        setMetrics(first, {offsetLeft: 6, offsetWidth: 160});
        setMetrics(second, {offsetLeft: 174, offsetWidth: 160});

        scrollTo({scrollHeight: 600});

        expect(tab("top").style.left).toBe("170px");
        expect(tab("bottom").style.left).toBe("170px");
    });

    it("centers the top and bottom tabs on the area while it scrolls sideways", () => {
        const {scroller, scrollTo, tab} = renderArea();
        setMetrics(scroller.firstElementChild!.firstElementChild!, {
            offsetLeft: 0,
            offsetWidth: 800,
        });

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
        resize(scroller.firstElementChild!.firstElementChild!);

        expect(visibleEdges()).toEqual(["right"]);
    });

    // Three columns of four 80x100px items, 8px apart, behind 8px of padding.
    function renderGrid(reversed = false, scrollWidth = 272, itemHeight = 100) {
        const {container} = render(
            <ScrollArea reversed={reversed} style={{rowGap: "8px", columnGap: "8px"}}>
                {Array.from({length: 12}, (_, i) => (
                    <div key={i} />
                ))}
            </ScrollArea>,
        );
        const scroller = container.firstElementChild!.firstElementChild as HTMLElement;
        const scrollTo = vi.fn<(options: ScrollToOptions) => void>();
        Object.defineProperty(scroller, "scrollTo", {value: scrollTo});
        const item = scroller.firstElementChild!.firstElementChild as HTMLElement;
        item.style.width = "80px";
        item.style.height = `${itemHeight}px`;

        const click = (edge: string, scrollTop: number, scrollLeft = 0) => {
            setMetrics(scroller, {
                scrollTop,
                scrollLeft,
                scrollHeight: 440,
                clientHeight: 200,
                scrollWidth,
                clientWidth: 150,
            });
            fireEvent.scroll(scroller);
            scrollTo.mockClear();
            fireEvent.click(container.querySelector(`[data-overflow-edge="${edge}"]`)!);
            return scrollTo.mock.lastCall?.[0];
        };
        const tab = (edge: string) => container.querySelector(`[data-overflow-edge="${edge}"]`)!;
        const clickAgain = (edge: string) => {
            fireEvent.click(tab(edge));
            return scrollTo.mock.lastCall?.[0];
        };
        return {click, clickAgain, scroller};
    }

    it.each([
        ["from the start to the second row", "bottom", 0, 108],
        ["from between two rows to the next one", "bottom", 150, 216],
        ["past the last row to the end", "bottom", 216, 240],
        ["from the end back onto a row", "top", 240, 216],
        ["from between two rows to the previous one", "top", 150, 108],
        ["from the second row to the start", "top", 108, 0],
    ])("scrolls %s on a tab click", (_, edge, from, to) => {
        const {click} = renderGrid();

        expect(click(edge, from)).toEqual({top: to, behavior: "smooth"});
        expect(invoke).toHaveBeenCalledWith("audio_play_ui_click", undefined);
    });

    it.each([
        ["top", 0, -108],
        ["top", -150, -216],
        ["bottom", -240, -216],
        ["bottom", -108, -0],
    ])("lines rows up from the bottom of a reversed area (%s from %i)", (edge, from, to) => {
        const {click} = renderGrid(true);

        expect(click(edge, from)).toEqual({top: to, behavior: "smooth"});
    });

    it.each([
        ["right", 0, 88],
        ["right", 88, 122],
        ["left", 122, 88],
        ["left", 88, 0],
    ])("scrolls one column at a time (%s from %i)", (edge, from, to) => {
        const {click} = renderGrid();

        expect(click(edge, 0, from)).toEqual({left: to, behavior: "smooth"});
    });

    it.each([
        ["right", 0, 93],
        ["left", 93, 0],
    ])("skips a column just short of the end (%s from %i)", (edge, from, to) => {
        const {click} = renderGrid(false, 243);

        expect(click(edge, 0, from)).toEqual({left: to, behavior: "smooth"});
    });

    it.each([
        ["bottom", 106, 216],
        ["bottom", 105, 216],
        ["top", 110, 0],
    ])("takes a rounded offset as its row (%s from %i)", (edge, from, to) => {
        const {click} = renderGrid();

        expect(click(edge, from)).toEqual({top: to, behavior: "smooth"});
    });

    it("keeps the fraction of an item size", () => {
        const {click} = renderGrid(false, 272, 100.4);

        expect(click("bottom", 108)).toEqual({top: expect.closeTo(216.8), behavior: "smooth"});
    });

    it("steps on from the target of a click that is still scrolling", () => {
        const {click, clickAgain} = renderGrid();

        click("bottom", 0);

        expect(clickAgain("bottom")).toEqual({top: 216, behavior: "smooth"});
    });

    it.each(["wheel", "pointerDown"] as const)(
        "counts from the current position again after a %s",
        input => {
            const {click, clickAgain, scroller} = renderGrid();
            click("bottom", 0);

            fireEvent[input](scroller);

            expect(clickAgain("bottom")).toEqual({top: 108, behavior: "smooth"});
        },
    );

    it("counts from the current position once the target is reached", () => {
        const {click, clickAgain, scroller} = renderGrid();
        click("bottom", 0);
        setMetrics(scroller, {scrollTop: 108});
        fireEvent.scroll(scroller);
        setMetrics(scroller, {scrollTop: 0});

        expect(clickAgain("bottom")).toEqual({top: 108, behavior: "smooth"});
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
