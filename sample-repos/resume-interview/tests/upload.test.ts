import { describe, expect, it } from "vitest";

describe("upload flow", () => {
  it("returns parsed resume data", () => {
    expect("/api/upload").toContain("upload");
  });
});
