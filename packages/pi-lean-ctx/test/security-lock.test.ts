import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const packageRoot = resolve(__dirname, "..");

interface PackageManifest {
  overrides?: Record<string, string>;
}

interface PackageLock {
  packages?: Record<string, { version?: string }>;
}

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, "utf8")) as T;
}

function isAtLeast(version: string, minimum: [number, number, number]): boolean {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
  if (!match) return false;
  const actual = match.slice(1).map(Number);
  for (let index = 0; index < minimum.length; index += 1) {
    if (actual[index] > minimum[index]) return true;
    if (actual[index] < minimum[index]) return false;
  }
  return true;
}

describe("security dependency lock", () => {
  it("keeps @hono/node-server at or above the patched version", () => {
    const manifest = readJson<PackageManifest>(resolve(packageRoot, "package.json"));
    const lock = readJson<PackageLock>(resolve(packageRoot, "package-lock.json"));
    const honoVersion = lock.packages?.["node_modules/@hono/node-server"]?.version ?? "";

    expect(manifest.overrides?.["@hono/node-server"]).toBe("^2.0.5");
    expect(honoVersion).toMatch(/^\d+\.\d+\.\d+$/);
    expect(isAtLeast(honoVersion, [2, 0, 5])).toBe(true);
  });
});
