import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TranslateFn } from "./i18n";

export type Loader = "vanilla" | "fabric";

export interface Instance {
  id: string;
  name: string;
  versionId: string;
  loader: Loader;
  loaderVersion: string | null;
  createdAt: number;
  downloaded: boolean;
}

export type InstanceError =
  | { type: "storage"; message: string }
  | { type: "not_found"; id: string }
  | { type: "invalid_name"; message: string }
  | { type: "busy"; id: string }
  | { type: "network"; message: string }
  | { type: "download"; message: string }
  | { type: "launch"; message: string }
  | { type: "cancelled" };

export function parseInstanceError(error: unknown): InstanceError | null {
  if (typeof error !== "object" || error === null) {
    return null;
  }
  const type = (error as { type?: unknown }).type;
  if (typeof type !== "string") {
    return null;
  }
  switch (type) {
    case "storage":
    case "network":
    case "download":
      if (typeof (error as { message?: unknown }).message === "string") {
        return error as InstanceError;
      }
      return null;
    case "not_found":
    case "busy":
      if (typeof (error as { id?: unknown }).id === "string") {
        return error as InstanceError;
      }
      return null;
    case "invalid_name":
      if (typeof (error as { message?: unknown }).message === "string") {
        return error as InstanceError;
      }
      return null;
    case "cancelled":
      return error as InstanceError;
    case "launch":
      if (typeof (error as { message?: unknown }).message === "string") {
        return error as InstanceError;
      }
      return null;
    default:
      return null;
  }
}

export function instanceErrorMessage(error: unknown, t: TranslateFn): string {
  const parsed = parseInstanceError(error);
  if (parsed === null) {
    return t("error.unexpected_instance");
  }
  switch (parsed.type) {
    case "storage":
      return t("error.storage", { message: parsed.message });
    case "network":
      return t("error.instance_network", { message: parsed.message });
    case "download":
      return t("error.instance_download", { message: parsed.message });
    case "invalid_name":
      return t("error.instance_invalid_name", { message: parsed.message });
    case "not_found":
      return t("error.instance_not_found");
    case "busy":
      return t("error.instance_busy");
    case "cancelled":
      return t("error.instance_cancelled");
    case "launch":
      return t("error.instance_launch", { message: parsed.message });
  }
}

export interface VersionInfo {
  id: string;
  type: string;
  releaseTime: string;
  url: string;
  sha1?: string | null;
}

export interface FabricLoaderVersion {
  version: string;
  stable: boolean;
}

export interface DownloadProgress {
  instanceId: string;
  stage: "jar" | "libraries" | "assets" | string;
  done: number;
  total: number;
  bytesDone: number;
  bytesTotal: number;
}

export interface CreateInstanceArgs {
  name: string;
  versionId: string;
  loader: Loader;
  loaderVersion?: string | null;
}

export interface RunState {
  instanceId: string;
  running: boolean;
  error?: string | null;
}

export interface InstancesService {
  list(): Promise<Instance[]>;
  create(args: CreateInstanceArgs): Promise<Instance>;
  remove(id: string): Promise<void>;
  rename(id: string, name: string): Promise<Instance>;
  download(id: string): Promise<void>;
  cancelDownload(id: string): Promise<void>;
  launch(id: string): Promise<number>;
  runningIds(): Promise<string[]>;
  versions(): Promise<VersionInfo[]>;
  fabricLoaderVersions(gameVersion: string): Promise<FabricLoaderVersion[]>;
  openFolder(id: string, sub?: string | null): Promise<void>;
}

export const instances: InstancesService = {
  list: () => invoke("instance_list"),
  create: (args) =>
    invoke("instance_create", {
      name: args.name,
      versionId: args.versionId,
      loader: args.loader,
      loaderVersion: args.loaderVersion ?? null,
    }),
  remove: (id) => invoke("instance_remove", { id }),
  rename: (id, name) => invoke("instance_rename", { id, name }),
  download: (id) => invoke("instance_download", { id }),
  cancelDownload: (id) => invoke("instance_cancel_download", { id }),
  launch: (id) => invoke("instance_launch", { id }),
  runningIds: () => invoke("instance_running"),
  versions: () => invoke("version_list"),
  fabricLoaderVersions: (gameVersion) =>
    invoke("fabric_loader_versions", { gameVersion }),
  openFolder: (id, sub = null) => invoke("instance_open_folder", { id, sub }),
};

export function onDownloadProgress(
  handler: (progress: DownloadProgress) => void,
): Promise<UnlistenFn> {
  return listen<DownloadProgress>("instance-download-progress", (event) => {
    handler(event.payload);
  });
}

export function onRunState(
  handler: (state: RunState) => void,
): Promise<UnlistenFn> {
  return listen<RunState>("instance-run-state", (event) => {
    handler(event.payload);
  });
}
