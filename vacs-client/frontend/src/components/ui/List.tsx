import {clsx} from "clsx";
import {Fragment, JSX, TargetedMouseEvent} from "preact";
import {useEffect, useRef, useState} from "preact/hooks";
import {invokeSafe} from "../../error.ts";
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
            {/*HEADER*/}
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
                            disabled={scrollOffset <= 0}
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
                            disabled={scrollOffset >= maxScrollOffset}
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
    const scrollHandleVisible = maxScrollOffset > 0;
    const position = (1 / maxScrollOffset) * scrollOffset;

    const getNormalizedScrollPosition = (clientY: number): number | null => {
        const container = containerRef.current;
        if (!container) return null;

        const rect = container.getBoundingClientRect();
        const usableHeight = rect.height - 28 * 2;

        let newY = clientY - rect.top - 28;
        newY = Math.max(0, Math.min(newY, usableHeight));

        return newY / usableHeight;
    };

    const updatePositionFromClientY = (clientY: number) => {
        const newPos = getNormalizedScrollPosition(clientY);
        if (newPos === null) return;

        const stepSize = 1 / maxScrollOffset;

        if (newPos >= position + stepSize / 2) {
            setScrollOffset(Math.min(scrollOffset + 1, maxScrollOffset));
        } else if (newPos <= position - stepSize / 2) {
            setScrollOffset(Math.max(scrollOffset - 1, 0));
        }
    };

    const handleScrollBarMouseDown = (event: TargetedMouseEvent<HTMLDivElement>) => {
        const newPos = getNormalizedScrollPosition(event.clientY);
        if (newPos === null) return;

        if (newPos > position) {
            setScrollOffset(scrollOffset => Math.min(scrollOffset + 1, maxScrollOffset));
        } else {
            setScrollOffset(scrollOffset => Math.max(scrollOffset - 1, 0));
        }
    };

    const handleScrollHandleMouseDown = (
        event: MouseEvent | TargetedMouseEvent<HTMLDivElement>,
    ) => {
        event.stopPropagation();
        if (event.button !== 0) return;
        isDraggingRef.current = true;
        setDragging(true);
    };

    const handleWindowMouseMove = (event: MouseEvent) => {
        if (!isDraggingRef.current) return;
        updatePositionFromClientY(event.clientY);
    };

    const handleWindowMouseUp = () => {
        isDraggingRef.current = false;
        setDragging(false);
    };

    useEffect(() => {
        window.addEventListener("mouseup", handleWindowMouseUp);
        window.addEventListener("mousemove", handleWindowMouseMove);
        return () => {
            window.removeEventListener("mouseup", handleWindowMouseUp);
            window.removeEventListener("mousemove", handleWindowMouseMove);
        };
    });

    return (
        <div className="bg-gray-300" style={{gridRow: `span ${rowSpan} / span ${rowSpan}`}}>
            <div
                onMouseDown={handleScrollBarMouseDown}
                ref={containerRef}
                className="relative h-full w-full px-4 py-7"
            >
                <div className="h-full w-full border border-b-gray-100 border-r-gray-100 border-l-gray-700 border-t-gray-700 flex flex-col-reverse"></div>
                {scrollHandleVisible && (
                    <div
                        onClick={e => e.stopPropagation()}
                        onMouseDown={handleScrollHandleMouseDown}
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
    disabled,
    onClick,
}: {
    direction: "up" | "down";
    disabled: boolean;
    onClick: () => void;
}) {
    const timeoutRef = useRef<number | undefined>(undefined);
    const intervalRef = useRef<number | undefined>(undefined);

    const handleOnMouseDown = () => {
        timeoutRef.current = setTimeout(() => {
            intervalRef.current = setInterval(() => {
                if (!disabled) onClick();
            }, 75);
            timeoutRef.current = undefined;
        }, 200);
    };

    const handleOnMouseUp = () => {
        if (timeoutRef.current !== undefined) {
            clearTimeout(timeoutRef.current);
            timeoutRef.current = undefined;

            void invokeSafe("audio_play_ui_click");
            onClick();
        }
        if (intervalRef.current !== undefined) {
            clearInterval(intervalRef.current);
            intervalRef.current = undefined;
        }
    };

    useEffect(() => {
        window.addEventListener("mouseup", handleOnMouseUp);

        return () => window.removeEventListener("mouseup", handleOnMouseUp);
    }, []);

    return (
        <div
            className="relative bg-gray-300"
            style={{cursor: disabled ? "not-allowed" : "pointer"}}
            onMouseDown={!disabled ? handleOnMouseDown : undefined}
            onMouseUp={handleOnMouseUp}
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
                <path d="M62.5 0L120 60H5L62.5 0Z" fill={disabled ? "#6A7282" : "black"} />
                <path
                    d="M63.2217 26.3076L120.722 86.3076L122.344 88H2.65625L4.27832 86.3076L61.7783 26.3076L62.5 25.5547L63.2217 26.3076Z"
                    fill={disabled ? "#6A7282" : "black"}
                    stroke="#D1D5DC"
                    strokeWidth="2"
                />
            </svg>
        </div>
    );
}

export default List;
