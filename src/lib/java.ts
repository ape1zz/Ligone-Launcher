import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TranslateFn } from "./i18n";

export type JavaSource = "system" | "bundled" | "custom";

export type JavaStatus =
  | { type: "idle" }
  | { type: "checking" }
  | {
      type: "ready";
      source: JavaSource;
      path: string;
      version?: string | null;
    }
  | {
      type: "installing";
      component: string;
      done: number;
      total: number;
      bytesDone: number;
      bytesTotal: number;
    }
  | { type: "error"; message: string };

export function javaStatus(): Promise<JavaStatus> {
  return invoke("java_status");
}

export function onJavaStatus(
  handler: (status: JavaStatus) => void,
): Promise<UnlistenFn> {
  return listen<JavaStatus>("java-status", (event) => {
    handler(event.payload);
  });
}

export function useJavaStatus(): JavaStatus | null {
  const [status, setStatus] = useState<JavaStatus | null>(null);
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    javaStatus()
      .then((loaded) => {
        if (active) {
          setStatus(loaded);
        }
      })
      .catch(() => undefined);
    onJavaStatus((incoming) => {
      if (active) {
        setStatus(incoming);
      }
    })
      .then((stop) => {
        if (active) {
          unlisten = stop;
        } else {
          stop();
        }
      })
      .catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
  return status;
}

export function javaStatusText(status: JavaStatus | null, t: TranslateFn): string {
  if (status === null) {
    return t("java.checking");
  }
  switch (status.type) {
    case "idle":
      return t("java.idle");
    case "checking":
      return t("java.checking");
    case "ready": {
      const version = status.version ?? "";
      switch (status.source) {
        case "system":
          return t("java.ready_system", { version });
        case "bundled":
          return t("java.ready_bundled", { version });
        case "custom":
          return t("java.ready_custom");
      }
      break;
    }
    case "installing": {
      const percent =
        status.bytesTotal > 0
          ? Math.round((status.bytesDone / status.bytesTotal) * 100)
          : status.total > 0
            ? Math.round((status.done / status.total) * 100)
            : 0;
      return t("java.installing", { percent });
    }
    case "error":
      return t("java.error", { message: status.message });
  }
}
