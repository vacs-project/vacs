import clsx from "clsx";
import {ComponentChildren} from "preact";
import {ForwardedRef, forwardRef} from "preact/compat";

type MainPageContainerProps = {
    children: ComponentChildren;
    width?: string;
    className?: string;
};

const MainPageContainer = forwardRef(
    (props: MainPageContainerProps, ref: ForwardedRef<HTMLDivElement>) => {
        return (
            <div
                ref={ref}
                className={clsx(
                    "relative h-full shrink-0 bg-[#B5BBC6] border-l border-t border-r-2 border-b-2 border-gray-700 rounded-sm flex flex-row",
                    props.width === undefined && "flex-1 min-w-0",
                    props.className,
                )}
                style={{width: props.width}}
            >
                {props.children}
            </div>
        );
    },
);

export default MainPageContainer;
