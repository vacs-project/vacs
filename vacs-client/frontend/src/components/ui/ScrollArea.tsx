import {clsx} from "clsx";
import {ComponentChildren, CSSProperties, TargetedWheelEvent} from "preact";
import {useEffect, useRef, useState} from "preact/hooks";
import {invokeSafe} from "../../error.ts";

type Edge = "top" | "bottom" | "left" | "right";

type Overflow = Record<Edge, boolean> & {
    /** Horizontal center of the content in px, or of the area while it scrolls sideways. */
    centerX: number;
};

const NO_OVERFLOW: Overflow = {top: false, bottom: false, left: false, right: false, centerX: 0};

const EDGES: Edge[] = ["top", "bottom", "left", "right"];

// The webview rounds scroll offsets, by more than a pixel when zoomed out.
const SNAP_TOLERANCE_PX = 4;

const DIRECTION: Record<Edge, string> = {
    top: "up",
    bottom: "down",
    left: "left",
    right: "right",
};

const TAB: Record<Edge, string> = {
    top: "top-0 w-8 h-3 -translate-x-1/2 rounded-b-md border-x border-b",
    bottom: "bottom-0 w-8 h-3 -translate-x-1/2 rounded-t-md border-x border-t",
    left: "left-0 top-1/2 w-3 h-8 -translate-y-1/2 rounded-r-md border-y border-r",
    right: "right-0 top-1/2 w-3 h-8 -translate-y-1/2 rounded-l-md border-y border-l",
};

const ARROW: Record<Edge, {viewBox: string; size: string; path: string}> = {
    top: {viewBox: "0 0 10 5", size: "w-[10px] h-[5px]", path: "M0 5 L5 0 L10 5 Z"},
    bottom: {viewBox: "0 0 10 5", size: "w-[10px] h-[5px]", path: "M0 0 L5 5 L10 0 Z"},
    left: {viewBox: "0 0 5 10", size: "w-[5px] h-[10px]", path: "M5 0 L0 5 L5 10 Z"},
    right: {viewBox: "0 0 5 10", size: "w-[5px] h-[10px]", path: "M0 0 L5 5 L0 10 Z"},
};

function scrollPosition(element: HTMLElement, horizontal: boolean, reversed: boolean): number {
    if (horizontal) return element.scrollLeft;
    // A reversed container starts scrolled to the bottom: scrollTop is zero there and negative above.
    return reversed ? -element.scrollTop : element.scrollTop;
}

function measure(element: HTMLElement, content: HTMLElement, reversed: boolean): Overflow {
    const fromStart = scrollPosition(element, false, reversed);
    const restAfterStart = element.scrollHeight - element.clientHeight - fromStart;
    const left = element.scrollLeft > SNAP_TOLERANCE_PX;
    const right =
        element.scrollWidth - element.clientWidth - element.scrollLeft > SNAP_TOLERANCE_PX;

    const children = Array.from(content.children) as HTMLElement[];
    const centerX =
        left || right || children.length === 0
            ? element.clientWidth / 2
            : (Math.min(...children.map(c => c.offsetLeft)) +
                  Math.max(...children.map(c => c.offsetLeft + c.offsetWidth))) /
              2;

    return {
        top: (reversed ? restAfterStart : fromStart) > SNAP_TOLERANCE_PX,
        bottom: (reversed ? fromStart : restAfterStart) > SNAP_TOLERANCE_PX,
        left,
        right,
        centerX: Math.round(centerX),
    };
}

/**
 * The scroll distance from the start at which the next column or row towards an edge lines up with
 * the start of the area (left, top, or bottom if reversed), counted from `from`.
 */
function snapTarget(
    scroller: HTMLElement,
    content: HTMLElement,
    edge: Edge,
    reversed: boolean,
    from?: number,
): number {
    const horizontal = edge === "left" || edge === "right";
    const position = scrollPosition(scroller, horizontal, reversed);
    const max = horizontal
        ? scroller.scrollWidth - scroller.clientWidth
        : scroller.scrollHeight - scroller.clientHeight;

    const item = content.firstElementChild as HTMLElement;
    const style = getComputedStyle(content);
    const gap = parseFloat(horizontal ? style.columnGap : style.rowGap) || 0;
    const itemStyle = getComputedStyle(item);
    // Not offsetWidth or offsetHeight: their rounding adds up over the columns and rows.
    const pitch = parseFloat(horizontal ? itemStyle.width : itemStyle.height) + gap;
    const current = from ?? position;

    const forward = edge === "right" || edge === (reversed ? "top" : "bottom");
    if (forward) {
        const next = (Math.floor((current + SNAP_TOLERANCE_PX) / pitch) + 1) * pitch;
        // The padding behind the last item is not worth a stop of its own.
        return next < max - gap ? next : max;
    }
    const before = Math.min(current - SNAP_TOLERANCE_PX, max - gap);
    return Math.max(0, (Math.ceil(before / pitch) - 1) * pitch);
}

function sameOverflow(a: Overflow, b: Overflow): boolean {
    return EDGES.every(edge => a[edge] === b[edge]) && a.centerX === b.centerX;
}

type ScrollAreaProps = {
    /** Classes for the element inside the scrolling one, which lays out the children. */
    className?: string;
    style?: CSSProperties;
    /** The content starts at the bottom. Lay the children out upwards (`flex-wrap-reverse`). */
    reversed?: boolean;
    onWheel?: (event: TargetedWheelEvent<HTMLDivElement>) => void;
    children?: ComponentChildren;
};

/**
 * Fills its parent with a scroll container without a scrollbar, and marks each edge behind which
 * content is hidden with a tab that scrolls to the next column or row towards it. The tabs expect
 * the children to be the items, all as large as the first one. Only the last one may be smaller.
 */
function ScrollArea({className, style, reversed = false, onWheel, children}: ScrollAreaProps) {
    const scrollRef = useRef<HTMLDivElement>(null);
    const contentRef = useRef<HTMLDivElement>(null);
    // Where a tab click is still scrolling to, so that the next click steps on from there.
    const pendingRef = useRef<{horizontal: boolean; target: number} | undefined>(undefined);
    const [overflow, setOverflow] = useState<Overflow>(NO_OVERFLOW);

    useEffect(() => {
        const element = scrollRef.current;
        const content = contentRef.current;
        if (element === null || content === null) return;

        const update = () => {
            const pending = pendingRef.current;
            if (
                pending !== undefined &&
                Math.abs(scrollPosition(element, pending.horizontal, reversed) - pending.target) <=
                    SNAP_TOLERANCE_PX
            ) {
                pendingRef.current = undefined;
            }
            const next = measure(element, content, reversed);
            setOverflow(prev => (sameOverflow(prev, next) ? prev : next));
        };

        const forgetPending = () => {
            pendingRef.current = undefined;
        };
        const resizeObserver = new ResizeObserver(update);
        const observeSizes = () => {
            resizeObserver.disconnect();
            resizeObserver.observe(element);
            resizeObserver.observe(content);
            for (const child of content.children) resizeObserver.observe(child);
        };
        observeSizes();
        const mutationObserver = new MutationObserver(() => {
            forgetPending();
            observeSizes();
            update();
        });
        mutationObserver.observe(content, {childList: true, subtree: true});
        element.addEventListener("scroll", update, {passive: true});
        // Scrolling by hand takes over from a tab click.
        element.addEventListener("wheel", forgetPending, {passive: true});
        element.addEventListener("pointerdown", forgetPending, {passive: true});
        update();

        return () => {
            resizeObserver.disconnect();
            mutationObserver.disconnect();
            element.removeEventListener("scroll", update);
            element.removeEventListener("wheel", forgetPending);
            element.removeEventListener("pointerdown", forgetPending);
        };
    }, [reversed]);

    const scrollTowards = (edge: Edge) => {
        const element = scrollRef.current;
        const content = contentRef.current;
        if (element === null || content === null) return;

        void invokeSafe("audio_play_ui_click");
        const horizontal = edge === "left" || edge === "right";
        const pending = pendingRef.current;
        const from = pending?.horizontal === horizontal ? pending.target : undefined;
        const target = snapTarget(element, content, edge, reversed, from);
        pendingRef.current = {horizontal, target};
        element.scrollTo({
            ...(horizontal ? {left: target} : {top: reversed ? -target : target}),
            behavior: "smooth",
        });
    };

    return (
        <div className="relative w-full h-full">
            <div
                ref={scrollRef}
                className={clsx(
                    "w-full h-full overflow-auto scrollbar-none",
                    reversed && "flex flex-col-reverse",
                )}
                onWheel={onWheel}
            >
                <div ref={contentRef} className={className} style={style}>
                    {children}
                </div>
            </div>
            {EDGES.map(edge => {
                const arrow = ARROW[edge];
                return (
                    <button
                        key={edge}
                        type="button"
                        aria-label={`Scroll ${DIRECTION[edge]}`}
                        data-overflow-edge={edge}
                        className={clsx(
                            "absolute z-[6] flex justify-center items-center bg-gray-700 border-gray-900 cursor-pointer transition-opacity duration-150",
                            "disabled:opacity-0 disabled:pointer-events-none",
                            TAB[edge],
                        )}
                        style={
                            edge === "top" || edge === "bottom"
                                ? {left: `${overflow.centerX}px`}
                                : undefined
                        }
                        disabled={!overflow[edge]}
                        onClick={() => scrollTowards(edge)}
                    >
                        <svg
                            viewBox={arrow.viewBox}
                            className={clsx("text-white", arrow.size)}
                            fill="currentColor"
                        >
                            <path d={arrow.path} />
                        </svg>
                    </button>
                );
            })}
        </div>
    );
}

export default ScrollArea;
