import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, render, screen} from "@testing-library/preact";
import {createRef} from "preact";
import MainPageContainer from "../../../src/components/ui/MainPageContainer.tsx";

afterEach(cleanup);

describe("MainPageContainer", () => {
    it("forwards an object ref to the root div", () => {
        const ref = createRef<HTMLDivElement>();
        render(<MainPageContainer ref={ref}>content</MainPageContainer>);

        expect(ref.current).toBe(screen.getByText("content"));
    });

    it("calls a callback ref with the root div and runs its cleanup on unmount", () => {
        const cleanupRef = vi.fn<() => void>();
        const callback = vi.fn<(dom: HTMLDivElement | null) => () => void>(() => cleanupRef);
        const {unmount} = render(<MainPageContainer ref={callback}>content</MainPageContainer>);

        expect(callback).toHaveBeenCalledWith(screen.getByText("content"));
        expect(cleanupRef).not.toHaveBeenCalled();

        unmount();
        expect(cleanupRef).toHaveBeenCalledTimes(1);
    });

    it("applies the explicit width instead of flex-1", () => {
        render(<MainPageContainer width="300px">content</MainPageContainer>);

        const root = screen.getByText("content");
        expect(root.style.width).toBe("300px");
        expect(root.className).not.toContain("flex-1");
    });
});
