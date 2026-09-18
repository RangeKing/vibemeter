import { describe, expect, it } from "vitest";
import { accountsSummary, combineBalances, edgeAgentQuotas } from "./quota";
import type { CreditBalance, ProviderAccount, ProviderUsage, SourceStatus } from "../types";

function balance(total: number, extra: Partial<CreditBalance> = {}): CreditBalance {
  return { currency: "CNY", total, provenance: "observed", ...extra };
}
function apiAccount(
  id: string,
  value: CreditBalance | undefined,
  available = true,
): ProviderAccount {
  return {
    id,
    provider: "deepseek",
    kind: "api",
    label: id,
    available,
    windows: [],
    balance: value,
  };
}
function source(agent: string, available = true): SourceStatus {
  return {
    agent,
    available,
    selected: true,
    capabilityLevel: "full",
    liveCapability: "exact",
    parserVersion: "1",
    sessionCount: 0,
    status: available ? "ready" : "not-found",
    warningCount: 0,
    pathLabel: agent,
  };
}

describe("multi-account quota", () => {
  it("adds up what several keys on one provider hold", () => {
    const combined = combineBalances([
      apiAccount("a", balance(110, { granted: 10, toppedUp: 100, spendable: true })),
      apiAccount("b", balance(40, { granted: 0, toppedUp: 40, spendable: true })),
    ]);
    expect(combined?.total).toBe(150);
    expect(combined?.granted).toBe(10);
    expect(combined?.toppedUp).toBe(140);
    expect(combined?.spendable).toBe(true);
  });

  it("leaves mixed currencies alone rather than summing them", () => {
    const combined = combineBalances([
      apiAccount("a", balance(20, { currency: "USD" })),
      apiAccount("b", balance(110, { currency: "CNY" })),
    ]);
    // The larger holding, reported in its own currency — 130 of nothing would
    // be a number the user could not act on.
    expect(combined).toMatchObject({ currency: "CNY", total: 110 });
  });

  it("surfaces one blocked key even while the total still looks healthy", () => {
    const combined = combineBalances([
      apiAccount("a", balance(500, { spendable: true })),
      apiAccount("b", balance(10, { spendable: false })),
    ]);
    expect(combined?.total).toBe(510);
    expect(combined?.spendable).toBe(false);
    expect(accountsSummary([
      apiAccount("a", balance(500, { spendable: true })),
      apiAccount("b", balance(10, { spendable: false })),
    ]).band).toBe("critical");
  });

  it("ignores an account that could not be read, rather than counting it as nothing", () => {
    const combined = combineBalances([
      apiAccount("a", balance(60)),
      apiAccount("b", undefined, false),
    ]);
    expect(combined?.total).toBe(60);
    expect(combineBalances([apiAccount("b", undefined, false)])).toBeUndefined();
    expect(accountsSummary([apiAccount("b", undefined, false)]).band).toBe("unknown");
  });

  it("reads an exhausted balance as a problem, not as missing data", () => {
    expect(accountsSummary([apiAccount("a", balance(0))]).band).toBe("critical");
    expect(accountsSummary([apiAccount("a", balance(12))]).band).toBe("steady");
  });

  it("hands every account of an Agent's provider to the card", () => {
    const provider: ProviderUsage = {
      provider: "deepseek",
      available: true,
      source: "api-key",
      windows: [],
      accounts: [apiAccount("a", balance(1)), apiAccount("b", balance(2))],
      health: { state: "operational", description: "", statusUrl: "" },
      stale: false,
    };
    const rings = edgeAgentQuotas([source("deepseek-harness")], [provider], undefined);
    expect(rings).toHaveLength(1);
    expect(rings[0].accounts.map((account) => account.id)).toEqual(["a", "b"]);
    expect(rings[0].summary.balance?.total).toBe(3);
  });
});
