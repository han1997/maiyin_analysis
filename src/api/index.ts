import type { AppApi } from "./contract";
import { browserApi } from "./browserApi";
import { tauriApi } from "./tauriApi";

export const appApi: AppApi = window.__TAURI_INTERNALS__ ? tauriApi : browserApi;

