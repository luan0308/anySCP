// Guards the host card's context-menu wiring — specifically that "Duplicate"
// and "Delete" invoke the callbacks the dashboard routes to the store/backend
// (duplicateHost copies the keychain secret; delete purges it). A regression
// here is exactly how a duplicated password host lost its credential: the menu
// item silently pointed at the wrong path.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { HostCard } from "./HostCard";
import { useHostsStore } from "../../stores/hosts-store";
import { useHealthStore } from "../../stores/health-store";
import type { SavedHost } from "../../types";

function makeHost(over: Partial<SavedHost> = {}): SavedHost {
  return {
    id: "h1",
    label: "web",
    host: "example.com",
    port: 22,
    username: "root",
    auth_type: "password",
    group_id: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    key_path: null,
    color: null,
    notes: null,
    environment: null,
    os_type: null,
    startup_command: null,
    proxy_jump: null,
    proxy_jump_host_id: null,
    start_directory: null,
    keep_alive_interval: null,
    default_shell: null,
    font_size: null,
    last_connected_at: null,
    connection_count: null,
    ...over,
  };
}

function renderCard() {
  const host = makeHost();
  const props = {
    host,
    onConnect: vi.fn(),
    onExplore: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    onDuplicate: vi.fn(),
  };
  render(<HostCard {...props} />);
  return props;
}

/** Right-click the card to open its context menu. */
function openContextMenu(hostId = "h1") {
  fireEvent.contextMenu(screen.getByTestId(`host-card-${hostId}`));
}

describe("HostCard context menu", () => {
  beforeEach(() => {
    // Real stores, default state — the card only reads health + a jump-host lookup.
    useHostsStore.setState({ hosts: [] });
    useHealthStore.setState({ byHostId: {} });
  });

  it("wires Duplicate to onDuplicate (the credential-copying path)", () => {
    const { host, onDuplicate } = renderCard();
    openContextMenu();
    fireEvent.click(screen.getByText("Duplicar"));
    expect(onDuplicate).toHaveBeenCalledTimes(1);
    expect(onDuplicate).toHaveBeenCalledWith(host);
  });

  it("wires Delete to onDelete after the danger confirmation", () => {
    const { host, onDelete } = renderCard();
    openContextMenu();
    fireEvent.click(screen.getByText("Excluir"));
    // The Delete item opens a danger confirmation; confirm it (default label).
    fireEvent.click(screen.getByRole("button", { name: /^confirmar$/i }));
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onDelete).toHaveBeenCalledWith(host.id);
  });
});

describe("HostCard (FTP host)", () => {
  beforeEach(() => {
    useHostsStore.setState({ hosts: [] });
    useHealthStore.setState({ byHostId: {} });
  });

  it("labels the protocol and offers no terminal", () => {
    const host = makeHost({ id: "f1", port: 21, protocol: "ftp", ftp_security: "explicit" });
    render(
      <HostCard
        host={host}
        onConnect={vi.fn()}
        onExplore={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
      />,
    );
    expect(screen.getByText("FTPS")).toBeTruthy();
    expect(screen.getByText(/^root · example\.com/)).toBeTruthy();
    expect(screen.queryByTestId("host-card-f1-terminal")).toBeNull();
    expect(screen.getByTestId("host-card-f1-explorer")).toBeTruthy();
    openContextMenu("f1");
    expect(screen.queryByText("Terminal")).toBeNull();
    expect(screen.getAllByText("Arquivos").length).toBeGreaterThanOrEqual(2);
  });

  it("calls plain FTP just FTP", () => {
    const host = makeHost({ id: "f2", port: 21, protocol: "ftp", ftp_security: "none" });
    render(
      <HostCard
        host={host}
        onConnect={vi.fn()}
        onExplore={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
      />,
    );
    expect(screen.getByText("FTP")).toBeTruthy();
  });
});
