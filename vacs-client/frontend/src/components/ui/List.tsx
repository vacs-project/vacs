import {Fragment, JSX} from "preact";
import {clsx} from "clsx";
import {HEADER_HEIGHT_REM, useList} from "../../hooks/list-hook.ts";
import {useEffect, useRef} from "preact/hooks";

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
                        <div
                            className="bg-gray-300"
                            style={{gridRow: `span ${rowSpan} / span ${rowSpan}`}}
                        >
                            <div className="relative h-full w-full px-4 py-7">
                                <div className="h-full w-full border border-b-gray-100 border-r-gray-100 border-l-gray-700 border-t-gray-700 flex flex-col-reverse"></div>
                                {/*<div*/}
                                {/*    className={clsx(*/}
                                {/*        "dotted-background absolute translate-y-[-50%] left-0 w-full aspect-square shadow-[0_0_0_1px_#364153] rounded-md cursor-pointer bg-blue-600 border",*/}
                                {/*        true && "border-t-blue-200 border-l-blue-200 border-r-blue-900 border-b-blue-900",*/}
                                {/*        false && "border-b-blue-200 border-r-blue-200 border-l-blue-900 border-t-blue-900 shadow-none",*/}
                                {/*    )}*/}
                                {/*    style={{top: `calc(2.25rem + (1 - ${1}) * (100% - 4.5rem))`}}>*/}
                                {/*</div>*/}
                            </div>
                        </div>
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
