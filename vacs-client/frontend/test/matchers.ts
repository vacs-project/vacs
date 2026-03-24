import {expect} from "vitest";

// Custom matchers for DOM testing without jest-dom
export const matchers = {
    toHaveClasses(element: Element | null, classes: string) {
        if (!element) {
            return {
                message: () => "Element is null",
                pass: false,
            };
        }

        const expectedClasses = classes.split(" ");
        const hasAllClasses = expectedClasses.every(cls => element.classList.contains(cls));

        return {
            message: () =>
                `expected element ${hasAllClasses ? "not " : ""}to have classes: ${classes}`,
            pass: hasAllClasses,
        };
    },
};

expect.extend(matchers);

declare module "vitest" {
    interface Assertion<T> {
        toHaveClasses(classes: string): T;
    }
    interface AsymmetricMatchersContaining {
        toHaveClasses(classes: string): Element;
    }
}
