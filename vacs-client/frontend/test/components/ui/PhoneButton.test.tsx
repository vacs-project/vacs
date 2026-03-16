import {describe, expect, it, afterEach} from "vitest";
import {render, screen} from "@testing-library/preact";
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
});

function getButton() {
    return screen.getByRole("button", {name: "Phone"});
}

function expectButton(color: ButtonColor, highlight?: ButtonHighlightColor) {
    const btn = getButton();
    expect(btn).toHaveClass(ButtonColors[color]);
    if (highlight !== undefined) {
        expect(btn.querySelector("div")).toHaveClass(ButtonHighlightColors[highlight]);
    } else {
        expect(btn.querySelector("div")).toBeNull();
    }
}

describe("PhoneButton", () => {
    it("renders gray when idle", () => {
        render(<PhoneButton />);
        expectButton("gray");
    });

    describe("outgoing call", () => {
        it("renders gray with green highlight", () => {
            useCallStore.setState({
                callDisplay: {type: "outgoing", call: makeTestCall()},
            });
            render(<PhoneButton />);
            expectButton("gray", "green");
        });

        it("blinks between yellow with green highlight and gray with green highlight for priority call", () => {
            useCallStore.setState({
                callDisplay: {type: "outgoing", call: makeTestCall({prio: true})},
                blink: true,
            });
            render(<PhoneButton />);
            expectButton("yellow", "green");

            flipBlink();
            expectButton("gray", "green");

            flipBlink();
            expectButton("yellow", "green");
        });

        it("shows outgoing state when both outgoing and incoming calls exist", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "outgoing", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectButton("gray", "green");

            flipBlink();
            expectButton("gray", "green");

            flipBlink();
            expectButton("gray", "green");
        });

        it("ignores incoming prio when outgoing call exists", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "outgoing", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId, prio: true})],
            });
            render(<PhoneButton />);
            expectButton("gray", "green");

            flipBlink();
            expectButton("gray", "green");

            flipBlink();
            expectButton("gray", "green");
        });

        it("shows rejected state when both rejected and incoming calls exist", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "rejected", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectButton("green", "green");

            flipBlink();
            expectButton("gray", "green");

            flipBlink();
            expectButton("green", "green");
        });

        it("shows error state when both error and incoming calls exist", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "error", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectButton("red");

            flipBlink();
            expectButton("gray");

            flipBlink();
            expectButton("red");
        });

        it("shows accepted state when both accepted and incoming calls exist", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "accepted", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectButton("green");

            flipBlink();
            expectButton("green");

            flipBlink();
            expectButton("green");
        });
    });

    describe("incoming call", () => {
        it("blinks between green and gray for incoming call", () => {
            useCallStore.setState({
                blink: true,
                incomingCalls: [makeTestCall()],
            });
            render(<PhoneButton />);
            expectButton("green");

            flipBlink();
            expectButton("gray");

            flipBlink();
            expectButton("green");
        });

        it("blinks between yellow with green highlight and gray for priority call", () => {
            useCallStore.setState({
                blink: true,
                incomingCalls: [makeTestCall({prio: true})],
            });
            render(<PhoneButton />);
            expectButton("yellow", "green");

            flipBlink();
            expectButton("gray", "gray");

            flipBlink();
            expectButton("yellow", "green");
        });
    });

    describe("accepted call", () => {
        it("renders green", () => {
            useCallStore.setState({
                callDisplay: {type: "accepted", call: makeTestCall()},
            });
            render(<PhoneButton />);
            expectButton("green");
        });

        it("renders yellow with green highlight for priority call", () => {
            useCallStore.setState({
                callDisplay: {type: "accepted", call: makeTestCall({prio: true})},
            });
            render(<PhoneButton />);
            expectButton("yellow", "green");
        });

        it("shows accepted state when incoming calls exist", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "accepted", call: makeTestCall()},
                incomingCalls: [makeTestCall({callId: "call1" as CallId})],
            });
            render(<PhoneButton />);
            expectButton("green");
        });
    });

    describe("rejected call", () => {
        it("blinks between green and gray with green highlight", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "rejected", call: makeTestCall()},
            });
            render(<PhoneButton />);
            expectButton("green", "green");

            flipBlink();
            expectButton("gray", "green");

            flipBlink();
            expectButton("green", "green");
        });
    });

    describe("error call", () => {
        it("blinks between red and gray", () => {
            useCallStore.setState({
                blink: true,
                callDisplay: {type: "error", call: makeTestCall()},
            });
            render(<PhoneButton />);
            expectButton("red");

            flipBlink();
            expectButton("gray");

            flipBlink();
            expectButton("red");
        });
    });
});
