/*
 * La documentazione tecnica dice il vero sul repository.
 *
 *   node --test "strumenti/*.test.js"
 */
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const { ancoreDelNucleo, caricati, carica, controlla, forma, chiuso, rimandi } = require("./docs.js");

const TUTTO_ESISTE = () => true;

function mini() {
  return [
    {
      id: "c",
      title: "C",
      entries: [
        {
          id: "uno",
          title: "Uno",
          src: ["a.rs"],
          body: ["p [due](#due)", { ul: ["a", "b"] }, { code: "x **" }, { table: { head: ["k", "v"], rows: [["a", "`b`"]] } }],
          see: ["due"],
        },
        { id: "due", title: "Due", src: ["b.rs"], body: ["p"] },
      ],
    },
  ];
}

test("la forma di un blocco", () => {
  assert.equal(forma("x"), "p");
  assert.equal(forma({ h: "x" }), "h");
  assert.equal(forma({ code: "x" }), "code");
  assert.equal(forma({ ul: ["a"] }), "ul");
  assert.equal(forma({ table: { head: ["a"], rows: [["b"]] } }), "table");
  assert.equal(forma({ ul: [] }), null);
  assert.equal(forma({ quote: "x" }), null);
  assert.equal(forma({ h: "x", note: "y" }), null);
});

test("grassetto e codice vanno chiusi", () => {
  assert.ok(chiuso("**a** e `b`"));
  assert.ok(!chiuso("**a e `b`"));
  assert.ok(!chiuso("`a"));
});

test("i rimandi interni si riconoscono", () => {
  assert.deepEqual(rimandi("vedi [qui](#uno) e [là](https://x.y)"), ["uno"]);
  // Un esempio di sintassi fra apici inversi non è un rimando.
  assert.deepEqual(rimandi("si scrive `[testo](#voce)`"), []);
});

test("una documentazione in ordine non ha problemi", () => {
  assert.deepEqual(controlla(mini(), ["uno", "due"], TUTTO_ESISTE), []);
});

test("il codice non passa dal controllo del grassetto", () => {
  // `x **` sta in un blocco di codice: non è un grassetto aperto.
  assert.deepEqual(controlla(mini(), [], TUTTO_ESISTE), []);
});

test("un percorso che non esiste si vede", () => {
  const p = controlla(mini(), [], (x) => x !== "b.rs");
  assert.equal(p.length, 1);
  assert.match(p[0], /b\.rs/);
});

test("una riga di tabella più corta si vede", () => {
  const c = mini();
  c[0].entries[0].body[3].table.rows.push(["solo"]);
  assert.equal(controlla(c, [], TUTTO_ESISTE).length, 1);
});

test("un rimando a una voce che non esiste si vede", () => {
  const c = mini();
  c[0].entries[1].see = ["fantasma"];
  c[0].entries[1].body = ["[x](#nessuno)"];
  assert.equal(controlla(c, [], TUTTO_ESISTE).length, 2);
});

test("un'ancora ammessa dal nucleo senza voce si vede", () => {
  assert.equal(controlla(mini(), ["manca"], TUTTO_ESISTE).length, 1);
});

test("un id ripetuto si vede", () => {
  const c = mini();
  c[0].entries[1].id = "uno";
  assert.ok(controlla(c, [], TUTTO_ESISTE).some((p) => /ripetuta/.test(p)));
});

test("la pagina carica i capitoli che dichiara", () => {
  const pagina = '<script src="./contenuto/a.js"></script>\n<script src="./docs.js"></script>';
  assert.deepEqual(caricati(pagina), ["contenuto/a.js"]);
});

test("la documentazione vera torna", () => {
  const { capitoli, pagina, mancanti, orfani } = carica();
  assert.deepEqual(mancanti, []);
  assert.deepEqual(orfani, []);
  const ancore = ancoreDelNucleo();
  assert.ok(ancore.length > 0);
  const repo = path.dirname(__dirname);
  assert.deepEqual(
    controlla(capitoli, ancore, (p) => fs.existsSync(path.join(repo, p)), pagina),
    [],
  );
});
