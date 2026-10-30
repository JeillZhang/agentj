import { expect, test, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { render, screen, fireEvent, within, waitFor } from "@testing-library/react";
import type { RpcClient } from "@agentj/sdk";
import { METHODS } from "@agentj/sdk";
import SettingsDialog from "./SettingsDialog";
import type { SubscriptionState, SubscriptionStatusData } from "../hooks/useSubscriptionStatus";

/** Full snapshot shape; only the state matters to the nav badge. */
function statusWith(state: SubscriptionState): SubscriptionStatusData {
  return {
    state,
    windows: [],
    worstPercent: null,
    fetchedAt: 0,
    credential: null,
    warnPercent: null,
    stale: false,
    errorKind: null,
    error: null,
    loading: false,
    refresh: async () => {},
  };
}

// Answers the subscription panel's mount reads and one verified API-key link.
function stubClientWithVerifiedLink() {
  const request = vi.fn(async (method: string) => {
    if (method === METHODS.authStatus) return { authenticated: false };
    if (method === METHODS.providerUsage) return { windows: [] };
    if (method === METHODS.authSetApiKey) {
      return {
        provider: "opencode-go",
        saved: true,
        outcome: "verified",
        effective_source: "file",
        fetched_at: 1,
        models: [
          { id: "opencode-go/kimi-k3", verified: true, format: "anthropic" },
          { id: "opencode-go/m3", verified: false, format: "openai-compat" },
        ],
      };
    }
    return {};
  });
  return { request } as unknown as RpcClient;
}

test("defaults to the 连接 tab, uses the wide card and switches categories via the tablist", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  expect(document.querySelector(".aj-dialog-frame.aj-settings")).not.toBeNull();
  expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual([
    "连接",
    "显示",
    "项目",
    "演示数据",
    "用量",
    "订阅",
  ]);
  expect(screen.getByRole("tab", { name: "连接" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByLabelText("connection")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("tab", { name: "显示" }));
  expect(screen.getByRole("tab", { name: "显示" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "aj-settings-tab-display");
  expect(screen.queryByLabelText("connection")).toBeNull();

  fireEvent.click(screen.getByRole("tab", { name: "演示数据" }));
  expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "aj-settings-tab-demo");
});

test("ArrowDown moves tab selection, focus and the tabpanel", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  const connection = screen.getByRole("tab", { name: "连接" });
  connection.focus();
  fireEvent.keyDown(connection, { key: "ArrowDown" });
  const display = screen.getByRole("tab", { name: "显示" });
  expect(display).toHaveAttribute("aria-selected", "true");
  expect(document.activeElement).toBe(display);
  expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "aj-settings-tab-display");
});

test("shows the six categories and mounts both new panels", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  const tabs = screen.getAllByRole("tab").map((t) => t.textContent);
  expect(tabs).toEqual(["连接", "显示", "项目", "演示数据", "用量", "订阅"]);
  expect(screen.queryByText("会话列表")).toBeNull();
  fireEvent.click(screen.getByRole("tab", { name: "显示" }));
  expect(screen.getByRole("switch", { name: "减弱动画" })).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "项目" }));
  expect(screen.getByLabelText("默认项目")).toBeTruthy();
});

test("keeps keyboard roaming across six tabs", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  const first = screen.getByRole("tab", { name: "连接" });
  first.focus();
  fireEvent.keyDown(first, { key: "ArrowDown" });
  expect(document.activeElement?.textContent).toBe("显示");
  fireEvent.keyDown(document.activeElement as Element, { key: "End" });
  expect(document.activeElement?.textContent).toBe("订阅");
});

test("keeps wrap + Home/End via shared helpers", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  const connection = screen.getByRole("tab", { name: "连接" });
  connection.focus();
  fireEvent.keyDown(connection, { key: "End" });
  expect(document.activeElement).toHaveTextContent(/订阅/);
  fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" }); // wraps to first
  expect(document.activeElement).toHaveTextContent(/^连接$/);
});

test("demo toggle flips the flag and reflects state text", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} />);
  fireEvent.click(screen.getByRole("tab", { name: "演示数据" }));
  const toggle = screen.getByRole("switch");
  expect(toggle).toHaveAttribute("aria-checked", "false");
  expect(toggle).toHaveTextContent("已关闭");
  fireEvent.click(toggle);
  expect(localStorage.getItem("agentj.demo")).toBe("1");
  expect(toggle).toHaveAttribute("aria-checked", "true");
  expect(toggle).toHaveTextContent("已开启");
});

test("connection tab submits through ConnectionBar then closes the dialog", () => {
  localStorage.clear();
  const onConnect = vi.fn();
  const onClose = vi.fn();
  render(<SettingsDialog onConnect={onConnect} onClose={onClose} />);
  const bar = screen.getByLabelText("connection");
  fireEvent.change(within(bar).getByLabelText("Host"), { target: { value: "10.0.0.5" } });
  fireEvent.change(within(bar).getByLabelText("Port"), { target: { value: "5000" } });
  fireEvent.click(within(bar).getByRole("button", { name: /connect/i }));
  expect(onConnect).toHaveBeenCalledWith("10.0.0.5", "5000", "");
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("lists the subscription tab and reaches it from the keyboard", async () => {
  localStorage.clear();
  const c = { request: vi.fn().mockResolvedValue({}) } as unknown as RpcClient;
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} client={c} />);
  expect(screen.getAllByRole("tab").map((t) => t.textContent))
    .toEqual(["连接", "显示", "项目", "演示数据", "用量", "订阅"]);
  const demo = screen.getByRole("tab", { name: "演示数据" });
  demo.focus();
  fireEvent.keyDown(demo, { key: "ArrowDown" });
  expect(screen.getByRole("tab", { name: "用量" })).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
  expect(screen.getByRole("tab", { name: "订阅" })).toHaveAttribute("aria-selected", "true");
  await waitFor(() => expect(c.request).toHaveBeenCalled()); // SubscriptionPanel mounted with the client
});

test("renders a connect hint when no client exists", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} client={null} />);
  fireEvent.click(screen.getByRole("tab", { name: "订阅" }));
  expect(screen.getByText(/先连接内核/)).toBeInTheDocument();
});

test("forwards the linked catalog to its parent", async () => {
  localStorage.clear();
  const onModelsLinked = vi.fn();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}}
    client={stubClientWithVerifiedLink()} onModelsLinked={onModelsLinked} />);
  fireEvent.click(screen.getByRole("tab", { name: "订阅" }));
  fireEvent.change(screen.getByLabelText(/API Key/), { target: { value: "sk-x" } });
  fireEvent.click(screen.getByRole("button", { name: "链接" }));
  await waitFor(() => expect(onModelsLinked).toHaveBeenCalledTimes(1));
  expect(onModelsLinked).toHaveBeenCalledWith(expect.objectContaining({
    saved: true,
    outcome: "verified",
    models: [
      { id: "opencode-go/kimi-k3", verified: true, format: "anthropic" },
      { id: "opencode-go/m3", verified: false, format: "openai-compat" },
    ],
  }));
});

test("keeps the six categories while a callback is threaded through", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} client={null}
    onModelsLinked={() => {}} />);
  expect(screen.getAllByRole("tab").map((t) => t.textContent))
    .toEqual(["连接", "显示", "项目", "演示数据", "用量", "订阅"]);
  const demo = screen.getByRole("tab", { name: "演示数据" });
  demo.focus();
  fireEvent.keyDown(demo, { key: "ArrowDown" });
  expect(screen.getByRole("tab", { name: "用量" })).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
  expect(screen.getByRole("tab", { name: "订阅" })).toHaveAttribute("aria-selected", "true");
});

test("resolves every category to exactly one non-empty pane", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} client={stubClientWithVerifiedLink()} />);
  for (const tab of screen.getAllByRole("tab")) {
    fireEvent.click(tab);
    const panels = screen.getAllByRole("tabpanel");
    expect(panels).toHaveLength(1);
    expect(panels[0].getAttribute("aria-labelledby")).toBe(tab.id);
    expect((panels[0].textContent ?? "").trim().length).toBeGreaterThan(0);
  }
});

test("splits 用量 from 订阅 so each tab carries only its own half", async () => {
  localStorage.clear();
  const c = stubClientWithVerifiedLink();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}} client={c} />);
  fireEvent.click(screen.getByRole("tab", { name: "用量" }));
  await waitFor(() => expect(c.request).toHaveBeenCalledWith(METHODS.providerUsage, expect.anything()));
  // Usage half: the read-only usage block, no credential editing.
  expect(screen.getByRole("tabpanel").textContent).toContain("刷新用量");
  expect(screen.queryByLabelText(/API Key/)).toBeNull();

  fireEvent.click(screen.getByRole("tab", { name: "订阅" }));
  // Subscription half: the credential controls, no usage refresh button.
  expect(screen.getByLabelText(/API Key/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "刷新用量" })).toBeNull();
});

test("flags a missing credential on the 订阅 tab without renaming it", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}}
    client={stubClientWithVerifiedLink()} subscription={statusWith("invalid")} />);
  expect(screen.getByTestId("settings-nav-badge")).toBeInTheDocument();
  const tab = screen.getByRole("tab", { name: "订阅" });
  expect(tab).toHaveAttribute("title", "订阅 · 待配置凭据");
});

test("shows no credential badge once the account is valid", () => {
  localStorage.clear();
  render(<SettingsDialog onConnect={() => {}} onClose={() => {}}
    client={stubClientWithVerifiedLink()} subscription={statusWith("ok")} />);
  expect(screen.queryByTestId("settings-nav-badge")).toBeNull();
});
