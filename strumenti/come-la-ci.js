/*
 * Le prove di Rust come le fa la CI, sul computer di chi sviluppa.
 *
 * `npm run verify` usa la toolchain di `rust-toolchain.toml` — `stable`, cioè
 * quella del giorno — e niente `-D warnings`; la CI la 1.97.1 inchiodata in
 * `verify.yml`, con gli avvisi che diventano errori. Due verdi diversi: con la
 * 1.99 in locale c'erano quattro avvisi che la CI non vedeva ancora, e che il
 * giorno in cui la si alza la fanno diventare rossa. Questo script legge la
 * versione da `verify.yml` (un posto solo) e lancia fmt, clippy e prove con
 * quella, con `RUSTFLAGS=-D warnings` e `--locked`, come il lavoro `verifica`.
 *
 *   npm run verify:ci          (da apps/desktop)
 *
 * La toolchain va installata una volta:
 *   rustup toolchain install <versione> --profile minimal --component rustfmt,clippy
 */
const { spawnSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const REPO = path.dirname(__dirname);

/** La toolchain del lavoro `verifica` di `verify.yml`. */
function toolchainDellaCi(yml) {
  const m = /^\s+RUSTUP_TOOLCHAIN: "([^"]+)"/m.exec(yml);
  if (!m) throw new Error("verify.yml: non trovo RUSTUP_TOOLCHAIN");
  return m[1];
}

/** Le opzioni del compilatore che la CI mette nell'ambiente di tutto il workflow. */
function rustflagsDellaCi(yml) {
  const m = /^\s+RUSTFLAGS: "([^"]*)"/m.exec(yml);
  if (!m) throw new Error("verify.yml: non trovo RUSTFLAGS");
  return m[1];
}

/** I comandi di Rust del lavoro `verifica`, nello stesso ordine. */
const COMANDI = [
  ["fmt", "--all", "--check"],
  ["clippy", "--workspace", "--all-targets", "--locked"],
  ["test", "--workspace", "--locked"],
];

function main() {
  const yml = fs.readFileSync(path.join(REPO, ".github/workflows/verify.yml"), "utf8");
  const toolchain = toolchainDellaCi(yml);
  const env = { ...process.env, RUSTUP_TOOLCHAIN: toolchain, RUSTFLAGS: rustflagsDellaCi(yml) };
  const presente = spawnSync("rustup", ["run", toolchain, "cargo", "--version"], { env, encoding: "utf8" });
  if (presente.status !== 0) {
    console.error(
      `Manca la toolchain ${toolchain} della CI. Si installa una volta con:\n` +
        `  rustup toolchain install ${toolchain} --profile minimal --component rustfmt,clippy`,
    );
    process.exit(1);
  }
  for (const argomenti of COMANDI) {
    console.log(`\n> RUSTUP_TOOLCHAIN=${toolchain} RUSTFLAGS="${env.RUSTFLAGS}" cargo ${argomenti.join(" ")}`);
    const esito = spawnSync("cargo", argomenti, { cwd: REPO, env, stdio: "inherit" });
    if (esito.status !== 0) process.exit(esito.status ?? 1);
  }
  console.log(`\nRust come in CI (${toolchain}, ${env.RUSTFLAGS}): verde.`);
}

if (require.main === module) main();

module.exports = { toolchainDellaCi, rustflagsDellaCi, COMANDI };
