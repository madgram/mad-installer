import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DeviceInfo = {
  udid: string;
  name: string;
  version: string;
  productType: string;
  developerMode: boolean | null;
};

export type DeviceStatus =
  | { state: "noService"; message: string }
  | { state: "noDriver" }
  | { state: "none" }
  | { state: "locked" }
  | { state: "trust" }
  | { state: "denied" }
  | { state: "error"; message: string }
  | { state: "ready"; device: DeviceInfo };

export type Release = {
  tag: string;
  version: string;
  name: string;
  publishedAt: string | null;
  prerelease: boolean;
  assetName: string;
  size: number;
  url: string;
  sha256: string | null;
  cached: boolean;
};

export type AccountInfo = { email: string; name: string | null };

export type InstallRecord = {
  udid: string;
  deviceName: string;
  email: string;
  tag: string;
  version: string;
  bundleId: string;
  installedAt: string;
  expiresAt: string;
};

export type AppError = { kind: string; message: string };

export type TrustedNumber = {
  numberWithDialCode: string;
  lastTwoDigits: string;
  pushMode: string;
  id: number;
};

export type TfaParams = {
  lastError: string | null;
  unknown: boolean;
  sms: boolean;
  numbers: TrustedNumber[];
  selectedNumberId: number | null;
};

export type TfaAnswer =
  | { type: "code"; code: string }
  | { type: "sms"; id: number }
  | { type: "devices" }
  | { type: "resend" }
  | { type: "cancel" };

export type CertInfo = {
  serial: string;
  name: string | null;
  machine: string | null;
  expires: string | null;
};

export type Step = "download" | "prepare" | "sign" | "install" | "done";

export const api = {
  platform: () => invoke<string>("platform"),
  deviceStatus: () => invoke<DeviceStatus>("device_status"),
  listReleases: () => invoke<Release[]>("list_releases"),
  downloadRelease: (release: Release) => invoke<void>("download_release", { release }),
  accountStatus: () => invoke<AccountInfo | null>("account_status"),
  login: (email: string, password: string) => invoke<AccountInfo>("login", { email, password }),
  answerTfa: (answer: TfaAnswer) => invoke<void>("answer_tfa", { answer }),
  logout: () => invoke<void>("logout"),
  install: (udid: string, release: Release) => invoke<InstallRecord>("install", { udid, release }),
  installRecord: (udid: string) => invoke<InstallRecord | null>("install_record", { udid }),
  answerCerts: (serials: string[] | null) => invoke<void>("answer_certs", { serials }),
};

export const on = <T,>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> =>
  listen<T>(event, (e) => cb(e.payload));

export const asError = (e: unknown): AppError =>
  typeof e === "object" && e !== null && "kind" in e
    ? (e as AppError)
    : { kind: "unknown", message: String(e) };
