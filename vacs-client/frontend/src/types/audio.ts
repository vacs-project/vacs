export type AudioDevices = {
    preferred?: string;
    picked?: string;
    default: string;
    all: string[];
};

export type AudioVolumes = {
    input: number;
    output: number;
    click: number;
    chime: number;
};

export type RingSoundType = "ring" | "priorityRing";

export type RingSound = {
    path: string;
    available: boolean; // false when the file could not be loaded and the built-in chime plays
};

export type RingSounds = {
    ring?: RingSound;
    priorityRing?: RingSound;
};

export type AudioHosts = {
    selected: string;
    all: string[];
};

export type InputLevel = {
    dbfsRms: number; // e.g. -23.4
    dbfsPeak: number; // e.g. -1.2
    norm: number; // 0..1, for display purposes
    clipping: boolean;
};

export type PlaybackDeviceType = "Output" | "Speaker";
