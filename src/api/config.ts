import { typedInvoke } from "./invoke";
import type { AppConfig } from "./types";

export const apiConfig = {
  getConfig: () => typedInvoke("get_config"),
  saveConfig: (cfg: AppConfig) => typedInvoke("save_config_cmd", { cfg }),
};
