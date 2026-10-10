import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen} from "@testing-library/preact";

import Hint from "../../src/components/Hint.tsx";

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
});

describe("Hint", () => {
    it("positions the tooltip in pixels", () => {
        vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
            left: 100,
            width: 20,
            top: 50,
            bottom: 60,
        } as DOMRect);

        const {container} = render(<Hint maxWidth={300}>tooltip text</Hint>);
        fireEvent.mouseEnter(container.firstElementChild!);

        const tooltip = screen.getByText("tooltip text");
        expect(tooltip.style.top).toBe("68px");
        expect(tooltip.style.left).toBe("110px");
        expect(tooltip.style.maxWidth).toBe("300px");
    });
});
