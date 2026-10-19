/* La documentazione tecnica di Rymembra: il disegno.
 *
 * Il testo non sta qui: sta in `contenuto/<capitolo>.js`, un file per capitolo,
 * e ognuno aggiunge il suo a `window.RYMEMBRA_DOCS` nell'ordine in cui
 * `index.html` li carica. `strumenti/docs.js` controlla che i file siano tutti
 * caricati, che le ancore esistano e che ogni percorso citato in `src` esista
 * davvero nel repository. Questo file sa soltanto come si mette in pagina —
 * l'indice, la ricerca, i blocchi.
 *
 * # Perché niente `innerHTML`
 *
 * Il contenuto è scritto da noi, ma costruire i nodi uno per uno costa venti
 * righe e leva di mezzo un'intera classe di difetti: un `<` in un blocco di
 * codice sarebbe un frammento di pagina che sparisce, non un carattere.
 *
 * # Una lingua sola
 *
 * È documentazione del codice, e il codice si legge in una lingua: l'inglese.
 * Un `?lang=` rimasto in un collegamento vecchio non fa niente. */
(function () {
  "use strict";

  var CAPITOLI = window.RYMEMBRA_DOCS || [];
  var indice = document.querySelector("[data-index]");
  var corpo = document.querySelector("[data-body]");
  var vuoto = document.querySelector("[data-empty]");
  var ricerca = document.querySelector("[data-search]");
  var menu = document.querySelector("[data-menu]");
  var lato = document.getElementById("indice");

  /* ── i nodi ── */
  function el(nome, classe, testo) {
    var n = document.createElement(nome);
    if (classe) n.className = classe;
    if (testo !== undefined) n.textContent = testo;
    return n;
  }

  /** `**grassetto**`, `*corsivo*`, `codice`, `[testo](https://…)` e `[testo](#voce)` dentro una riga, senza HTML. */
  var PEZZI =
    /(\*\*[^*]+\*\*|\*[^*\s][^*]*\*|`[^`]+`|\[[^\]]+\]\((?:https:\/\/[^)\s]+|#[a-z0-9-]+)\))/g;
  function inline(genitore, testo) {
    String(testo)
      .split(PEZZI)
      .forEach(function (p) {
        if (p === "") return;
        if (p.slice(0, 2) === "**" && p.slice(-2) === "**") {
          genitore.appendChild(el("strong", "", p.slice(2, -2)));
        } else if (p.charAt(0) === "`" && p.slice(-1) === "`") {
          genitore.appendChild(el("code", "", p.slice(1, -1)));
        } else if (p.charAt(0) === "[" && /\]\((https:\/\/|#)/.test(p)) {
          var taglio = p.indexOf("](");
          var a = el("a", "", p.slice(1, taglio));
          a.href = p.slice(taglio + 2, -1);
          if (a.getAttribute("href").charAt(0) !== "#") {
            a.target = "_blank";
            a.rel = "noopener";
          }
          genitore.appendChild(a);
        } else if (p.charAt(0) === "*" && p.slice(-1) === "*") {
          genitore.appendChild(el("em", "", p.slice(1, -1)));
        } else {
          genitore.appendChild(document.createTextNode(p));
        }
      });
    return genitore;
  }

  function tavola(t) {
    var cornice = el("div", "tavola");
    var tab = el("table");
    var testa = el("thead");
    var riga = el("tr");
    t.head.forEach(function (c) {
      riga.appendChild(inline(el("th"), c));
    });
    testa.appendChild(riga);
    tab.appendChild(testa);
    var righe = el("tbody");
    t.rows.forEach(function (r) {
      var tr = el("tr");
      r.forEach(function (c) {
        tr.appendChild(inline(el("td"), c));
      });
      righe.appendChild(tr);
    });
    tab.appendChild(righe);
    cornice.appendChild(tab);
    return cornice;
  }

  function blocco(b) {
    if (typeof b === "string") return inline(el("p"), b);
    if (b.h !== undefined) return inline(el("h4"), b.h);
    if (b.note !== undefined) return inline(el("p", "callout"), b.note);
    if (b.code !== undefined) {
      var pre = el("pre");
      pre.appendChild(el("code", "", b.code));
      return pre;
    }
    if (b.table !== undefined) return tavola(b.table);
    var lista = el(b.ol ? "ol" : "ul");
    (b.ol || b.ul).forEach(function (riga) {
      lista.appendChild(inline(el("li"), riga));
    });
    return lista;
  }

  /** Il testo di una voce, tutto di fila, per la ricerca. */
  function testoDi(capitolo, voce) {
    var pezzi = [capitolo.title, voce.title, voce.id].concat(voce.src);
    voce.body.forEach(function (b) {
      if (typeof b === "string") pezzi.push(b);
      else if (b.h !== undefined) pezzi.push(b.h);
      else if (b.note !== undefined) pezzi.push(b.note);
      else if (b.code !== undefined) pezzi.push(b.code);
      else if (b.table !== undefined) {
        pezzi.push(b.table.head.join(" "));
        b.table.rows.forEach(function (r) {
          pezzi.push(r.join(" "));
        });
      } else pezzi.push((b.ol || b.ul).join(" "));
    });
    return pezzi.join(" ").toLowerCase();
  }

  /* ── la pagina ── */
  var elementi = []; // { voce, sezione, li, link, testo }
  var capitoliDom = []; // { sezione, titolo, elenco, voci: [] }

  function disegna() {
    var titoli = {};
    CAPITOLI.forEach(function (c) {
      c.entries.forEach(function (v) {
        titoli[v.id] = v.title;
      });
    });

    CAPITOLI.forEach(function (capitolo) {
      var sezione = el("section", "capitolo");
      sezione.id = "c-" + capitolo.id;
      sezione.appendChild(el("h2", "", capitolo.title));
      if (capitolo.lead) sezione.appendChild(inline(el("p", "lead"), capitolo.lead));

      var titoloIndice = el("h2", "", capitolo.title);
      var elenco = el("ul");
      var dom = { sezione: sezione, titolo: titoloIndice, elenco: elenco, voci: [] };

      capitolo.entries.forEach(function (voce) {
        var s = el("article", "voce");
        s.id = voce.id;
        var h = el("h3");
        h.appendChild(document.createTextNode(voce.title));
        var ancora = el("a", "ancora", "#");
        ancora.href = "#" + voce.id;
        ancora.setAttribute("aria-label", "Link to this entry: " + voce.title);
        h.appendChild(ancora);
        s.appendChild(h);
        if (voce.src && voce.src.length) {
          var fonte = el("p", "fonte", "Source ");
          voce.src.forEach(function (percorso) {
            fonte.appendChild(el("code", "", percorso));
          });
          s.appendChild(fonte);
        }
        voce.body.forEach(function (b) {
          s.appendChild(blocco(b));
        });
        if (voce.see && voce.see.length) {
          var vedi = el("p", "vedi", "See also:");
          voce.see.forEach(function (id) {
            if (!titoli[id]) return;
            var a = el("a", "", titoli[id]);
            a.href = "#" + id;
            vedi.appendChild(a);
          });
          s.appendChild(vedi);
        }
        sezione.appendChild(s);

        var li = el("li");
        var link = el("a", "", voce.title);
        link.href = "#" + voce.id;
        li.appendChild(link);
        elenco.appendChild(li);

        var e = { voce: voce, sezione: s, li: li, link: link, testo: testoDi(capitolo, voce) };
        elementi.push(e);
        dom.voci.push(e);
      });
      corpo.appendChild(sezione);
      indice.appendChild(titoloIndice);
      indice.appendChild(elenco);
      capitoliDom.push(dom);
    });

    filtra();
    osserva();
  }

  /* ── la ricerca ── */
  function filtra() {
    var q = ricerca.value.trim().toLowerCase();
    var trovate = 0;
    capitoliDom.forEach(function (c) {
      var visibili = 0;
      c.voci.forEach(function (e) {
        var vista = q === "" || e.testo.indexOf(q) !== -1;
        e.sezione.hidden = !vista;
        e.li.hidden = !vista;
        if (vista) visibili++;
      });
      trovate += visibili;
      c.sezione.hidden = visibili === 0;
      c.titolo.hidden = visibili === 0;
      c.elenco.hidden = visibili === 0;
    });
    vuoto.hidden = trovate !== 0;
  }

  /* ── dov'è il lettore ── */
  var osservatore = null;
  function osserva() {
    if (osservatore) osservatore.disconnect();
    if (!("IntersectionObserver" in window)) return;
    osservatore = new IntersectionObserver(
      function (righe) {
        righe.forEach(function (r) {
          if (!r.isIntersecting) return;
          elementi.forEach(function (e) {
            if (e.sezione === r.target) e.link.setAttribute("aria-current", "true");
            else e.link.removeAttribute("aria-current");
          });
        });
      },
      { rootMargin: "-84px 0px -65% 0px" },
    );
    elementi.forEach(function (e) {
      osservatore.observe(e.sezione);
    });
  }

  /* ── i gesti ── */
  ricerca.addEventListener("input", filtra);
  function chiudiMenu() {
    lato.removeAttribute("data-aperto");
    menu.setAttribute("aria-expanded", "false");
  }
  menu.addEventListener("click", function () {
    var aperto = lato.hasAttribute("data-aperto");
    if (aperto) chiudiMenu();
    else {
      lato.setAttribute("data-aperto", "");
      menu.setAttribute("aria-expanded", "true");
    }
  });
  lato.addEventListener("click", function (ev) {
    if (ev.target.closest && ev.target.closest("a")) chiudiMenu();
  });
  document.addEventListener("keydown", function (ev) {
    if (ev.key === "Escape") chiudiMenu();
  });

  /** Se l'indirizzo porta un'ancora, ci si va: la pagina nasce dopo il caricamento. */
  function vaiAllaPagina() {
    var id = decodeURIComponent((window.location.hash || "").slice(1));
    if (!id) return;
    var n = document.getElementById(id);
    if (n) n.scrollIntoView({ behavior: "instant", block: "start" });
  }
  // Un'ancora verso una voce nascosta dalla ricerca: si toglie il filtro, o il
  // collegamento non porterebbe da nessuna parte.
  window.addEventListener("hashchange", function () {
    var id = decodeURIComponent((window.location.hash || "").slice(1));
    var e = elementi.filter(function (x) {
      return x.voce.id === id;
    })[0];
    if (e && e.sezione.hidden) {
      ricerca.value = "";
      filtra();
      vaiAllaPagina();
    }
  });

  if (CAPITOLI.length === 0) {
    corpo.textContent = "Documentation unavailable.";
    return;
  }
  disegna();
  vaiAllaPagina();
})();
