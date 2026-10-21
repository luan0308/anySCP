/** Format a number with a fixed number of decimals using the pt-BR comma. */
function fixedPtBr(value: number, digits: number): string {
  return value.toFixed(digits).replace(".", ",");
}

/**
 * Format a byte count to a human-readable string.
 * Examples: 0 → "0 B", 1024 → "1,0 KB", 1048576 → "1,0 MB"
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${fixedPtBr(bytes / Math.pow(k, i), i > 0 ? 1 : 0)} ${sizes[i]}`;
}

/**
 * Format a transfer speed in bytes/sec to a human-readable string.
 * Examples: 500 → "500 B/s", 2048 → "2,0 KB/s", 5242880 → "5,0 MB/s"
 */
export function formatSpeed(bps: number): string {
  if (bps < 1024) return `${bps} B/s`;
  if (bps < 1024 * 1024) return `${fixedPtBr(bps / 1024, 1)} KB/s`;
  return `${fixedPtBr(bps / (1024 * 1024), 1)} MB/s`;
}

/**
 * Format an ETA in seconds to a human-readable string.
 * Returns an empty string when eta is null or zero.
 */
export function formatEta(secs: number | null): string {
  if (secs === null || secs <= 0) return "";
  if (secs < 60) return `~${secs} s`;
  if (secs < 3600) return `~${Math.floor(secs / 60)} min ${secs % 60} s`;
  return `~${Math.floor(secs / 3600)} h ${Math.floor((secs % 3600) / 60)} min`;
}

import type { TransferStatusValue } from "../types";

/**
 * Coerce a TransferStatusValue (which may be a tagged union object) into a
 * plain string for display or comparison purposes.
 */
export function getStatusString(status: TransferStatusValue): string {
  if (typeof status === "string") return status;
  if ("Failed" in status) return "Failed";
  return "Unknown";
}
