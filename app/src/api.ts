// What the interface asks of the app underneath it.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Kind = "image" | "screenshot" | "pdf" | "doc" | "audio" | "video";
export type SegmentKind = "name" | "image" | "ocr" | "page" | "chunk" | "sound" | "frame";

export interface Hit {
  file: number;
  path: string;
  kind: Kind;
  score: number;
  z: number | null;
  cosine: number | null;
  words: number;
  segment: number;
  segment_kind: SegmentKind;
  loc: number | null;
  loc_end: number | null;
  excerpt: string | null;
  modified: number;
  size: number;
  info: Record<string, string | number>;
  strong: boolean;
}

export interface Filters {
  kinds: Kind[];
  folders: string[];
  extensions: string[];
  after: number | null;
  before: number | null;
  negative: string[];
  phrases: string[];
}

export interface SearchResult {
  hits: Hit[];
  ms: number;
  vectors: number;
  filters: Filters;
}

export interface Moment {
  segment: number;
  kind: SegmentKind;
  loc: number;
  loc_end: number | null;
  z: number;
}

export interface Root {
  path: string;
  exclude: string[];
}

export interface Config {
  faces?: boolean;
  updates?: boolean;
  speech?: boolean;
  roots: Root[];
  rules: { skip_folders: string[]; skip_extensions: string[]; skip_kinds: Kind[] };
  ocr: boolean;
  device: "auto" | "cpu" | "gpu";
  hotkey: string;
  theme: "system" | "light" | "dark";
  accent: "gold" | "navy" | "violet";
  language: "auto" | "en" | "vi";
  autostart: boolean;
  only_on_power: boolean;
  paused: boolean;
  setup_done: boolean;
  door: { enabled: boolean; port: number; token: string };
}

export interface IndexState {
  running: boolean;
  paused: boolean;
  phase: "idle" | "crawling" | "loading" | "reading" | "deepening";
  done: number;
  total: number;
  current: string | null;
  failed: number;
  waiting_for: "model" | "battery" | "paused" | null;
  error: string | null;
  finished_at: number | null;
  last_indexed: number;
}

export interface Counts {
  files: number;
  pending: number;
  partial: number;
  failed: number;
  segments: number;
  vectors: number;
  by_kind: [Kind, number][];
}

export interface Status {
  counts: Counts;
  index_bytes: number;
  model_ready: boolean;
  model_missing: string[];
  ffmpeg: string | null;
  speed: Speed | null;
  index_state: IndexState;
  version: string;
}

export interface Speed {
  device: "cpu" | "gpu";
  image: number;
  screenshot: number;
  passage: number;
  sound: number;
  gpu_name: string | null;
  agreement: number | null;
  cpu_image: number | null;
  gpu_image: number | null;
}

export interface Estimate {
  files: number;
  seconds: number;
  kinds: Record<Kind, { files: number; bytes: number; seconds: number }>;
  cloud_only: number;
  datasets: string[];
  measured: boolean;
  device: string;
}

/** A request to the search helper. */
export function ask<T>(request: Record<string, unknown>): Promise<T> {
  return invoke<T>("ask", { request });
}

export const search = (q: string, options: { typing?: boolean; limit?: number } = {}) =>
  ask<SearchResult>({ op: "search", q, typing: options.typing ?? false, limit: options.limit ?? 30 });
export const similar = (file: number, q = "", limit = 30) => ask<SearchResult>({ op: "similar", file, q, limit });
export const byExample = (path: string, q = "", limit = 30) => ask<SearchResult>({ op: "example", path, q, limit });
export const byFace = (path: string, limit = 30) => ask<SearchResult>({ op: "face_example", path, limit });
export const moments = (file: number, q: string) => ask<{ moments: Moment[] }>({ op: "moments", file, q });
export const segmentText = (segment: number) => ask<{ text: string | null }>({ op: "text", segment });
export const pageImage = (path: string, page: number, side = 1400) =>
  ask<{ path: string; width: number; height: number }>({ op: "page", path, page, side });
export const frameImage = (path: string, second: number, side = 640) =>
  ask<{ path: string }>({ op: "frame", path, second, side });

export const getConfig = () => invoke<Config>("get_config");
export const setConfig = (settings: Config) => invoke<Config>("set_config", { settings });
export const status = () => invoke<Status>("status");
export const indexState = () => invoke<IndexState>("index_state");
export const indexNow = () => invoke<void>("index_now");
export const indexRetry = () => invoke<void>("index_retry");
export const estimate = (settings: Config) => invoke<Estimate>("estimate", { settings });
export const modelDownload = () => invoke<void>("model_download");
export const speedCheck = () => invoke<void>("speed_check");
export const pickFolders = () => invoke<string[]>("pick_folders");
export const pickExample = () => invoke<string | null>("pick_example");
export const suggestedFolders = () => invoke<{ name: string; path: string; cloud: boolean }[]>("suggested_folders");
export const saveExample = (bytes: Uint8Array, extension: string) =>
  invoke<string>("save_example", { bytes: Array.from(bytes), extension });
export const openResult = (path: string, page?: number | null, second?: number | null) =>
  invoke<void>("open_result", { path, page: page ?? null, second: second ?? null });
export const reveal = (path: string) => invoke<void>("reveal", { path });
export const copyFiles = (paths: string[]) => invoke<void>("copy_files", { paths });
export const openMain = (view?: string, query?: string) =>
  invoke<void>("open_main", { view: view ?? null, query: query ?? null });
export const hidePanel = () => invoke<void>("hide_panel");
export const pinPanel = (pinned: boolean) => invoke<void>("pin_panel", { pinned });
export const openSite = (rest: string) => invoke<void>("open_site", { rest });
export const appVersion = () => invoke<string>("app_version");
export const quit = () => invoke<void>("quit");
export const launchTarget = () => invoke<{ view: string | null; query: string | null } | null>("launch_target");
export const ocrLanguages = () => invoke<string[]>("ocr_languages");

const thumbs = new Map<string, Promise<string>>();

/** Explorer's thumbnail of a file, as a URL an <img> can show. */
export function thumbnail(path: string, side = 256): Promise<string> {
  const key = `${side}|${path}`;
  let known = thumbs.get(key);
  if (!known) {
    known = invoke<string>("thumbnail", { path, side }).then(convertFileSrc);
    thumbs.set(key, known);
    known.catch(() => thumbs.delete(key));
  }
  return known;
}

export const fileUrl = (path: string) => convertFileSrc(path);

export function on<T>(event: string, handler: (payload: T) => void): void {
  void listen<T>(event, (message) => handler(message.payload));
}
