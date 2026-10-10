import {ComponentChildren} from "preact";
import {useEffect, useRef, useState} from "preact/hooks";

type HintProps = {
    children: ComponentChildren;
    maxWidth?: number;
};

type Position = {
    top: number;
    left: number;
};

function Hint({children, maxWidth = 280}: HintProps) {
    const [visible, setVisible] = useState(false);
    const [position, setPosition] = useState<Position>({top: 0, left: 0});
    const wrapperRef = useRef<HTMLDivElement>(null);
    const tooltipRef = useRef<HTMLDivElement>(null);

    const show = () => {
        if (!wrapperRef.current) return;
        const rect = wrapperRef.current.getBoundingClientRect();
        const estimatedHeight = 80;
        const centerX = rect.left + rect.width / 2;
        const top =
            rect.bottom + estimatedHeight > window.innerHeight - 8
                ? rect.top - estimatedHeight - 8
                : rect.bottom + 8;
        const left = Math.min(Math.max(centerX, 8), window.innerWidth - maxWidth - 8);
        setPosition({top, left});
        setVisible(true);
    };

    const hide = () => setVisible(false);

    const toggle = () => (visible ? hide() : show());

    const handleTouchStart = (e: TouchEvent) => {
        e.preventDefault();
        toggle();
    };

    useEffect(() => {
        if (!visible) return;
        const handleOutside = (e: MouseEvent | TouchEvent) => {
            const target = e.target as Node;
            if (!wrapperRef.current?.contains(target) && !tooltipRef.current?.contains(target)) {
                hide();
            }
        };
        document.addEventListener("mousedown", handleOutside);
        document.addEventListener("touchstart", handleOutside);
        return () => {
            document.removeEventListener("mousedown", handleOutside);
            document.removeEventListener("touchstart", handleOutside);
        };
    }, [visible]);

    return (
        <>
            <div
                ref={wrapperRef}
                className="inline-flex p-1 cursor-help"
                onMouseEnter={show}
                onMouseLeave={hide}
                onTouchStart={handleTouchStart}
            >
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    className="stroke-gray-600"
                >
                    <circle cx="12" cy="12" r="10" />
                    <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
                    <path d="M12 17h.01" />
                </svg>
            </div>
            {visible && (
                <div
                    ref={tooltipRef}
                    style={{
                        position: "fixed",
                        top: `${position.top}px`,
                        left: `${position.left}px`,
                        transform: "translateX(-50%)",
                        maxWidth: `${maxWidth}px`,
                        zIndex: 100,
                    }}
                    className="bg-gray-300 border-2 border-t-gray-200 border-l-gray-200 border-b-gray-500 border-r-gray-500 rounded px-2 py-1.5 text-xs leading-relaxed"
                >
                    {children}
                </div>
            )}
        </>
    );
}

export default Hint;
