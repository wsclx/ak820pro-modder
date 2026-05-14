/**
 * Single source of truth for the app's version + author string.
 * Update this when bumping `package.json` and `Cargo.toml` `[workspace.package]`
 * — keep them in lockstep.
 */

export const APP_VERSION = "0.7.0-beta";
export const APP_AUTHOR = "wsclx";
export const APP_HOMEPAGE = "https://github.com/wsclx/ak820pro-modder";

/** Compact one-line credit shown in the sidebar footer + About dialog. */
export const APP_CREDIT = `v${APP_VERSION} · by ${APP_AUTHOR}`;
