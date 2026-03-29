import {useRef, useState} from "preact/hooks";

const getNextChar = (char: string, chars: string[]): string => {
    const index = chars.indexOf(char);
    if (index === chars.length - 1 || index === -1) {
        return chars[0];
    }
    return chars[index + 1];
};

export function useDialPadInput() {
    const [dialInput, setDialInput] = useState<string>("");
    const multipleTapTimeoutRef = useRef<number | undefined>(undefined);

    const handleDialClick = (digit: string, buttonChars: string) => {
        if (dialInput.length >= 8) return;

        if (buttonChars === "") {
            setDialInput(dialInput => dialInput + digit);
            return;
        }

        const chars = Array.from(buttonChars);
        const lastChar = dialInput[dialInput.length - 1] ?? "";

        if (multipleTapTimeoutRef.current !== undefined) {
            clearTimeout(multipleTapTimeoutRef.current);

            if (lastChar === digit || chars.includes(lastChar)) {
                // Same button within 1 second
                setDialInput(dialInput => dialInput.slice(0, -1) + getNextChar(lastChar, chars));
            } else {
                // Another button within 1 second
                setDialInput(dialInput => dialInput + digit);
            }
        } else {
            // No button within 1 second
            setDialInput(dialInput => dialInput + digit);
        }
        multipleTapTimeoutRef.current = setTimeout(
            () => (multipleTapTimeoutRef.current = undefined),
            1000,
        );
    };

    const clearLastChar = () => {
        setDialInput(dialInput => dialInput.slice(0, -1));
    };

    const clearAll = () => {
        setDialInput("");
    };

    return {dialInput, setDialInput, handleDialClick, clearLastChar, clearAll};
}
