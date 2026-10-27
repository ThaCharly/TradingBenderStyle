import { describe, expect, it } from "vitest";
import { SCENE_MODEL_VERSION } from "./index";

describe("chart-scene", () => {
  it("expone la versión del modelo de escena", () => {
    expect(SCENE_MODEL_VERSION).toBe(0);
  });
});
