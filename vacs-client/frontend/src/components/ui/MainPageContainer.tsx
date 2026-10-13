import {clsx} from "clsx";
import {ComponentChildren, Ref} from "preact";

type MainPageContainerProps = {
    children: ComponentChildren;
    width?: string;
    className?: string;
    ref?: Ref<HTMLDivElement>;
};

const MainPageContainer = ({ref, ...props}: MainPageContainerProps) => {
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
};

export default MainPageContainer;
