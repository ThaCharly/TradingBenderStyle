import { describe, expect, it } from "vitest";
import { compatibility, parseVersion } from "./index";

describe("versionado", () => {
  it("acepta una menor de diferencia, como el servidor Rust", () => {
    const s = { major: 1, minor: 3 };
    expect(compatibility(s, { major: 1, minor: 3 })).toBe("same");
    expect(compatibility(s, { major: 1, minor: 4 })).toBe("minor-skew");
    expect(compatibility(s, { major: 1, minor: 1 })).toBe("incompatible");
    expect(compatibility(s, { major: 2, minor: 3 })).toBe("incompatible");
  });

  it("lee MAYOR.MENOR", () => {
    expect(parseVersion("1.0")).toEqual({ major: 1, minor: 0 });
    expect(parseVersion("1")).toBeUndefined();
    expect(parseVersion(null)).toBeUndefined();
  });
});
