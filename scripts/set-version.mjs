// Sets the app version in package.json, package-lock.json and src-tauri/Cargo.toml.
// tauri.conf.json reads its version from package.json, so it needs no edit.
//
//   npm run set-version -- 0.2.0
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("Usage: npm run set-version -- <major.minor.patch>");
  process.exit(1);
}

execFileSync("npm", ["version", version, "--no-git-tag-version", "--allow-same-version"], {
  stdio: "inherit",
  shell: process.platform === "win32",
});

const cargoPath = "src-tauri/Cargo.toml";
const cargo = readFileSync(cargoPath, "utf8");
const updated = cargo.replace(/^version = "[^"]*"/m, `version = "${version}"`);
if (updated === cargo && !cargo.includes(`version = "${version}"`)) {
  console.error(`Could not find the package version in ${cargoPath}`);
  process.exit(1);
}
writeFileSync(cargoPath, updated);

console.log(`Version set to ${version}. Commit, then tag v${version} and push the tag to release.`);
