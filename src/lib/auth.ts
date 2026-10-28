import { invoke } from "@tauri-apps/api/core";
import type { TranslateFn } from "./i18n";

export interface ElyAccount {
  accessToken: string;
  refreshToken?: string | null;
  clientToken?: string | null;
  accessTokenExpiresAt?: number | null;
}

export interface Account {
  username: string;
  uuid: string;
  isOffline: boolean;
  loggedInAt: number;
  ely?: ElyAccount | null;
}

export type AuthError =
  | { type: "invalid_username"; username: string; reason: string }
  | { type: "unknown_account"; username: string }
  | { type: "storage"; message: string }
  | { type: "oauth"; message: string }
  | { type: "ely_api"; message: string }
  | { type: "cancelled" };

export function parseAuthError(error: unknown): AuthError | null {
  if (typeof error !== "object" || error === null) {
    return null;
  }
  const type = (error as AuthError).type;
  if (
    type === "invalid_username" ||
    type === "unknown_account" ||
    type === "storage" ||
    type === "oauth" ||
    type === "ely_api" ||
    type === "cancelled"
  ) {
    return error as AuthError;
  }
  return null;
}

export function authErrorMessage(error: unknown, t: TranslateFn): string {
  const authError: AuthError | null = parseAuthError(error);
  if (authError === null) {
    return t("error.unexpected");
  }
  switch (authError.type) {
    case "invalid_username":
      return t("error.invalid_username", {
        username: authError.username,
        reason: authError.reason,
      });
    case "unknown_account":
      return t("error.unknown_account", { username: authError.username });
    case "storage":
      return t("error.storage", { message: authError.message });
    case "oauth":
      return t("error.oauth", { message: authError.message });
    case "ely_api":
      return t("error.ely_api", { message: authError.message });
    case "cancelled":
      return t("error.cancelled");
  }
}

export interface AuthService {
  loginOffline(username: string): Promise<Account>;
  loginSavedAccount(username: string, isOffline: boolean): Promise<Account>;
  getCurrentAccount(): Promise<Account | null>;
  logout(): Promise<void>;
  listSavedAccounts(): Promise<Account[]>;
  removeSavedAccount(
    username: string,
    isOffline: boolean,
  ): Promise<Account[]>;
  loginEly(): Promise<Account>;
  cancelEly(): Promise<void>;
  loginElyPassword(username: string, password: string): Promise<Account>;
  refreshEly(): Promise<Account>;
}

export const auth: AuthService = {
  loginOffline: (username) => invoke("auth_login_offline", { username }),
  loginSavedAccount: (username, isOffline) =>
    invoke("auth_login_saved", { username, isOffline }),
  getCurrentAccount: () => invoke("auth_current"),
  logout: () => invoke("auth_logout"),
  listSavedAccounts: () => invoke("auth_saved_accounts"),
  removeSavedAccount: (username, isOffline) =>
    invoke("auth_remove_saved", { username, isOffline }),
  loginEly: () => invoke("auth_login_ely"),
  cancelEly: () => invoke("auth_cancel_ely"),
  loginElyPassword: (username, password) =>
    invoke("auth_login_ely_password", { username, password }),
  refreshEly: () => invoke("auth_ely_refresh"),
};
