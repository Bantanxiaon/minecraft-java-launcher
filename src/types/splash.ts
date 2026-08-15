export type BootStepState = "pending" | "running" | "done" | "warn" | "error";

export type BootStepKey =
  | "game"
  | "instances"
  | "mods"
  | "java"
  | "settings"
  | "update";

export type BootStep = {
  key: BootStepKey;
  label: string;
  detail: string;
  state: BootStepState;
};

export type BootHealthReport = {
  java: {
    detectedCount: number;
    has64Bit: boolean;
    recommendedMajor?: number | null;
  };
  instances: Array<{
    id: number;
    name: string;
    gameVersion: string;
    loaderType: string;
    status: string;
  }>;
  mods: Array<{
    instanceId: number;
    modCount: number;
    missingDependencies: string[];
    incompatibleMods: string[];
  }>;
};
