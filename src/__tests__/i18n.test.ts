import i18next from "i18next";
import { beforeEach, expect, test, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import en from "../i18n/en.json";
import { initI18n, resolveLanguage } from "../i18n";
import zhCN from "../i18n/zh-CN.json";

const keys = (value: object, prefix = ""): string[] =>
  Object.entries(value).flatMap(([key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return child && typeof child === "object" ? keys(child, path) : path;
  });

beforeEach(() => {
  invokeMock.mockReset();
});

test("resolves only missing and system settings from navigator language", () => {
  expect(resolveLanguage("system", "zh-Hans-CN")).toBe("zh-CN");
  expect(resolveLanguage(undefined, "en-US")).toBe("en");
});

test("returns explicit supported languages unchanged", () => {
  expect(resolveLanguage("zh-CN", "en-US")).toBe("zh-CN");
  expect(resolveLanguage("en", "zh-CN")).toBe("en");
});

test.each(["", "fr", null, 42])("rejects invalid language setting %j", (setting) => {
  expect(() => resolveLanguage(setting, "zh-CN")).toThrow(
    "Invalid language setting",
  );
});

test("initializes from the settings record before rendering", async () => {
  invokeMock.mockResolvedValue({ language: "en" });

  await initI18n();

  expect(invokeMock).toHaveBeenCalledWith("get_settings");
  expect(i18next.language).toBe("en");
});

test("rejects when get_settings fails", async () => {
  const failure = new Error("settings unavailable");
  invokeMock.mockRejectedValue(failure);

  await expect(initI18n()).rejects.toBe(failure);
});

test("rejects an invalid language returned by get_settings", async () => {
  invokeMock.mockResolvedValue({ language: "fr" });

  await expect(initI18n()).rejects.toThrow("Invalid language setting");
});

test("Chinese and English resources have identical key sets", () => {
  expect(keys(zhCN).sort()).toEqual(keys(en).sort());
});
