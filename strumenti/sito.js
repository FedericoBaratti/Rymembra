/*
 * Il sito, costruito da un sorgente solo.
 *
 * # Cosa c'è da dove
 *
 * `site/` è il sorgente: il modello della pagina (`pagina.html`), i suoi testi
 * in quattro lingue (`testi/<lingua>.json`), lo stile e il poco di JavaScript, e
 * i capitoli della documentazione (`docs/contenuto/*.js`, gli stessi che
 * controlla `strumenti/docs.js`, nell'ordine di `docs/indice.json`). Da lì esce
 * `_site/`, che è quello che `.github/workflows/sito.yml` pubblica e che non si
 * versiona: si ricostruisce ogni volta.
 *
 * # Perché costruire invece di scrivere a mano
 *
 * Quattro lingue scritte a mano sono quattro pagine che divergono alla prima
 * modifica; la documentazione disegnata nel browser è una pagina che i motori
 * di ricerca leggono male. Qui la pagina è una, le lingue sono dati, e ogni
 * capitolo delle docs diventa HTML vero con il suo indirizzo. Il browser riceve
 * pagine finite: il JavaScript del sito aggiunge movimento e ricerca, ma senza
 * si legge tutto.
 *
 * # La versione che annuncia
 *
 * Il sito dice «scarica la versione X» e linka l'installer di X: deve essere
 * una release **pubblicata**, non la versione a cui è arrivato il codice. Per
 * questo `sito.yml` passa `--versione` con il tag della release; da sola, cioè
 * in `verify`, la costruzione usa quella di `apps/desktop/package.json`. Data,
 * nome dell'installer e indirizzo si ricavano da lì: il `CHANGELOG.md` deve
 * avere la voce di quella versione.
 *
 * Con `--installer` (la cartella in cui `sito.yml` ha scaricato l'installer
 * della release) la pagina dice anche quanto pesa e la sua impronta SHA-256,
 * misurati sul file vero. Senza, cioè offline, quella riga non c'è.
 *
 * # I controlli
 *
 * Ogni costruzione finisce con i controlli, così un sito rotto non si
 * pubblica: le quattro lingue hanno le stesse chiavi, ogni segnaposto è
 * risolto e ogni testo è usato, ogni collegamento interno porta a un file e a
 * un'ancora che esistono, ogni ancora che l'app apre (`ANCORE_GUIDA`) arriva a
 * una voce, e nessuna pagina carica qualcosa da un altro dominio.
 *
 *   node strumenti/sito.js                     costruisce _site/
 *   node strumenti/sito.js --versione v3.0.1   la stessa cosa, annunciando quella release
 *   node strumenti/sito.js --versione v3.0.1 --installer <cartella>   e con peso e impronta dell'installer
 *   node strumenti/sito.js --controlla         costruisce in una cartella temporanea, controlla, butta via
 */
const crypto = require("crypto");
const fs = require("fs");
const os = require("os");
const path = require("path");
const docs = require("./docs.js");

// I percorsi si risolvono rispetto a questo file e non alla cartella da cui lo
// si lancia: gli script npm girano da `apps/desktop`.
const REPO = path.dirname(__dirname);
const SORGENTE = path.join(REPO, "site");
const USCITA = path.join(REPO, "_site");

/** Il dominio del sito: serve agli indirizzi assoluti (canonical, hreflang, Open Graph, sitemap). */
const DOMINIO = "https://rymembra.com";

/**
 * La scheda di Rymembra nel Microsoft Store: l'ID è quello di Partner Center
 * (Identità del prodotto). Senza parametri di lingua, perché il Store sceglie da
 * sé quella di chi lo apre.
 */
const STORE = "https://apps.microsoft.com/detail/9NW2F2PW2B91";

/** La prima è quella della radice, ed è anche `x-default`. */
const LINGUE = [
  { codice: "en", cartella: "", locale: "en_US", nome: "English" },
  { codice: "it", cartella: "it/", locale: "it_IT", nome: "Italiano" },
  { codice: "fr", cartella: "fr/", locale: "fr_FR", nome: "Français" },
  { codice: "de", cartella: "de/", locale: "de_DE", nome: "Deutsch" },
];

/* ── il testo ─────────────────────────────────────────────────────────── */

function esc(s) {
  return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/**
 * Il marcatore in linea delle docs, uguale per le pagine: `**grassetto**`,
 * `*corsivo*`, apici inversi per il codice, `[testo](indirizzo)`. Niente HTML
 * nei testi: tutto passa da `esc`.
 */
const PEZZI = /(\*\*[^*]+\*\*|\*[^*\s][^*]*\*|`[^`]+`|\[[^\]]+\]\([^)\s]+\))/g;

/** Un indirizzo che un testo può linkare: https, mailto, un'ancora o un percorso relativo. */
function indirizzoAmmesso(h) {
  return /^(https:\/\/|mailto:|#)/.test(h) || !/[:\\]/.test(h);
}

function inline(testo, collega = (h) => h) {
  return String(testo)
    .split(PEZZI)
    .map((p) => {
      if (p === "") return "";
      if (p.length > 4 && p.startsWith("**") && p.endsWith("**")) return `<strong>${esc(p.slice(2, -2))}</strong>`;
      if (p.length > 2 && p.startsWith("`") && p.endsWith("`")) return `<code>${esc(p.slice(1, -1))}</code>`;
      const l = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(p);
      if (l) {
        if (!indirizzoAmmesso(l[2])) throw new Error(`indirizzo non ammesso in un testo: ${l[2]}`);
        return `<a href="${esc(collega(l[2]))}">${esc(l[1])}</a>`;
      }
      if (p.length > 2 && p.startsWith("*") && p.endsWith("*")) return `<em>${esc(p.slice(1, -1))}</em>`;
      return esc(p);
    })
    .join("");
}

/** Lo stesso testo senza marcatori: per i `<meta>`, i titoli e la ricerca. */
function piano(testo) {
  return String(testo)
    .replace(/\[([^\]]+)\]\([^)\s]+\)/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\*([^*\s][^*]*)\*/g, "$1");
}

/* ── il modello ───────────────────────────────────────────────────────── */

/**
 * Riempie un modello.
 *
 *   {{chiave}}          testo, con il marcatore in linea
 *   {{=chiave}}         testo semplice: per gli attributi
 *   {{&$chiave}}        HTML già fatto, solo dai valori del generatore
 *   {{#elenco}}…{{/elenco}}  una volta per elemento; dentro, `.campo` è il
 *                       campo dell'elemento, `.` l'elemento stesso, `@n` il suo
 *                       numero da 1
 *
 * Le chiavi sono percorsi nei testi (`hero.titolo`) o valori del generatore
 * (`$versione`); dentro un testo, `{versione}` vale `$versione`. Una chiave
 * che manca è un problema, non una stringa vuota in silenzio. `usate` raccoglie
 * le chiavi lette, perché il controllo trovi i testi che nessuno mostra.
 */
function riempi(modello, testi, valori, problemi, usate = new Set()) {
  const valore = (chiave, elemento, nomeElenco) => {
    if (chiave === ".") return elemento;
    if (chiave.startsWith(".")) {
      usate.add(`${nomeElenco}[].${chiave.slice(1)}`);
      return elemento == null ? undefined : elemento[chiave.slice(1)];
    }
    if (chiave.startsWith("$")) return valori[chiave];
    usate.add(chiave);
    return chiave.split(".").reduce((o, k) => (o == null ? undefined : o[k]), testi);
  };
  const interpola = (s, chiave) =>
    s.replace(/\{(\w+)\}/g, (tutto, nome) => {
      const v = valori["$" + nome];
      if (v === undefined) {
        problemi.push(`«${chiave}»: {${nome}} non è un valore del generatore`);
        return tutto;
      }
      return v;
    });
  const sostituisci = (testo, elemento, nomeElenco, numero) =>
    testo.replace(/\{\{([=&]?)([$.@][\w.-]*|[\w-]+(?:\.[\w-]+)*)\}\}/g, (tutto, modo, chiave) => {
      if (chiave === "@n") return numero === undefined ? tutto : String(numero);
      const dentro = chiave.startsWith(".");
      if (dentro && elemento === undefined) return tutto;
      if (!dentro && elemento !== undefined) return tutto; // lo risolve il giro di fuori
      const v = valore(chiave, elemento, nomeElenco);
      if (typeof v !== "string") {
        problemi.push(`segnaposto «${chiave}»${nomeElenco ? ` in «${nomeElenco}»` : ""}: ${v === undefined ? "manca" : "non è un testo"}`);
        return "";
      }
      if (modo === "&") {
        if (!chiave.startsWith("$")) problemi.push(`«${chiave}»: l'HTML grezzo viene solo dal generatore`);
        return v;
      }
      const s = chiave.startsWith("$") ? v : interpola(v, chiave);
      return modo === "=" ? esc(piano(s)) : inline(s);
    });

  const conElenchi = modello.replace(/\{\{#([\w.-]+)\}\}([\s\S]*?)\{\{\/\1\}\}/g, (tutto, nome, dentro) => {
    const elenco = valore(nome);
    if (!Array.isArray(elenco)) {
      problemi.push(`elenco «${nome}»: ${elenco === undefined ? "manca" : "non è un elenco"}`);
      return "";
    }
    return elenco.map((e, i) => sostituisci(dentro, e, nome, i + 1)).join("");
  });
  return sostituisci(conElenchi);
}

/**
 * Le foglie di un documento di testi, ognuna con la sua forma: `a.b` per un
 * testo, `a.b[]` per un elenco (con la lunghezza e i campi dei suoi elementi).
 */
function foglie(oggetto, prefisso = "", fuori = new Map()) {
  for (const [k, v] of Object.entries(oggetto)) {
    const p = prefisso ? `${prefisso}.${k}` : k;
    if (Array.isArray(v)) {
      const campi = new Set();
      for (const e of v) if (e && typeof e === "object") Object.keys(e).forEach((c) => campi.add(c));
      fuori.set(`${p}[]`, { lunghezza: v.length, campi: [...campi].sort() });
    } else if (v && typeof v === "object") foglie(v, p, fuori);
    else fuori.set(p, { tipo: typeof v });
  }
  return fuori;
}

/** Le lingue dicono le stesse cose: stesse chiavi, elenchi lunghi uguali, elementi con gli stessi campi. */
function confrontaLingue(testi) {
  const problemi = [];
  const [prima, ...altre] = Object.keys(testi);
  const riferimento = foglie(testi[prima]);
  for (const lingua of altre) {
    const queste = foglie(testi[lingua]);
    for (const [k, forma] of riferimento) {
      const q = queste.get(k);
      if (!q) problemi.push(`${lingua}: manca «${k}»`);
      else if (forma.lunghezza !== q.lunghezza) problemi.push(`${lingua}: «${k}» ha ${q.lunghezza} elementi invece di ${forma.lunghezza}`);
      else if (forma.campi && forma.campi.join() !== q.campi.join()) problemi.push(`${lingua}: gli elementi di «${k}» hanno campi diversi`);
    }
    for (const k of queste.keys()) if (!riferimento.has(k)) problemi.push(`${lingua}: «${k}» non c'è in ${prima}`);
  }
  for (const [lingua, t] of Object.entries(testi)) {
    for (const [k, v] of foglie(t)) {
      if (v.tipo && v.tipo !== "string") problemi.push(`${lingua}: «${k}» dev'essere un testo`);
    }
  }
  return problemi;
}

/** Le chiavi che nessun modello mostra: un testo tradotto quattro volte e mai letto. */
function nonUsate(testi, usate) {
  const fuori = [];
  for (const [k, forma] of foglie(testi)) {
    if (forma.campi && forma.campi.length) {
      for (const c of forma.campi) if (!usate.has(`${k}.${c}`)) fuori.push(`${k}.${c}`);
    } else if (!usate.has(k.replace(/\[\]$/, ""))) fuori.push(k);
  }
  return fuori;
}

/* ── il repository ────────────────────────────────────────────────────── */

function leggi(rel) {
  return fs.readFileSync(path.join(REPO, rel), "utf8");
}

/** Il campo `nome` di `[workspace.package]` nel `Cargo.toml` di radice. */
function campoDelWorkspace(nome) {
  const blocco = leggi("Cargo.toml").split("[workspace.package]")[1] || "";
  const m = new RegExp(`^${nome}\\s*=\\s*"([^"]+)"`, "m").exec(blocco);
  if (!m) throw new Error(`manca ${nome} in [workspace.package] di Cargo.toml`);
  return m[1];
}

/** Il primo nome di `authors` in `[workspace.package]`. */
function autore() {
  const blocco = leggi("Cargo.toml").split("[workspace.package]")[1] || "";
  const m = /^authors\s*=\s*\[\s*"([^"]+)"/m.exec(blocco);
  if (!m) throw new Error("manca authors in [workspace.package] di Cargo.toml");
  return m[1];
}

/**
 * Le lingue in cui parla il programma, che non sono quelle del sito: l'elenco
 * è il contenuto di `apps/desktop/src/lingue/`, come per il programma stesso.
 */
function lingueDelProgramma() {
  return fs
    .readdirSync(path.join(REPO, "apps/desktop/src/lingue"))
    .filter((f) => f.endsWith(".json"))
    .map((f) => f.slice(0, -".json".length));
}

/** La data della voce di una versione nel changelog, `AAAA-MM-GG`. */
function dataNelChangelog(changelog, versione) {
  const v = versione.replace(/\./g, "\\.");
  const m = new RegExp(`^## \\[${v}\\] — (\\d{4}-\\d{2}-\\d{2})\\s*$`, "m").exec(changelog);
  return m ? m[1] : null;
}

function dataInLingua(data, locale) {
  const [a, m, g] = data.split("-").map(Number);
  return new Intl.DateTimeFormat(locale.replace("_", "-"), {
    day: "numeric",
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  }).format(new Date(Date.UTC(a, m - 1, g)));
}

/** Quello che la pagina annuncia, ricavato da una versione e da quello che il repository dice di lei. */
function rilascio(versione, cartellaInstaller = null) {
  if (!/^\d+\.\d+\.\d+$/.test(versione)) throw new Error(`versione «${versione}»: servono tre numeri`);
  const data = dataNelChangelog(leggi("CHANGELOG.md"), versione);
  if (!data) throw new Error(`il CHANGELOG.md non ha la voce «## [${versione}] — AAAA-MM-GG»`);
  const repo = campoDelWorkspace("repository");
  const prodotto = JSON.parse(leggi("apps/desktop/src-tauri/tauri.conf.json")).productName;
  // Il nome che il bundler NSIS di Tauri dà all'installer, ed è quello degli
  // asset pubblicati (`Rymembra_3.0.1_x64-setup.exe`): `release.yml` ne
  // pretende uno solo.
  const file = `${prodotto}_${versione}_x64-setup.exe`;
  return {
    versione,
    data,
    repo,
    file,
    scarica: `${repo}/releases/download/v${versione}/${file}`,
    note: `${repo}/releases/tag/v${versione}`,
    installer: cartellaInstaller ? misuraInstaller(path.join(cartellaInstaller, file)) : null,
  };
}

/** Peso e impronta del file che il pulsante scarica: si misurano sul file, non si scrivono a mano. */
function misuraInstaller(file) {
  if (!fs.existsSync(file)) throw new Error(`l'installer «${file}» non c'è`);
  const dati = fs.readFileSync(file);
  // In maiuscolo, come la stampa `Get-FileHash`: chi le confronta a occhio deve vedere la stessa stringa.
  return { byte: dati.length, sha256: crypto.createHash("sha256").update(dati).digest("hex").toUpperCase() };
}

/** Il peso come lo scrive Esplora risorse: in MB da 1024 × 1024 byte, con una cifra decimale, nei numeri della lingua. */
function pesoInLingua(byte, locale) {
  return new Intl.NumberFormat(locale.replace("_", "-"), { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(byte / 1048576);
}

/* ── la documentazione ────────────────────────────────────────────────── */

/** L'indirizzo di un capitolo: `data-chapter` e `studio-chapter` esistono come id perché `data` e `studio` sono voci. */
function percorsoCapitolo(id) {
  return id.replace(/-chapter$/, "");
}

/** Voce o capitolo → la pagina del capitolo che lo contiene. */
function mappaDocs(capitoli) {
  const mappa = {};
  for (const c of capitoli) {
    mappa[`c-${c.id}`] = percorsoCapitolo(c.id);
    for (const v of c.entries) mappa[v.id] = percorsoCapitolo(c.id);
  }
  return mappa;
}

function tavola(t, collega) {
  const th = t.head.map((c) => `<th scope="col">${inline(c, collega)}</th>`).join("");
  const righe = t.rows.map((r) => `<tr>${r.map((c) => `<td>${inline(c, collega)}</td>`).join("")}</tr>`).join("\n");
  return `<div class="tavola"><table><thead><tr>${th}</tr></thead><tbody>\n${righe}\n</tbody></table></div>`;
}

function blocco(b, collega) {
  if (typeof b === "string") return `<p>${inline(b, collega)}</p>`;
  if (b.h !== undefined) return `<h3>${inline(b.h, collega)}</h3>`;
  if (b.note !== undefined) return `<p class="nota">${inline(b.note, collega)}</p>`;
  if (b.code !== undefined) return `<pre><code>${esc(b.code)}</code></pre>`;
  if (b.table !== undefined) return tavola(b.table, collega);
  const tag = b.ol ? "ol" : "ul";
  return `<${tag}>${(b.ol || b.ul).map((r) => `<li>${inline(r, collega)}</li>`).join("")}</${tag}>`;
}

/** Il testo di una voce, tutto di fila e senza marcatori, per la ricerca. */
function testoDellaVoce(voce) {
  const pezzi = [voce.title, ...voce.src];
  for (const b of voce.body) {
    if (typeof b === "string") pezzi.push(b);
    else if (b.code !== undefined) pezzi.push(b.code);
    else pezzi.push(...docs.testiInLinea(b));
  }
  return piano(pezzi.join(" ")).replace(/\s+/g, " ").toLowerCase();
}

/* ── il sito ──────────────────────────────────────────────────────────── */

const MARCHIO = `<svg class="marchio-segno" width="22" height="22" viewBox="0 0 20 20" fill="none" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M10 2.4a7.6 7.6 0 0 1 0 15.2" stroke="#8b7cf6"/><path d="M10 17.6a7.6 7.6 0 0 1 0-15.2" stroke="#4fd6e0"/></svg>`;

/* Il disco dietro l'apertura: i solchi li disegna il CSS, l'etichetta è questo
 * SVG, in un quadrato di 200 unità, con l'icona del programma. Quando la musica
 * suona il disco gira, tutto intero: niente dentro cambia, e la rotazione la fa
 * la scheda video senza ridipingere.
 *
 * Nei solchi c'era il testo del JSON dell'apertura, 2300 lettere lungo una
 * spirale: al telefono il browser lo impaginava e lo dipingeva mentre la
 * pagina si caricava, e Lighthouse contava 700 ms di blocco in più. */
const VINILE =
  `<span class="vinile"><span class="v-disco">` +
  `<svg class="v-faccia" viewBox="0 0 200 200"><circle class="v-etichetta" cx="100" cy="100" r="34"/>` +
  // Le due metà del marchio, come in `MARCHIO` ma divise in orizzontale: il
  // riquadro del JSON copre la metà sinistra, e da fermo si vedono tutte e due.
  `<path d="M77.2 100a22.8 22.8 0 0 1 45.6 0" stroke="#8b7cf6"/><path d="M122.8 100a22.8 22.8 0 0 1-45.6 0" stroke="#4fd6e0"/>` +
  `<circle class="v-foro" cx="100" cy="100" r="2.4"/></svg></span><span class="v-luce"></span></span>`;

const PLAY = `<svg class="t-play" viewBox="0 0 20 20" aria-hidden="true"><path d="M6 3.8v12.4a.8.8 0 0 0 1.2.7l10-6.2a.8.8 0 0 0 0-1.4l-10-6.2A.8.8 0 0 0 6 3.8Z"/></svg>`;
const PAUSA = `<svg class="t-pausa" viewBox="0 0 20 20" aria-hidden="true"><rect x="4.5" y="3.5" width="4" height="13" rx="1"/><rect x="11.5" y="3.5" width="4" height="13" rx="1"/></svg>`;

function jsonLd(oggetto) {
  // `</script>` dentro un testo chiuderebbe il blocco: `<` si scrive escapato.
  return JSON.stringify(oggetto).replace(/</g, "\\u003c");
}

function urlLingua(l) {
  return `${DOMINIO}/${l.cartella}`;
}

/** Le pagine della landing, una per lingua. */
/** Sotto il pulsante finale: peso e impronta dell'installer, e come controllarla. Solo se il generatore ha il file. */
const IMPRONTA =
  '<details class="impronta"><summary>{{scarica.impronta.sommario}}</summary>' +
  '<p class="hash"><code>{{=$sha256}}</code></p><p>{{scarica.impronta.verifica}}</p></details>';

function landing(r, testi, problemi, usate) {
  const modello = fs.readFileSync(path.join(SORGENTE, "pagina.html"), "utf8");
  const pagine = {};
  const alternative =
    LINGUE.map((l) => `<link rel="alternate" hreflang="${l.codice}" href="${urlLingua(l)}">`).join("\n") +
    `\n<link rel="alternate" hreflang="x-default" href="${urlLingua(LINGUE[0])}">`;

  for (const l of LINGUE) {
    const t = testi[l.codice];
    const base = l.cartella ? "../" : "";
    const lingue = LINGUE.map(
      (x) =>
        `<a href="${base + x.cartella || "./"}" hreflang="${x.codice}" lang="${x.codice}" data-lingua="${x.codice}"` +
        ` title="${esc(x.nome)}"${x === l ? ' aria-current="page"' : ""}>${x.codice.toUpperCase()}</a>`,
    ).join("");
    // Solo sulla radice, che è quella in cui arriva chi non ha scelto: una
    // striscia per lingua, nella lingua che offre. `sito.js` mostra quella del
    // browser, se c'è; nessun redirect automatico.
    usate.add("lingua.suggerimento").add("lingua.chiudi");
    const suggerimenti =
      l === LINGUE[0]
        ? `<aside class="suggerimento" data-suggerimento hidden>${LINGUE.slice(1)
            .map(
              (x) =>
                `<p lang="${x.codice}" data-per="${x.codice}"><a href="${x.cartella}" data-lingua="${x.codice}">${esc(piano(testi[x.codice].lingua.suggerimento))}</a>` +
                `<button type="button" data-chiudi aria-label="${esc(testi[x.codice].lingua.chiudi)}">×</button></p>`,
            )
            .join("")}</aside>`
        : "";
    const ld = {
      "@context": "https://schema.org",
      "@graph": [
        {
          "@type": "SoftwareApplication",
          name: "Rymembra",
          url: urlLingua(l),
          description: piano(t.meta.descrizione),
          inLanguage: lingueDelProgramma(),
          operatingSystem: "Windows",
          applicationCategory: "MultimediaApplication",
          softwareVersion: r.versione,
          datePublished: r.data,
          downloadUrl: r.scarica,
          releaseNotes: r.note,
          license: "https://opensource.org/licenses/MIT",
          isAccessibleForFree: true,
          offers: { "@type": "Offer", price: "0", priceCurrency: "EUR" },
          author: { "@type": "Person", name: autore() },
          screenshot: `${DOMINIO}/immagini/casa.webp`,
          image: `${DOMINIO}/anteprima.jpg`,
          sameAs: [r.repo],
        },
        { "@type": "WebSite", name: "Rymembra", url: `${DOMINIO}/`, inLanguage: LINGUE.map((x) => x.codice) },
      ],
    };
    const valori = {
      $lingua: l.codice,
      $base: base,
      $canonico: urlLingua(l),
      $alternative: alternative,
      $ogLocale: l.locale,
      $ogAlternative: LINGUE.filter((x) => x !== l)
        .map((x) => `<meta property="og:locale:alternate" content="${x.locale}">`)
        .join("\n"),
      $lingue: lingue,
      $suggerimenti: suggerimenti,
      $jsonld: jsonLd(ld),
      $marchio: MARCHIO,
      $vinile: VINILE,
      $play: PLAY,
      $pausa: PAUSA,
      $dominio: DOMINIO,
      $versione: r.versione,
      $data: dataInLingua(r.data, l.locale),
      $file: r.file,
      $scarica: r.scarica,
      $store: STORE,
      $verifica: "",
      $note: r.note,
      $tutte: `${r.repo}/releases`,
      $repo: r.repo,
      $changelog: `${r.repo}/blob/main/CHANGELOG.md`,
      $privacy: `${r.repo}/blob/main/PRIVACY.md`,
      $termini: `${r.repo}/blob/main/TERMS.md`,
      $sicurezza: `${r.repo}/blob/main/SECURITY.md`,
      $licenza: `${r.repo}/blob/main/LICENSE`,
      $sponsor: `https://github.com/sponsors/${r.repo.split("/")[3]}`,
      $problemi: `${r.repo}/issues/new/choose`,
      $discussioni: `${r.repo}/discussions`,
      $email: "barattifederico@gmail.com",
      $anno: r.data.slice(0, 4),
    };
    const prima = problemi.length;
    usate.add("scarica.impronta.sommario").add("scarica.impronta.verifica");
    if (r.installer) {
      const conFile = { ...valori, $dimensione: pesoInLingua(r.installer.byte, l.locale), $sha256: r.installer.sha256 };
      valori.$verifica = riempi(IMPRONTA, t, conFile, problemi, usate);
    }
    pagine[`${l.cartella}index.html`] = riempi(modello, t, valori, problemi, usate);
    for (let i = prima; i < problemi.length; i++) problemi[i] = `${l.codice}: ${problemi[i]}`;
  }
  return pagine;
}

/** La pagina che Pages serve a qualunque indirizzo che non esiste: percorsi dalla radice, perché può comparire ovunque. */
function pagina404(r, problemi) {
  const modello = fs.readFileSync(path.join(SORGENTE, "404.html"), "utf8");
  const lingue = LINGUE.map((l) => `<a href="/${l.cartella}" hreflang="${l.codice}" lang="${l.codice}">${esc(l.nome)}</a>`).join("");
  return riempi(modello, {}, { $marchio: MARCHIO, $lingue: lingue, $versione: r.versione }, problemi);
}

/** Le pagine della documentazione: l'indice e un capitolo per pagina. */
function documentazione(r, problemi) {
  const { capitoli, mancanti, orfani } = docs.carica(path.join(SORGENTE, "docs"));
  for (const f of mancanti) problemi.push(`docs: indice.json elenca ${f}, che non esiste`);
  for (const f of orfani) problemi.push(`docs: ${f} non è in indice.json`);
  const modello = fs.readFileSync(path.join(SORGENTE, "docs", "pagina.html"), "utf8");
  const mappa = mappaDocs(capitoli);
  const titoli = {};
  for (const c of capitoli) for (const v of c.entries) titoli[v.id] = v.title;
  const pagine = {};

  const fonte = (p) => {
    const cartella = fs.existsSync(path.join(REPO, p)) && fs.statSync(path.join(REPO, p)).isDirectory();
    return `<a href="${r.repo}/${cartella ? "tree" : "blob"}/main/${p}"><code>${esc(p)}</code></a>`;
  };
  const latoCon = (base, corrente) =>
    `<ol class="capitoli">${capitoli
      .map((c) => {
        const qui = c === corrente;
        const voci = qui
          ? `<ol class="voci">${c.entries.map((v) => `<li><a href="#${v.id}" data-voce="${v.id}">${esc(v.title)}</a></li>`).join("")}</ol>`
          : "";
        return `<li><a href="${base}${percorsoCapitolo(c.id)}/"${qui ? ' aria-current="page"' : ""}>${esc(c.title)}</a>${voci}</li>`;
      })
      .join("")}</ol>`;
  const comuni = (base) => ({
    $marchio: MARCHIO,
    $versione: r.versione,
    $repo: r.repo,
    $privacy: `${r.repo}/blob/main/PRIVACY.md`,
    $radice: `${base}../`,
    $docs: base || "./",
    $ricerca: `${base}ricerca.json`,
    $base: base,
  });
  const percorsoPagina = (c) => `${DOMINIO}/docs/${percorsoCapitolo(c.id)}/`;

  // L'indice. L'app apre `…/docs/#<voce>` (l'indirizzo è fissato in
  // `comandi.rs` e nei suoi test): lo script nella testa porta l'ancora al
  // capitolo che la contiene, prima che l'indice si disegni.
  const rimando = `<script>(function(){var m=${jsonLd(mappa)};var h=location.hash.slice(1);try{h=decodeURIComponent(h)}catch(e){}if(h&&Object.prototype.hasOwnProperty.call(m,h)){location.replace(m[h]+"/"+(h.indexOf("c-")===0?"":"#"+h))}})();</script>`;
  const elenco = capitoli
    .map(
      (c) =>
        `<li><h2><a href="${percorsoCapitolo(c.id)}/">${esc(c.title)}</a></h2>${c.lead ? `<p>${inline(c.lead, (h) => (h.startsWith("#") ? `${mappa[h.slice(1)] || ""}/${h}` : h))}</p>` : ""}` +
        `<p class="conta">${c.entries.length} entries</p></li>`,
    )
    .join("\n");
  const prima = problemi.length;
  pagine["docs/index.html"] = riempi(
    modello,
    {},
    {
      ...comuni(""),
      $titolo: "Rymembra — Technical documentation",
      $descrizione:
        "How Rymembra works, from the code: architecture, the library database, scanning, the audio engine, network, listening together on the LAN, services, skins, the interface and the build.",
      $canonico: `${DOMINIO}/docs/`,
      $testa: rimando,
      $lato: latoCon("", null),
      $corpo:
        `<div class="intro"><h1>Technical documentation</h1>` +
        `<p>How Rymembra works, read from its code: what each part does, how it does it, and why it is built that way. It describes version ${esc(r.versione)} — the workspace in the repository — and every entry names the source files it comes from.</p>` +
        `<p>The code is written with Italian identifiers and comments; the <a href="${mappa.glossary}/#glossary">glossary</a> translates the words that recur. The <strong>?</strong> on each card of the Settings opens the entry for that feature. What travels over the network, request by request, is listed in <a href="${r.repo}/blob/main/PRIVACY.md">PRIVACY.md</a>.</p></div>` +
        `<ol class="indice-capitoli">\n${elenco}\n</ol>`,
      $jsonld: jsonLd({
        "@context": "https://schema.org",
        "@type": "CollectionPage",
        name: "Rymembra — Technical documentation",
        url: `${DOMINIO}/docs/`,
        inLanguage: "en",
        hasPart: capitoli.map((c) => ({ "@type": "TechArticle", headline: c.title, url: percorsoPagina(c) })),
      }),
    },
    problemi,
  );

  capitoli.forEach((c, i) => {
    const qui = percorsoCapitolo(c.id);
    // `#voce` resta nella pagina se la voce è di questo capitolo, altrimenti va
    // alla pagina del capitolo che la contiene.
    const collega = (h) => {
      if (!h.startsWith("#")) return h;
      const id = h.slice(1);
      const dove = mappa[id];
      if (!dove) return h;
      if (id.startsWith("c-")) return dove === qui ? "#" : `../${dove}/`;
      return dove === qui ? h : `../${dove}/${h}`;
    };
    const voci = c.entries
      .map((v) => {
        const fonti = v.src.length ? `<p class="fonte">Source ${v.src.map(fonte).join(" ")}</p>` : "";
        const vedi = (v.see || []).filter((id) => titoli[id]);
        const altro = vedi.length
          ? `<p class="vedi">See also: ${vedi.map((id) => `<a href="${esc(collega("#" + id))}">${esc(titoli[id])}</a>`).join(", ")}</p>`
          : "";
        return (
          `<article class="voce" id="${v.id}">\n<h2>${esc(v.title)}<a class="ancora" href="#${v.id}" aria-label="Link to this entry: ${esc(v.title)}">#</a></h2>\n` +
          `${fonti}\n${v.body.map((b) => blocco(b, collega)).join("\n")}\n${altro}\n</article>`
        );
      })
      .join("\n");
    const prec = capitoli[i - 1];
    const succ = capitoli[i + 1];
    const passi =
      `<nav class="passi" aria-label="Chapters">` +
      (prec ? `<a rel="prev" href="../${percorsoCapitolo(prec.id)}/"><span>Previous</span>${esc(prec.title)}</a>` : "<span></span>") +
      (succ ? `<a rel="next" href="../${percorsoCapitolo(succ.id)}/"><span>Next</span>${esc(succ.title)}</a>` : "") +
      `</nav>`;
    pagine[`docs/${qui}/index.html`] = riempi(
      modello,
      {},
      {
        ...comuni("../"),
        $titolo: `${c.title} — Rymembra docs`,
        $descrizione: piano(c.lead || c.title),
        $canonico: percorsoPagina(c),
        $testa: "",
        $lato: latoCon("../", c),
        $corpo:
          `<header class="intro" id="c-${c.id}"><p class="briciole"><a href="../">Docs</a></p><h1>${esc(c.title)}</h1>` +
          `${c.lead ? `<p class="lead">${inline(c.lead, collega)}</p>` : ""}</header>\n${voci}\n${passi}`,
        $jsonld: jsonLd({
          "@context": "https://schema.org",
          "@graph": [
            {
              "@type": "TechArticle",
              headline: c.title,
              description: piano(c.lead || c.title),
              url: percorsoPagina(c),
              inLanguage: "en",
              about: { "@type": "SoftwareApplication", name: "Rymembra", softwareVersion: r.versione },
              author: { "@type": "Person", name: autore() },
            },
            {
              "@type": "BreadcrumbList",
              itemListElement: [
                { "@type": "ListItem", position: 1, name: "Rymembra", item: `${DOMINIO}/` },
                { "@type": "ListItem", position: 2, name: "Docs", item: `${DOMINIO}/docs/` },
                { "@type": "ListItem", position: 3, name: c.title, item: percorsoPagina(c) },
              ],
            },
          ],
        }),
      },
      problemi,
    );
  });
  for (let i = prima; i < problemi.length; i++) problemi[i] = `docs: ${problemi[i]}`;

  const ricerca = capitoli.flatMap((c) =>
    c.entries.map((v) => ({ u: `${percorsoCapitolo(c.id)}/#${v.id}`, t: v.title, c: c.title, x: testoDellaVoce(v) })),
  );
  return { pagine, mappa, capitoli, ricerca };
}

function sitemap(capitoli) {
  const alternative = LINGUE.map(
    (l) => `    <xhtml:link rel="alternate" hreflang="${l.codice}" href="${urlLingua(l)}"/>`,
  ).join("\n");
  const url = [
    ...LINGUE.map((l) => `  <url>\n    <loc>${urlLingua(l)}</loc>\n${alternative}\n    <xhtml:link rel="alternate" hreflang="x-default" href="${urlLingua(LINGUE[0])}"/>\n  </url>`),
    `  <url>\n    <loc>${DOMINIO}/docs/</loc>\n  </url>`,
    ...capitoli.map((c) => `  <url>\n    <loc>${DOMINIO}/docs/${percorsoCapitolo(c.id)}/</loc>\n  </url>`),
  ];
  return (
    `<?xml version="1.0" encoding="UTF-8"?>\n` +
    `<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n${url.join("\n")}\n</urlset>\n`
  );
}

/** I file che si copiano così come sono: da `site/` e da dove vivono nel repository. */
function daCopiare() {
  const copie = [
    ["site/base.css", "base.css"],
    ["site/sito.css", "sito.css"],
    ["site/sito.js", "sito.js"],
    ["site/favicon.svg", "favicon.svg"],
    ["site/anteprima.jpg", "anteprima.jpg"],
    ["site/robots.txt", "robots.txt"],
    ["site/docs/docs.css", "docs/docs.css"],
    ["site/docs/docs.js", "docs/docs.js"],
    ["apps/desktop/src-tauri/icons/128x128@2x.png", "icona.png"],
    ["apps/desktop/src/font/OFL.txt", "font/OFL.txt"],
  ];
  // I font dell'app e le immagini del README: una copia sola nel repository.
  for (const f of fs.readdirSync(path.join(REPO, "apps/desktop/src/font")).filter((f) => f.endsWith(".woff2"))) {
    copie.push([`apps/desktop/src/font/${f}`, `font/${f}`]);
  }
  for (const f of fs.readdirSync(path.join(REPO, "immagini")).filter((f) => f.endsWith(".webp"))) {
    copie.push([`immagini/${f}`, `immagini/${f}`]);
  }
  // La colonna sonora: si scarica solo quando qualcuno la chiede.
  for (const f of fs.readdirSync(path.join(SORGENTE, "audio")).filter((f) => f.endsWith(".mp3"))) {
    copie.push([`site/audio/${f}`, `audio/${f}`]);
  }
  return copie;
}

function scrivi(cartella, rel, contenuto) {
  const p = path.join(cartella, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, contenuto);
}

function costruisci(cartella, versione, cartellaInstaller = null) {
  const problemi = [];
  const r = rilascio(versione, cartellaInstaller);
  const testi = {};
  for (const l of LINGUE) testi[l.codice] = JSON.parse(fs.readFileSync(path.join(SORGENTE, "testi", `${l.codice}.json`), "utf8"));
  problemi.push(...confrontaLingue(testi));

  const usate = new Set();
  const pagine = { ...landing(r, testi, problemi, usate), "404.html": pagina404(r, problemi) };
  for (const k of nonUsate(testi[LINGUE[0].codice], usate)) problemi.push(`testo «${k}» che nessuna pagina mostra`);
  const d = documentazione(r, problemi);
  Object.assign(pagine, d.pagine);

  fs.rmSync(cartella, { recursive: true, force: true });
  for (const [rel, html] of Object.entries(pagine)) scrivi(cartella, rel, html);
  scrivi(cartella, "docs/ricerca.json", JSON.stringify(d.ricerca));
  scrivi(cartella, "sitemap.xml", sitemap(d.capitoli));
  for (const [da, a] of daCopiare()) scrivi(cartella, a, fs.readFileSync(path.join(REPO, da)));

  problemi.push(...controllaUscita(cartella, d.mappa, docs.ancoreDelNucleo()));
  return { problemi, r, pagine: Object.keys(pagine).length };
}

/* ── i controlli sull'uscita ──────────────────────────────────────────── */

function tutti(cartella, rel = "") {
  return fs.readdirSync(path.join(cartella, rel), { withFileTypes: true }).flatMap((e) => {
    const p = rel ? `${rel}/${e.name}` : e.name;
    return e.isDirectory() ? tutti(cartella, p) : [p];
  });
}

/** Gli `id` di una pagina. */
function idDi(html) {
  return new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]));
}

/**
 * Ogni collegamento interno porta a un file che esiste e, se ha un'ancora, a
 * un `id` che quel file ha; nessuna pagina carica risorse da un altro dominio;
 * nessun segnaposto resta scritto; ogni ancora dell'app arriva a una voce.
 */
function controllaUscita(cartella, mappa, ancore) {
  const problemi = [];
  const file = tutti(cartella);
  const html = Object.fromEntries(file.filter((f) => f.endsWith(".html")).map((f) => [f, fs.readFileSync(path.join(cartella, f), "utf8")]));
  const ids = Object.fromEntries(Object.entries(html).map(([f, h]) => [f, idDi(h)]));
  const esiste = new Set(file);

  const risolvi = (da, indirizzo) => {
    const [percorso, ancora] = indirizzo.split("#");
    if (percorso === "") return { bersaglio: da, ancora };
    const base = percorso.startsWith("/") ? "" : path.posix.dirname(da);
    let bersaglio = path.posix.normalize(path.posix.join(base, percorso.replace(/^\//, ""))).replace(/\/$/, "");
    if (bersaglio === ".") bersaglio = "";
    // Una cartella si serve con il suo `index.html`, come fa Pages.
    if (percorso.endsWith("/") || !path.posix.extname(bersaglio)) bersaglio = bersaglio ? `${bersaglio}/index.html` : "index.html";
    return { bersaglio, ancora };
  };

  for (const [f, h] of Object.entries(html)) {
    if (/\{\{[=&#/]?[$.@\w-]/.test(h)) problemi.push(`${f}: un segnaposto è rimasto scritto`);
    for (const m of h.matchAll(/<(?:script|img|source|iframe)\b[^>]*\ssrc="([^"]*)"/g)) {
      if (/^(https?:)?\/\//.test(m[1])) problemi.push(`${f}: carica ${m[1]} da un altro dominio`);
    }
    for (const m of h.matchAll(/<link\b([^>]*)>/g)) {
      const rel = /\srel="([^"]+)"/.exec(m[1]);
      const href = /\shref="([^"]+)"/.exec(m[1]);
      if (!href) continue;
      const solo = rel && /^(canonical|alternate)$/.test(rel[1]);
      if (!solo && /^(https?:)?\/\//.test(href[1])) problemi.push(`${f}: carica ${href[1]} da un altro dominio`);
    }
    // `data-src` è un file che lo script carica da sé: si controlla come gli altri.
    for (const m of h.matchAll(/\s(?:href|src|data-src)="([^"]*)"/g)) {
      const u = m[1].replace(/&amp;/g, "&");
      if (u === "" || /^(https?:|mailto:|data:|\/\/)/.test(u)) continue;
      // La 404 si serve a qualunque indirizzo: lì un percorso relativo non sa da dove parte.
      if (f === "404.html" && !u.startsWith("/") && !u.startsWith("#")) {
        problemi.push(`404.html: «${u}» dev'essere un percorso dalla radice`);
        continue;
      }
      const { bersaglio, ancora } = risolvi(f, u);
      if (!esiste.has(bersaglio)) problemi.push(`${f}: «${u}» porta a ${bersaglio}, che non esiste`);
      else if (ancora && ancora !== "" && ids[bersaglio] && !ids[bersaglio].has(ancora)) {
        problemi.push(`${f}: «${u}» porta a un'ancora che ${bersaglio} non ha`);
      }
    }
  }
  for (const f of file.filter((f) => f.endsWith(".css"))) {
    const css = fs.readFileSync(path.join(cartella, f), "utf8");
    // Una graffa non chiusa non dà errore: il browser butta via in silenzio
    // tutto quello che viene dopo, token compresi.
    let livello = 0;
    for (const c of css.replace(/\/\*[\s\S]*?\*\//g, "").replace(/"[^"\n]*"|'[^'\n]*'/g, "")) {
      if (c === "{") livello++;
      else if (c === "}" && --livello < 0) break;
    }
    if (livello !== 0) problemi.push(`${f}: le graffe non tornano`);
    for (const m of css.matchAll(/url\(\s*['"]?([^'")]+)['"]?\s*\)/g)) {
      if (/^(data:|#)/.test(m[1])) continue;
      if (/^(https?:)?\/\//.test(m[1])) problemi.push(`${f}: carica ${m[1]} da un altro dominio`);
      else if (!esiste.has(path.posix.normalize(path.posix.join(path.posix.dirname(f), m[1])))) problemi.push(`${f}: «${m[1]}» non esiste`);
    }
  }
  for (const a of ancore || []) {
    const dove = mappa[a];
    const pagina = dove && `docs/${dove}/index.html`;
    if (!dove || !ids[pagina] || !ids[pagina].has(a)) problemi.push(`l'ancora dell'app «#${a}» non arriva a una voce`);
  }
  return problemi;
}

/* ── il comando ───────────────────────────────────────────────────────── */

function argomenti(argv) {
  const fuori = { controlla: false, versione: null, installer: null };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--controlla") fuori.controlla = true;
    else if (argv[i] === "--versione") fuori.versione = String(argv[++i] || "").replace(/^v/, "");
    else if (argv[i] === "--installer") fuori.installer = argv[++i] || null;
    else throw new Error(`argomento sconosciuto: ${argv[i]}`);
  }
  return fuori;
}

function main() {
  const a = argomenti(process.argv.slice(2));
  const versione = a.versione || JSON.parse(leggi("apps/desktop/package.json")).version;
  const cartella = a.controlla ? fs.mkdtempSync(path.join(os.tmpdir(), "rymembra-sito-")) : USCITA;
  let esito;
  try {
    esito = costruisci(cartella, versione, a.installer);
  } finally {
    if (a.controlla) fs.rmSync(cartella, { recursive: true, force: true });
  }
  if (esito.problemi.length) {
    console.error(`sito: ${esito.problemi.length} problemi\n` + esito.problemi.map((p) => "  - " + p).join("\n"));
    process.exit(1);
  }
  const dove = a.controlla ? "controllate" : `in ${path.relative(REPO, cartella)}/`;
  console.log(`sito: ${esito.pagine} pagine ${dove}, versione ${esito.r.versione} del ${esito.r.data}: tutto torna.`);
}

if (require.main === module) main();

module.exports = {
  esc,
  inline,
  piano,
  riempi,
  foglie,
  confrontaLingue,
  nonUsate,
  dataNelChangelog,
  dataInLingua,
  pesoInLingua,
  percorsoCapitolo,
  mappaDocs,
  controllaUscita,
  argomenti,
  costruisci,
  LINGUE,
};
