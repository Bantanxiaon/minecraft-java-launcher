import packageJson from "../package.json";

export const APP_VERSION = packageJson.version;

export const RELEASE_CHANNEL: "beta" | "stable" = /^0\./.test(APP_VERSION)
  ? "beta"
  : "stable";

export const RELEASE_CHANNEL_LABEL =
  RELEASE_CHANNEL === "beta" ? "Beta 测试版" : "正式版";
