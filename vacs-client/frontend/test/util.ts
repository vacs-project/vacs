import {act} from "@testing-library/preact";
import {useCallStore} from "../src/stores/call-store.ts";

export function flipBlink() {
    return act(() => {
        useCallStore.setState(s => ({blink: !s.blink}));
    });
}
