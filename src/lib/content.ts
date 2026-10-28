import { invoke } from "@tauri-apps/api/core";
import type { TranslateFn } from "./i18n";

export type ContentCategory = "mods" | "resourcepacks" | "shaderpacks";
export type ContentSource = "modrinth" | "curseforge";

export interface ContentItem {
  source: ContentSource;
  projectId: string;
  slug: string;
  name: string;
  description: string;
  author: string;
  downloads: number;
  iconUrl: string | null;
  websiteUrl: string | null;
}

export interface ContentVersion {
  source: ContentSource;
  versionId: string;
  name: string;
  fileName: string;
  fileUrl: string;
  fileSize: number;
  sha1: string | null;
  datePublished: string | null;
}

export interface InstalledContent {
  fileName: string;
  sizeBytes: number;
}

export type ContentError =
  | { type: "storage"; message: string }
  | { type: "not_found"; id: string }
  | { type: "network"; message: string }
  | { type: "api"; message: string };

export function parseContentError(error: unknown): ContentError | null {
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
    case "api":
      if (typeof (error as { message?: unknown }).message === "string") {
        return error as ContentError;
      }
      return null;
    case "not_found":
      if (typeof (error as { id?: unknown }).id === "string") {
        return error as ContentError;
      }
      return null;
    default:
      return null;
  }
}

export function contentErrorMessage(error: unknown, t: TranslateFn): string {
  const parsed = parseContentError(error);
  if (parsed === null) {
    return t("error.unexpected_instance");
  }
  switch (parsed.type) {
    case "storage":
      return t("error.storage", { message: parsed.message });
    case "network":
      return t("error.content_network", { message: parsed.message });
    case "api":
      return t("error.content_api", { message: parsed.message });
    case "not_found":
      return t("error.content_not_found", { id: parsed.id });
  }
}

export interface ContentService {
  search(
    instanceId: string,
    category: ContentCategory,
    query: string,
  ): Promise<ContentItem[]>;
  versions(
    instanceId: string,
    category: ContentCategory,
    source: ContentSource,
    projectId: string,
  ): Promise<ContentVersion[]>;
  install(
    instanceId: string,
    category: ContentCategory,
    source: ContentSource,
    projectId: string,
    versionId: string,
  ): Promise<InstalledContent[]>;
  installed(
    instanceId: string,
    category: ContentCategory,
  ): Promise<InstalledContent[]>;
  remove(
    instanceId: string,
    category: ContentCategory,
    fileName: string,
  ): Promise<void>;
}

export const content: ContentService = {
  search: (instanceId, category, query) =>
    invoke("content_search", { instanceId, category, query }),
  versions: (instanceId, category, source, projectId) =>
    invoke("content_versions", { instanceId, category, source, projectId }),
  install: (instanceId, category, source, projectId, versionId) =>
    invoke("content_install", {
      instanceId,
      category,
      source,
      projectId,
      versionId,
    }),
  installed: (instanceId, category) =>
    invoke("content_installed", { instanceId, category }),
  remove: (instanceId, category, fileName) =>
    invoke("content_remove", { instanceId, category, fileName }),
};

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }
  return `${bytes} B`;
}
