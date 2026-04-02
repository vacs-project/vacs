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
