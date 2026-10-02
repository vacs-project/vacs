/** The subset of gifenc's API the documentation captures use. gifenc ships no types. */
declare module "gifenc" {
    type Palette = number[][];

    type Encoder = {
        writeFrame(
            index: Uint8Array,
            width: number,
            height: number,
            options?: {
                palette?: Palette;
                delay?: number;
                repeat?: number;
                transparent?: boolean;
                transparentIndex?: number;
                dispose?: number;
            },
        ): void;
        finish(): void;
        bytes(): Uint8Array;
    };

    const gifenc: {
        GIFEncoder(): Encoder;
        quantize(rgba: Uint8Array, maxColors: number, options?: {format?: string}): Palette;
        applyPalette(rgba: Uint8Array, palette: Palette, format?: string): Uint8Array;
    };

    export default gifenc;
}
