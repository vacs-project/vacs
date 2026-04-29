// Mirrors `crate::replay::TapId` (snake_case serde rename).
export type TapId = {frequency: number} | "headset" | "speaker" | "merged";

// Mirrors `crate::replay::ClipMeta`. Field names use Rust's default snake_case;
// SystemTime serializes as `{ secs_since_epoch, nanos_since_epoch }`.
export type ClipMeta = {
    id: number;
    path: string;
    tap: TapId;
    callsign: string | null;
    frequency: number | null;
    started_at: {secs_since_epoch: number; nanos_since_epoch: number};
    ended_at: {secs_since_epoch: number; nanos_since_epoch: number};
    duration_ms: number;
};

export function clipUnixMs(t: ClipMeta["started_at"]): number {
    return t.secs_since_epoch * 1000 + Math.floor(t.nanos_since_epoch / 1_000_000);
}

export function formatFrequencyMhz(freq: number): string {
    // Frequency is encoded in kHz, e.g. 121500 -> 121.500 MHz.
    const mhz = freq / 1000;
    return mhz.toFixed(3);
}

export function tapLabel(tap: TapId): string {
    if (typeof tap === "object") return formatFrequencyMhz(tap.frequency);
    if (tap === "headset") return "Headset";
    if (tap === "speaker") return "Speaker";
    return "Mixed";
}
