export async function click(element: ChainablePromiseElement): Promise<void> {
    // Since WebKitWebDriver does not support native element clicking,
    // we have to resort to executing a click in the browser context on the element directly.
    await browser.execute((el: HTMLElement) => el.click(), element);
}
