import {clsx} from "clsx";
import {ForwardedRef, forwardRef} from "preact/compat";
import {invokeSafe} from "../../error.ts";

type SelectionFieldProps = {
    label: string;
    title?: string;
    active?: boolean;
    disabled?: boolean;
    removeDisabled?: boolean;
    className?: string;
    onClick: () => void;
    onRemove: () => void;
};

const SelectionField = forwardRef(
    (props: SelectionFieldProps, ref: ForwardedRef<HTMLDivElement>) => {
        const handleOnClick = () => {
            if (props.disabled) return;
            void invokeSafe("audio_play_ui_click");
            props.onClick();
        };

        const handleOnRemoveClick = () => {
            if (props.removeDisabled) return;
            void invokeSafe("audio_play_ui_click");
            props.onRemove();
        };

        return (
            <div className="grow h-full min-w-0 flex flex-row items-center justify-center">
                <div
                    ref={ref}
                    onClick={handleOnClick}
                    title={props.title}
                    className={clsx(
                        "w-full h-full min-w-10 min-h-8 grow text-sm py-1 px-2 rounded text-center flex items-center justify-center",
                        "bg-gray-300 border-2",
                        props.active
                            ? "border-r-gray-100 border-b-gray-100 border-t-gray-700 border-l-gray-700 *:translate-y-px *:translate-x-px"
                            : "border-t-gray-100 border-l-gray-100 border-r-gray-700 border-b-gray-700",
                        props.disabled ? "brightness-90 cursor-not-allowed" : "cursor-pointer",
                        props.className,
                    )}
                >
                    <p className="truncate max-w-full">{props.label}</p>
                </div>
                <svg
                    onClick={handleOnRemoveClick}
                    xmlns="http://www.w3.org/2000/svg"
                    width="27"
                    height="27"
                    viewBox="0 0 24 24"
                    fill="none"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    className={clsx(
                        "shrink-0 p-1 pr-0!",
                        props.removeDisabled
                            ? "stroke-gray-500 cursor-not-allowed"
                            : "stroke-gray-700 hover:stroke-red-500 transition-colors cursor-pointer",
                    )}
                >
                    <path d="M18 6 6 18" />
                    <path d="m6 6 12 12" />
                </svg>
            </div>
        );
    },
);

export default SelectionField;
