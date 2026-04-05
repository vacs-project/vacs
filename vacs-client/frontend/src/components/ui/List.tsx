import {clsx} from "clsx";
import {Fragment, JSX, TargetedMouseEvent, TargetedTouchEvent} from "preact";
import {useCallback, useEffect, useRef, useState} from "preact/hooks";
import {invokeSafe} from "../../error.ts";
import {useClickAndHold} from "../../hooks/click-and-hold-hook.ts";
import {HEADER_HEIGHT_REM, useList} from "../../hooks/list-hook.ts";

type ListProps = {
    itemsCount: number;
    selectedItem: number;
    setSelectedItem: (item: number) => void;
    defaultRows: number;
    row: (item: number, isSelected: boolean, onClick: () => void) => JSX.Element;
    header: {title: string; className?: string}[];
    columnWidths: string[];
    className?: string;
    enableKeyboardNavigation?: boolean;
};

function List(props: ListProps) {
    const {listContainer, scrollOffset, setScrollOffset, visibleItemIndices, maxScrollOffset} =
        useList(props);

    const gridCols = `${props.columnWidths.join(" ")} 4rem`;

    return (
        <div
            ref={listContainer}
            className={clsx(
                "h-full grid box-border gap-px [&>div]:outline-1 [&>div]:outline-gray-500",
                props.className,
            )}
            style={{
                gridTemplateRows: `${HEADER_HEIGHT_REM}rem repeat(${visibleItemIndices.length},1fr)`,
                gridTemplateColumns: gridCols,
            }}
        >
            {props.header.map((headerItem, idx) => (
                <div
                    key={idx}
                    className={clsx(
                        "bg-gray-300 flex justify-center items-center font-bold",
                        headerItem.className,
                    )}
                >
                    {headerItem.title}
                </div>
            ))}
            <div className="outline-0!"></div>

            {visibleItemIndices.map((itemIndex, idx) => {
                const rowSpan = visibleItemIndices.length - 2;
                const lastElement =
                    idx === 0 ? (
                        <ScrollButtonRow
                            direction="up"
                            enabled={scrollOffset > 0}
                            onClick={() =>
                                setScrollOffset(scrollOffset => Math.max(scrollOffset - 1, 0))
                            }
                        />
                    ) : idx === 1 ? (
                        <ScrollBar
                            rowSpan={rowSpan}
                            scrollOffset={scrollOffset}
                            maxScrollOffset={maxScrollOffset}
                            setScrollOffset={setScrollOffset}
                        />
                    ) : idx === visibleItemIndices.length - 1 ? (
                        <ScrollButtonRow
                            direction="down"
                            enabled={scrollOffset < maxScrollOffset}
                            onClick={() =>
                                setScrollOffset(scrollOffset =>
                                    Math.min(scrollOffset + 1, maxScrollOffset),
                                )
                            }
                        />
                    ) : (
                        <></>
                    );

                return (
                    <Fragment key={idx}>
                        {props.row(itemIndex, itemIndex === props.selectedItem, () => {
                            props.setSelectedItem(itemIndex);
                        })}
                        {lastElement}
                    </Fragment>
                );
            })}
        </div>
    );
}

function ScrollBar({
    rowSpan,
    scrollOffset,
    maxScrollOffset,
    setScrollOffset,
}: {
    rowSpan: number;
    scrollOffset: number;
    maxScrollOffset: number;
    setScrollOffset: (value: number | ((scrollOffset: number) => number)) => void;
}) {
    const [dragging, setDragging] = useState<boolean>(false);

    const isDraggingRef = useRef<boolean>(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const positionRef = useRef<number>(0);
    const maxScrollOffsetRef = useRef<number>(maxScrollOffset);
    const trackHoldTargetRef = useRef<number | null>(null);

    const scrollHandleVisible = maxScrollOffset > 0;
    const position = (1 / maxScrollOffset) * scrollOffset;

    useEffect(() => {
        positionRef.current = position;
        maxScrollOffsetRef.current = maxScrollOffset;
    }, [position, maxScrollOffset]);

    const getNormalizedScrollPosition = (clientY: number): number | null => {
        const container = containerRef.current;
        if (!container) return null;

        const rect = container.getBoundingClientRect();
        const usableHeight = rect.height - 28 * 2;

        let newY = clientY - rect.top - 28;
        newY = Math.max(0, Math.min(newY, usableHeight));

        return newY / usableHeight;
    };

    const updateOffsetFromClientY = useCallback(
        (clientY: number) => {
            const newPos = getNormalizedScrollPosition(clientY);
            if (newPos === null) return;

            const maxOffset = maxScrollOffsetRef.current;
            if (maxOffset <= 0) return;

            const stepSize = 1 / maxOffset;

            const posDelta = newPos - positionRef.current;
            const absPosDelta = Math.abs(posDelta);

            if (absPosDelta < stepSize / 2) return;

            const steps = Math.sign(posDelta) * Math.floor((absPosDelta + stepSize / 2) / stepSize);

            setScrollOffset(prev => {
                const nextOffset = Math.max(0, Math.min(prev + steps, maxOffset));
                positionRef.current = nextOffset / maxOffset;
                return nextOffset;
            });
        },
        [setScrollOffset],
    );

    const stepOffsetTowardTarget = useCallback(
        (targetOffset: number, fallbackDirection: -1 | 0 | 1 = 0) => {
            setScrollOffset(prev => {
                const maxOffset = maxScrollOffsetRef.current;
                if (maxOffset <= 0) return prev;

                const clampedTarget = Math.max(0, Math.min(targetOffset, maxOffset));

                let nextOffset = prev;
                if (prev < clampedTarget) {
                    nextOffset = prev + 1;
                } else if (prev > clampedTarget) {
                    nextOffset = prev - 1;
                } else if (fallbackDirection !== 0) {
                    nextOffset = Math.max(0, Math.min(prev + fallbackDirection, maxOffset));
                }

                if (nextOffset === prev) return prev;

                positionRef.current = nextOffset / maxOffset;
                return nextOffset;
            });
        },
        [setScrollOffset],
    );

    const {
        startHold: startTrackHold,
        stopHold: handleScrollBarMouseUp,
        isHoldingRef: isTrackHoldingRef,
    } = useClickAndHold({
        enabled: maxScrollOffset > 0,
        onHoldTick: () => {
            const targetOffset = trackHoldTargetRef.current;
            if (targetOffset === null) return;
            stepOffsetTowardTarget(targetOffset);
        },
        onStop: () => {
            trackHoldTargetRef.current = null;
        },
    });

    const startScrollBarHold = (clientY: number) => {
        const maxOffset = maxScrollOffsetRef.current;
        if (maxOffset <= 0) return;

        const newPos = getNormalizedScrollPosition(clientY);
        if (newPos === null) return;

        const targetOffset = Math.round(newPos * maxOffset);
        const fallbackDirection: -1 | 1 = newPos > positionRef.current ? 1 : -1;

        trackHoldTargetRef.current = targetOffset;
        stepOffsetTowardTarget(targetOffset, fallbackDirection);
        startTrackHold();
    };

    const handleScrollBarMouseDown = (event: TargetedMouseEvent<HTMLDivElement>) => {
        if (event.button !== 0) return;
        startScrollBarHold(event.clientY);
    };

    const handleScrollBarTouchStart = (event: TargetedTouchEvent<HTMLDivElement>) => {
        const touch = event.touches[0];
        if (!touch) return;
        event.preventDefault();
        startScrollBarHold(touch.clientY);
    };

    const startScrollHandleDrag = () => {
        isDraggingRef.current = true;
        setDragging(true);
    };

    const handleScrollHandleMouseDown = (event: TargetedMouseEvent<HTMLDivElement>) => {
        event.stopPropagation();
        if (event.button !== 0) return;
        startScrollHandleDrag();
    };

    const handleScrollHandleTouchStart = (event: TargetedTouchEvent<HTMLDivElement>) => {
        event.stopPropagation();
        event.preventDefault();
        startScrollHandleDrag();
    };

    useEffect(() => {
        const handleWindowMove = (clientY: number) => {
            if (isDraggingRef.current) {
                updateOffsetFromClientY(clientY);
            }

            if (!isTrackHoldingRef.current) return;

            const maxOffset = maxScrollOffsetRef.current;
            if (maxOffset <= 0) return;

            const newPos = getNormalizedScrollPosition(clientY);
            if (newPos === null) return;

            trackHoldTargetRef.current = Math.round(newPos * maxOffset);
        };

        const handleWindowMouseMove = (event: MouseEvent) => {
            handleWindowMove(event.clientY);
        };

        const handleWindowTouchMove = (event: TouchEvent) => {
            const touch = event.touches[0];
            if (!touch) return;
            if (isDraggingRef.current) event.preventDefault();
            handleWindowMove(touch.clientY);
        };

        const handleWindowUp = () => {
            isDraggingRef.current = false;
            setDragging(false);
        };

        window.addEventListener("mouseup", handleWindowUp);
        window.addEventListener("mousemove", handleWindowMouseMove);
        window.addEventListener("touchmove", handleWindowTouchMove, {passive: false});
        window.addEventListener("touchend", handleWindowUp);
        window.addEventListener("touchcancel", handleWindowUp);

        return () => {
            window.removeEventListener("mouseup", handleWindowUp);
            window.removeEventListener("mousemove", handleWindowMouseMove);
            window.removeEventListener("touchmove", handleWindowTouchMove);
            window.removeEventListener("touchend", handleWindowUp);
            window.removeEventListener("touchcancel", handleWindowUp);
        };
    }, [isTrackHoldingRef, updateOffsetFromClientY]);

    return (
        <div className="bg-gray-300" style={{gridRow: `span ${rowSpan} / span ${rowSpan}`}}>
            <div
                onMouseDown={handleScrollBarMouseDown}
                onTouchStart={handleScrollBarTouchStart}
                onMouseUp={handleScrollBarMouseUp}
                onContextMenu={e => e.preventDefault()}
                ref={containerRef}
                className="relative h-full w-full px-4 py-7"
            >
                <div className="h-full w-full border border-b-gray-100 border-r-gray-100 border-l-gray-700 border-t-gray-700 flex flex-col-reverse"></div>
                {scrollHandleVisible && (
                    <div
                        onClick={e => e.stopPropagation()}
                        onMouseDown={handleScrollHandleMouseDown}
                        onTouchStart={handleScrollHandleTouchStart}
                        className={clsx(
                            "dotted-background-gray absolute translate-y-[-50%] left-0 w-full h-13 rounded-md cursor-pointer bg-gray-300 border",
                            !dragging &&
                                "border-t-white border-l-white border-r-gray-900 border-b-gray-900",
                            dragging &&
                                "border-b-white border-r-white border-l-gray-900 border-t-gray-900 shadow-none",
                        )}
                        style={{
                            top: `calc(1.625rem + ${position} * (100% - 3.25rem))`,
                        }}
                    ></div>
                )}
            </div>
        </div>
    );
}

function ScrollButtonRow({
    direction,
    enabled,
    onClick,
}: {
    direction: "up" | "down";
    enabled: boolean;
    onClick: () => void;
}) {
    const {startHold, stopHold: handleOnMouseUp} = useClickAndHold({
        enabled,
        onHoldTick: onClick,
    });

    const handleDown = useCallback(() => {
        void invokeSafe("audio_play_ui_click");
        onClick();
        startHold();
    }, [startHold, onClick]);

    const handleOnMouseDown = useCallback(
        (event: TargetedMouseEvent<HTMLDivElement>) => {
            if (event.button !== 0) return;
            handleDown();
        },
        [handleDown],
    );

    const handleOnTouchStart = useCallback(
        (event: TargetedTouchEvent<HTMLDivElement>) => {
            event.preventDefault();
            handleDown();
        },
        [handleDown],
    );

    return (
        <div
            className="relative bg-gray-300"
            style={{cursor: enabled ? "pointer" : "not-allowed"}}
            onMouseDown={enabled ? handleOnMouseDown : undefined}
            onTouchStart={enabled ? handleOnTouchStart : undefined}
            onMouseUp={handleOnMouseUp}
            onContextMenu={e => e.preventDefault()}
        >
            <svg
                className={clsx(
                    "absolute h-[85%] max-w-[85%] top-1/2 -translate-y-1/2 left-1/2 -translate-x-1/2",
                    direction === "down" && "rotate-180",
                )}
                viewBox="0 0 125 89"
                fill="none"
                xmlns="http://www.w3.org/2000/svg"
            >
                <path d="M62.5 0L120 60H5L62.5 0Z" fill={enabled ? "black" : "#6A7282"} />
                <path
                    d="M63.2217 26.3076L120.722 86.3076L122.344 88H2.65625L4.27832 86.3076L61.7783 26.3076L62.5 25.5547L63.2217 26.3076Z"
                    fill={enabled ? "black" : "#6A7282"}
                    stroke="#D1D5DC"
                    strokeWidth="2"
                />
            </svg>
        </div>
    );
}

export default List;
