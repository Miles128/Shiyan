import { typedInvoke } from "./invoke";

export const apiData = {
  /** Whole-database snapshot via VACUUM INTO; returns the path or null on cancel. */
  backupDatabase: () => typedInvoke("backup_database"),
};
