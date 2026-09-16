/* tslint:disable */
/* eslint-disable */

/**
 * A chart part-way through the pipeline, for stage-by-stage timing.
 */
export class Chart {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Stage 3: encode the raster as PNG.
     */
    encode(compression: number): Uint8Array;
    /**
     * Stage 1: parse the spec and build the scene (scales, marks, axes).
     */
    constructor(spec: string);
    /**
     * Stage 2: rasterize the scene at `scale` device pixels per scene unit.
     */
    rasterize(scale: number): void;
    /**
     * Canvas height in pixels.
     */
    readonly height: number;
    /**
     * Number of items in the scene, a rough proxy for chart complexity.
     */
    readonly itemCount: number;
    readonly plotHeight: number;
    readonly plotWidth: number;
    /**
     * The data rectangle in canvas coordinates.
     *
     * Exposed so a caller can align this chart against another renderer's output
     * — the two canvases differ in size, but their plot rectangles are what
     * should coincide.
     */
    readonly plotX: number;
    readonly plotY: number;
    /**
     * Canvas width in pixels.
     */
    readonly width: number;
}

/**
 * Renders a Vega-Lite-style spec to PNG bytes.
 *
 * `scale` is device pixels per scene unit — pass `devicePixelRatio` to match
 * what a canvas renderer does on a HiDPI display.
 */
export function render_png(spec: string, compression: number, scale: number): Uint8Array;

/**
 * The library version, so a demo can show what it is running.
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_chart_free: (a: number, b: number) => void;
    readonly chart_encode: (a: number, b: number, c: number) => void;
    readonly chart_height: (a: number) => number;
    readonly chart_itemCount: (a: number) => number;
    readonly chart_new: (a: number, b: number, c: number) => void;
    readonly chart_plotHeight: (a: number) => number;
    readonly chart_plotWidth: (a: number) => number;
    readonly chart_plotX: (a: number) => number;
    readonly chart_plotY: (a: number) => number;
    readonly chart_rasterize: (a: number, b: number, c: number) => void;
    readonly chart_width: (a: number) => number;
    readonly render_png: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly version: (a: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export2: (a: number, b: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number, d: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
