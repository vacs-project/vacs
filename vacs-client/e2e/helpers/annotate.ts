/**
 * Numbered callouts drawn into the page before a capture.
 *
 * The manual's annotated screenshots mark a UI element with a box and a
 * numbered badge, and explain the numbers in the prose next to the image.
 * Deriving box and badge from the element's bounding rect keeps them on the
 * right element when the layout changes: re-run the capture and they follow.
 *
 * The overlay is an SVG on top of the page, so it lands in the screenshot
 * like any other pixel, and is removed again by clearAnnotations().
 */

/**
 * Where the badge sits relative to the element: on one of its corners, or
 * centered straight above, below or beside it.
 */
export type Placement =
    | "top-left"
    | "top-right"
    | "bottom-left"
    | "bottom-right"
    | "above"
    | "below"
    | "left"
    | "right";

export type Annotation = {
    /** CSS or XPath selector of the element the callout is about. */
    target: string;
    /** Number shown in the badge. Omit for a box without a badge. */
    badge?: number;
    /** Where the badge sits. Defaults to "top-left". */
    place?: Placement;
    /** Any CSS color. Defaults to the annotation red. */
    color?: string;
    /** Draws the box around the element. Defaults to true. */
    box?: boolean;
};

const OVERLAY_ID = "vacs-docs-annotations";

/**
 * Callout colors. Red is the manual's established annotation color and no
 * part of the app's own UI uses it at rest, so a callout never reads as
 * application state. The others are the documentation site's palette, for
 * the rare image that needs to tell two groups of callouts apart.
 */
export const ANNOTATION_COLORS = {
    red: "#e03131",
    blue: "#1a5bb8",
    violet: "#6741d9",
    teal: "#0b7285",
};

/** Thin ring around the badge, so it separates from a dark key behind it. */
const HALO = "#ffffff";

/**
 * Draws the given callouts over the page. Call before a capture, and
 * clearAnnotations() after, so a later capture in the same test starts clean.
 */
export async function annotate(
    browser: WebdriverIO.Browser,
    annotations: Annotation[],
): Promise<void> {
    await browser.execute(
        (specs: Annotation[], overlayId: string, fallback: string, halo: string) => {
            const SVG_NS = "http://www.w3.org/2000/svg";
            const INSET = 4;
            const BADGE_RADIUS = 13;

            const resolve = (selector: string): Element | null => {
                if (selector.startsWith("/") || selector.startsWith("(")) {
                    return document.evaluate(
                        selector,
                        document,
                        null,
                        XPathResult.FIRST_ORDERED_NODE_TYPE,
                        null,
                    ).singleNodeValue as Element | null;
                }
                return document.querySelector(selector);
            };

            const element = (name: string, attributes: Record<string, string>): SVGElement => {
                const node = document.createElementNS(SVG_NS, name);
                for (const [key, value] of Object.entries(attributes)) {
                    node.setAttribute(key, value);
                }
                return node;
            };

            document.getElementById(overlayId)?.remove();

            const svg = document.createElementNS(SVG_NS, "svg");
            svg.id = overlayId;
            svg.setAttribute("width", String(window.innerWidth));
            svg.setAttribute("height", String(window.innerHeight));
            Object.assign(svg.style, {
                position: "fixed",
                inset: "0",
                zIndex: "2147483647",
                pointerEvents: "none",
            });

            for (const spec of specs) {
                const target = resolve(spec.target);
                if (target === null) throw new Error(`Annotation target not found: ${spec.target}`);

                const rect = target.getBoundingClientRect();
                const color = spec.color ?? fallback;
                const place = spec.place ?? "top-left";

                const left = rect.left - INSET;
                const top = rect.top - INSET;
                const width = rect.width + INSET * 2;
                const height = rect.height + INSET * 2;

                if (spec.box !== false) {
                    svg.append(
                        element("rect", {
                            x: String(left),
                            y: String(top),
                            width: String(width),
                            height: String(height),
                            fill: "none",
                            stroke: color,
                            "stroke-width": "2.5",
                            rx: "4",
                        }),
                    );
                }

                if (spec.badge === undefined) continue;

                // The badge is tangent to the element: as close as it gets
                // without covering it or, where there is one, the box's
                // stroke. On a corner it approaches along the diagonal, on a
                // side it sits centered and straight out.
                const anchorInset = spec.box === false ? 1 : INSET;
                const anchorLeft = rect.left - anchorInset;
                const anchorTop = rect.top - anchorInset;
                const anchorRight = rect.right + anchorInset;
                const anchorBottom = rect.bottom + anchorInset;

                const straight = BADGE_RADIUS + 1;
                const diagonal = straight / Math.SQRT2;

                let x: number;
                let y: number;
                switch (place) {
                    case "above":
                    case "below":
                        x = (anchorLeft + anchorRight) / 2;
                        y = place === "below" ? anchorBottom + straight : anchorTop - straight;
                        break;
                    case "left":
                    case "right":
                        x = place === "right" ? anchorRight + straight : anchorLeft - straight;
                        y = (anchorTop + anchorBottom) / 2;
                        break;
                    default: {
                        const onRight = place === "top-right" || place === "bottom-right";
                        const onBottom = place === "bottom-left" || place === "bottom-right";
                        x = onRight ? anchorRight + diagonal : anchorLeft - diagonal;
                        y = onBottom ? anchorBottom + diagonal : anchorTop - diagonal;
                    }
                }

                // Clamped so a badge on an element at the window edge stays
                // inside the frame.
                const margin = BADGE_RADIUS + 2;
                const cx = Math.min(Math.max(x, margin), window.innerWidth - margin);
                const cy = Math.min(Math.max(y, margin), window.innerHeight - margin);

                svg.append(
                    element("circle", {
                        cx: String(cx),
                        cy: String(cy),
                        r: String(BADGE_RADIUS),
                        fill: color,
                        // A thin ring, not a halo: enough to separate the
                        // badge from a dark key behind it, nothing more.
                        stroke: halo,
                        "stroke-width": "1.5",
                    }),
                );

                const label = element("text", {
                    x: String(cx),
                    y: String(cy + 1),
                    fill: halo,
                    "font-family": "system-ui, sans-serif",
                    "font-size": "16",
                    "font-weight": "700",
                    "text-anchor": "middle",
                    "dominant-baseline": "middle",
                });
                label.textContent = String(spec.badge);
                svg.append(label);
            }

            document.body.append(svg);
        },
        annotations,
        OVERLAY_ID,
        ANNOTATION_COLORS.red,
        HALO,
    );
}

/** Removes the overlay drawn by annotate(). */
export async function clearAnnotations(browser: WebdriverIO.Browser): Promise<void> {
    await browser.execute((overlayId: string) => {
        document.getElementById(overlayId)?.remove();
    }, OVERLAY_ID);
}
