import {useSplitView} from "./page-hook";

export function useDaKeyWidth() {
    const splitView = useSplitView();

    return splitView ? "5.5rem" : "6.25rem";
}
