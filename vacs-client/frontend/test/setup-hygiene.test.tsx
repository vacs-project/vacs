import {describe, expect, it, vi} from "vitest";
import {cleanup, render} from "@testing-library/preact";
import {useEffect} from "preact/hooks";

describe("test setup", () => {
    it("runs unmount cleanups before cleanup returns", () => {
        const onCleanup = vi.fn<() => void>();
        function Component() {
            useEffect(() => onCleanup, []);
            return null;
        }
        render(<Component />);

        cleanup();

        expect(onCleanup).toHaveBeenCalledTimes(1);
    });
});
