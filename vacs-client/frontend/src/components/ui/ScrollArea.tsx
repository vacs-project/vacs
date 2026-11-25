import {clsx} from "clsx";
import {ComponentChildren, TargetedWheelEvent} from "preact";
import {useEffect, useRef, useState} from "preact/hooks";

type Edge = "top" | "bottom" | "left" | "right";

type Overflow = Record<Edge, boolean> & {
    /** Horizontal center of the content in px, or of the area while it scrolls sideways. */
    centerX: number;
};

const NO_OVERFLOW: Overflow = {top: false, bottom: false, left: false, right: false, centerX: 0};

const EDGES: Edge[] = ["top", "bottom", "left", "right"];

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

function measure(element: HTMLElement, reversed: boolean): Overflow {
    // A reversed container starts scrolled to the bottom: scrollTop is zero there and negative above.
    const fromStart = reversed ? -element.scrollTop : element.scrollTop;
    const restAfterStart = element.scrollHeight - element.clientHeight - fromStart;
    const left = element.scrollLeft > 1;
    const right = element.scrollWidth - element.clientWidth - element.scrollLeft > 1;

    const children = Array.from(element.children) as HTMLElement[];
    const centerX =
        left || right || children.length === 0
            ? element.clientWidth / 2
            : (Math.min(...children.map(c => c.offsetLeft)) +
                  Math.max(...children.map(c => c.offsetLeft + c.offsetWidth))) /
              2;

    return {
        top: reversed ? restAfterStart > 1 : fromStart > 1,
        bottom: reversed ? fromStart > 1 : restAfterStart > 1,
        left,
        right,
        centerX: Math.round(centerX),
    };
}

function sameOverflow(a: Overflow, b: Overflow): boolean {
    return EDGES.every(edge => a[edge] === b[edge]) && a.centerX === b.centerX;
}

type ScrollAreaProps = {
    /** Classes for the scrolling element, which lays out the children. */
    className?: string;
    /** The children are laid out from the bottom up (`flex-wrap-reverse`). */
    reversed?: boolean;
    onWheel?: (event: TargetedWheelEvent<HTMLDivElement>) => void;
    children?: ComponentChildren;
};

/**
 * Fills its parent with a scroll container without a scrollbar, and marks each edge behind which
 * content is hidden with a tab.
 */
function ScrollArea({className, reversed = false, onWheel, children}: ScrollAreaProps) {
    const scrollRef = useRef<HTMLDivElement>(null);
    const [overflow, setOverflow] = useState<Overflow>(NO_OVERFLOW);

    useEffect(() => {
        const element = scrollRef.current;
        if (element === null) return;

        const update = () => {
            const next = measure(element, reversed);
            setOverflow(prev => (sameOverflow(prev, next) ? prev : next));
        };

        const resizeObserver = new ResizeObserver(update);
        const observeSizes = () => {
            resizeObserver.disconnect();
            resizeObserver.observe(element);
            for (const child of element.children) resizeObserver.observe(child);
        };
        observeSizes();
        const mutationObserver = new MutationObserver(() => {
            observeSizes();
            update();
        });
        mutationObserver.observe(element, {childList: true, subtree: true});
        element.addEventListener("scroll", update, {passive: true});
        update();

        return () => {
            resizeObserver.disconnect();
            mutationObserver.disconnect();
            element.removeEventListener("scroll", update);
        };
    }, [reversed]);

    return (
        <div className="relative w-full h-full">
            <div
                ref={scrollRef}
                className={clsx("w-full h-full overflow-auto scrollbar-none", className)}
                onWheel={onWheel}
            >
                {children}
            </div>
            {EDGES.map(edge => {
                const arrow = ARROW[edge];
                const visibility = overflow[edge] ? "opacity-100" : "opacity-0";
                return (
                    <div
                        key={edge}
                        data-overflow-edge={edge}
                        className={clsx(
                            "pointer-events-none absolute z-[4] flex justify-center items-center bg-gray-700 border-gray-900 transition-opacity duration-150",
                            TAB[edge],
                            visibility,
                        )}
                        style={
                            edge === "top" || edge === "bottom"
                                ? {left: `${overflow.centerX}px`}
                                : undefined
                        }
                    >
                        <svg
                            viewBox={arrow.viewBox}
                            className={clsx("text-white", arrow.size)}
                            fill="currentColor"
                        >
                            <path d={arrow.path} />
                        </svg>
                    </div>
                );
            })}
        </div>
    );
}

export default ScrollArea;
