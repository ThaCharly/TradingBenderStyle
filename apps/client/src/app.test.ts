import { describe, expect, it } from "vitest";
import { appTitle } from "./app";

describe("appTitle", () => {
  it("devuelve el nombre del proyecto", () => {
    expect(appTitle()).toBe("TradingBenderStyle");
  });
});
