import {useErrorOverlayStore} from "../../stores/error-overlay-store.ts";
import {clsx} from "clsx";

function ErrorOverlay() {
    const visible = useErrorOverlayStore(state => state.visible);
    const title = useErrorOverlayStore(state => state.title);
    const detail = useErrorOverlayStore(state => state.detail);
    const isNonCritical = useErrorOverlayStore(state => state.isNonCritical);
    const dismissable = useErrorOverlayStore(state => state.dismissable);

    const close = useErrorOverlayStore(state => state.close);

    const handleClick = () => {
        if (dismissable) close();
    };

    return visible ? (
        <div
            className="z-50 absolute top-0 left-0 w-full h-full flex justify-center items-center bg-[rgba(0,0,0,0.5)]"
            onClick={handleClick}
        >
            <div
                className={clsx(
                    "bg-gray-300 border-4 rounded w-100 py-2",
                    isNonCritical
                        ? "border-t-yellow-400 border-l-yellow-400 border-b-yellow-500 border-r-yellow-500"
                        : "border-t-red-500 border-l-red-500 border-b-red-700 border-r-red-700",
                )}
            >
                <p className="w-full text-center text-lg font-semibold wrap-break-word">{title}</p>
                <p className="w-full text-center wrap-break-word">{detail}</p>
            </div>
        </div>
    ) : (
        <></>
    );
}

export default ErrorOverlay;
