// Sets the app version in package.json, package-lock.json, src-tauri/Cargo.toml and Cargo.lock.
// tauri.conf.json reads its version from package.json, so it needs no edit.
//
//   npm run set-version -- 0.2.0
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("Usage: npm run set-version -- <major.minor.patch>");
  process.exit(1);
}

// `version` is validated above, so it is safe in a shell command line (npm is a .cmd on Windows).
execSync(`npm version ${version} --no-git-tag-version --allow-same-version`, { stdio: "inherit" });

const cargoPath = "src-tauri/Cargo.toml";
const cargo = readFileSync(cargoPath, "utf8");
const updated = cargo.replace(/^version = "[^"]*"/m, `version = "${version}"`);
if (updated === cargo && !cargo.includes(`version = "${version}"`)) {
  console.error(`Could not find the package version in ${cargoPath}`);
  process.exit(1);
}
writeFileSync(cargoPath, updated);

// Refresh Cargo.lock so the root package entry carries the new version too.
try {
  execSync("cargo metadata --format-version 1 --manifest-path src-tauri/Cargo.toml", { stdio: "ignore" });
} catch {
  console.warn("cargo not found; Cargo.lock will be updated on the next cargo build.");
}

console.log(`Version set to ${version}. Commit, then tag v${version} and push the tag to release.`);
