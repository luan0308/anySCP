import { describe, it, expect, vi, beforeEach } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { explorerInvoke, closeExplorerSession, transferEventName } from "../explorer-transport";

describe("explorer transport", () => {
  beforeEach(() => invoke.mockReset().mockResolvedValue(undefined));

  it("prefixes commands and names the session argument per transport", async () => {
    await explorerInvoke("ftp", "list_dir", "f1", { path: "/www" });
    expect(invoke).toHaveBeenCalledWith("ftp_list_dir", { ftpSessionId: "f1", path: "/www" });
    await explorerInvoke("scp", "home_dir", "c1");
    expect(invoke).toHaveBeenCalledWith("scp_home_dir", { scpSessionId: "c1" });
    await explorerInvoke("sftp", "home_dir", "s1");
    expect(invoke).toHaveBeenCalledWith("sftp_home_dir", { sftpSessionId: "s1" });
  });

  it("closes a session with its own transport", async () => {
    await closeExplorerSession("ftp", "f1");
    expect(invoke).toHaveBeenCalledWith("ftp_close", { ftpSessionId: "f1" });
  });

  it("listens on the transport's transfer channel", () => {
    expect(transferEventName("ftp")).toBe("ftp:transfer");
  });
});
