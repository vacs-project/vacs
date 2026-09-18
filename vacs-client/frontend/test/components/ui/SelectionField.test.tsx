import {afterEach, describe, expect, it, vi} from "vitest";
import {cleanup, fireEvent, render, screen} from "@testing-library/preact";
import {createRef} from "preact";

const {invoke, listen} = vi.hoisted(() => ({
    invoke: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(() =>
        Promise.resolve(undefined),
    ),
    listen: vi.fn<() => Promise<() => void>>(() => Promise.resolve(() => {})),
}));

vi.mock("../../../src/transport", () => ({invoke, listen, isTauri: true, isRemote: () => false}));

import SelectionField from "../../../src/components/ui/SelectionField.tsx";

afterEach(() => {
    cleanup();
    vi.clearAllMocks();
});

const clicks = () => invoke.mock.calls.filter(([cmd]) => cmd === "audio_play_ui_click").length;
const field = () => screen.getByText("F1").parentElement!;
const remove = () => field().nextElementSibling!;

describe("SelectionField", () => {
    it("plays the click sound and calls the handlers for both halves", () => {
        const onClick = vi.fn<() => void>();
        const onRemove = vi.fn<() => void>();
        render(<SelectionField label="F1" onClick={onClick} onRemove={onRemove} />);

        fireEvent.click(field());
        expect(onClick).toHaveBeenCalledTimes(1);
        expect(clicks()).toBe(1);

        fireEvent.click(remove());
        expect(onRemove).toHaveBeenCalledTimes(1);
        expect(clicks()).toBe(2);
    });

    it("stays silent and inert when disabled", () => {
        const onClick = vi.fn<() => void>();
        const onRemove = vi.fn<() => void>();
        render(
            <SelectionField
                label="F1"
                disabled
                removeDisabled
                onClick={onClick}
                onRemove={onRemove}
            />,
        );

        fireEvent.click(field());
        fireEvent.click(remove());
        expect(onClick).not.toHaveBeenCalled();
        expect(onRemove).not.toHaveBeenCalled();
        expect(clicks()).toBe(0);
        expect(field().className).toContain("cursor-not-allowed");
        expect(remove().getAttribute("class")).toContain("cursor-not-allowed");
    });

    it("keeps the remove half live while only the field is disabled", () => {
        const onClick = vi.fn<() => void>();
        const onRemove = vi.fn<() => void>();
        render(<SelectionField label="F1" disabled onClick={onClick} onRemove={onRemove} />);

        fireEvent.click(field());
        fireEvent.click(remove());
        expect(onClick).not.toHaveBeenCalled();
        expect(onRemove).toHaveBeenCalledTimes(1);
        expect(clicks()).toBe(1);
        expect(remove().getAttribute("class")).toContain("cursor-pointer");
    });

    it("reflects the active state, title, class and forwarded ref on the field", () => {
        const ref = createRef<HTMLDivElement>();
        render(
            <SelectionField
                ref={ref}
                label="F1"
                title="Function key 1"
                active
                className="text-red-700"
                onClick={() => {}}
                onRemove={() => {}}
            />,
        );

        expect(ref.current).toBe(field());
        expect(field().getAttribute("title")).toBe("Function key 1");
        expect(field().className).toContain("text-red-700");
        expect(field().className).toContain("border-t-gray-700");
    });
});
