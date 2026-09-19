import { readFileSync } from "node:fs";

const packageVersion = JSON.parse(readFileSync("package.json", "utf8")).version;
const tauri = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const cargo = readFileSync("src-tauri/Cargo.toml", "utf8");
const section = cargo.split("[package]")[1]?.split(/^\[/m)[0];
const cargoVersion = section?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (
  !packageVersion ||
  packageVersion !== tauri.version ||
  packageVersion !== cargoVersion
) {
  throw new Error("package.json、Tauri 和 Cargo 的版本必须一致");
}
const ref = process.env.GITHUB_REF ?? "";
if (ref.startsWith("refs/tags/")) {
  const tag = ref.slice("refs/tags/".length);
  if (tag !== `v${packageVersion}`) {
    throw new Error(`发布 tag ${tag} 与应用版本 ${packageVersion} 不一致`);
  }
}
console.log(`版本检查通过：${packageVersion}`);
