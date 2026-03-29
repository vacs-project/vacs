import {describe, expect, it, afterEach} from "vitest";
import {render, screen, cleanup} from "@testing-library/preact";
import PhoneButton from "../../../src/components/ui/PhoneButton.tsx";
import {
    ButtonColor,
    ButtonColors,
    ButtonHighlightColor,
    ButtonHighlightColors,
} from "../../../src/components/ui/Button.tsx";
import {useCallStore} from "../../../src/stores/call-store.ts";
import {CallId} from "../../../src/types/generic.ts";
import {flipBlink, makeTestCall} from "../../util.ts";

afterEach(() => {
    useCallStore.getState().actions.reset();
    cleanup();
});

describe("PhoneButton", () => {
    it("renders gray when idle", () => {
        render(<PhoneButton />);
        expectColorWithoutHighlight("gray");
    });

    describe("outgoing call", () => {
        it("renders gray with green highlight", () => {
            useCallStore.setState({
                callDisplay: {type: "outgoing", call: makeTestCall()},
            });
            render(<PhoneButton />);
            expectColorAndHighlight("gray", "green");
        });

        it("blinks between yellow with green highlight and gray with green highlight for priority call", async () => {
            useCallStore.setState({
                callDisplay: {type: "outgoing", call: makeTestCall({prio: true})},
                blink: true,
            });
            render(<PhoneButton />);
            expectColorAndHighlight("yellow", "green");

            await flipBlink();
            expectColorAndHighlight("gray", "green");

            await flipBlink();
            expectColorAndHighlight("yellow", "green");
        });

        it("shows outgoing state when both outgoing and incoming calls exist", async () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "outgoing", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectColorAndHighlight("gray", "green");

            await flipBlink();
            expectColorAndHighlight("gray", "green");

            await flipBlink();
            expectColorAndHighlight("gray", "green");
        });

        it("ignores incoming prio when outgoing call exists", async () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "outgoing", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId, prio: true})],
            });
            render(<PhoneButton />);
            expectColorAndHighlight("gray", "green");

            await flipBlink();
            expectColorAndHighlight("gray", "green");

            await flipBlink();
            expectColorAndHighlight("gray", "green");
        });

        it("shows rejected state when both rejected and incoming calls exist", async () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "rejected", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectColorAndHighlight("green", "green");

            await flipBlink();
            expectColorAndHighlight("gray", "green");

            await flipBlink();
            expectColorAndHighlight("green", "green");
        });

        it("shows error state when both error and incoming calls exist", async () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "error", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectColorWithoutHighlight("red");

            await flipBlink();
            expectColorWithoutHighlight("gray");

            await flipBlink();
            expectColorWithoutHighlight("red");
        });

        it("shows accepted state when both accepted and incoming calls exist", async () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "accepted", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectColorWithoutHighlight("green");

            await flipBlink();
            expectColorWithoutHighlight("green");

            await flipBlink();
            expectColorWithoutHighlight("green");
        });
    });

    describe("incoming call", () => {
        it("blinks between green and gray for incoming call", async () => {
            useCallStore.setState({
                blink: true,
                incomingCalls: [makeTestCall()],
            });
            render(<PhoneButton />);
            expectColorWithoutHighlight("green");

            await flipBlink();
            expectColorWithoutHighlight("gray");

            await flipBlink();
            expectColorWithoutHighlight("green");
        });

        it("blinks between yellow with green highlight and gray for priority call", async () => {
            useCallStore.setState({
                blink: true,
                incomingCalls: [makeTestCall({prio: true})],
            });
            render(<PhoneButton />);
            expectColorAndHighlight("yellow", "green");

            await flipBlink();
            expectColorAndHighlight("gray", "gray");

            await flipBlink();
            expectColorAndHighlight("yellow", "green");
        });
    });

    describe("accepted call", () => {
        it("renders green", () => {
            useCallStore.setState({
                callDisplay: {type: "accepted", call: makeTestCall()},
            });
            render(<PhoneButton />);
            expectColorWithoutHighlight("green");
        });

        it("renders yellow with green highlight for priority call", () => {
            useCallStore.setState({
                callDisplay: {type: "accepted", call: makeTestCall({prio: true})},
            });
            render(<PhoneButton />);
            expectColorAndHighlight("yellow", "green");
        });

        it("shows accepted state when incoming calls exist", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "accepted", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectColorWithoutHighlight("green");
        });
    });

    describe("rejected call", () => {
        it("blinks between green and gray with green highlight", async () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "rejected", call: makeTestCall()},
            });
            render(<PhoneButton />);
            expectColorAndHighlight("green", "green");

            await flipBlink();
            expectColorAndHighlight("gray", "green");

            await flipBlink();
            expectColorAndHighlight("green", "green");
        });
    });

    describe("error call", () => {
        it("blinks between red and gray", async () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "error", call: makeTestCall()},
            });
            render(<PhoneButton />);
            expectColorWithoutHighlight("red");

            await flipBlink();
            expectColorWithoutHighlight("gray");

            await flipBlink();
            expectColorWithoutHighlight("red");
        });
    });
});

function getButton() {
    return screen.getByRole("button", {name: "Phone"});
}

function expectColorWithoutHighlight(color: ButtonColor) {
    const btn = getButton();
    expect(btn).toHaveClasses(ButtonColors[color]);
    expect(btn.querySelector("div")).toBeNull();
}

function expectColorAndHighlight(color: ButtonColor, highlight: ButtonHighlightColor) {
    const btn = getButton();
    expect(btn).toHaveClasses(ButtonColors[color]);
    expect(btn.querySelector("div")).toHaveClasses(ButtonHighlightColors[highlight]);
}
