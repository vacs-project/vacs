const LINE_HEIGHT_PX = 40;

/**
 * Scrolls a container that only overflows horizontally with a vertical mouse wheel.
 *
 * Returns whether the event was consumed. Wheel events that already scroll horizontally or zoom,
 * and containers that can scroll vertically, are left to the browser.
 */
export function scrollHorizontallyOnWheel(element: HTMLElement, event: WheelEvent): boolean {
    if (event.deltaY === 0 || event.deltaX !== 0 || event.shiftKey) return false;
    if (event.ctrlKey || event.metaKey) return false;
    if (element.scrollHeight > element.clientHeight) return false;
    if (element.scrollWidth <= element.clientWidth) return false;

    const delta =
        event.deltaMode === WheelEvent.DOM_DELTA_LINE
            ? event.deltaY * LINE_HEIGHT_PX
            : event.deltaY;
    element.scrollBy({left: delta, behavior: "smooth"});
    event.preventDefault();
    return true;
}
