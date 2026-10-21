/* La documentazione tecnica di Rymembra: quello che il JavaScript aggiunge.
 *
 * Le pagine arrivano finite: `strumenti/sito.js` scrive un capitolo per pagina
 * da `contenuto/<capitolo>.js`, e senza questo file si leggono tutte. Qui ci
 * sono tre comodità:
 *
 *   - la ricerca, su tutti i capitoli: l'indice (`ricerca.json`) si scarica la
 *     prima volta che si scrive nel campo, non a ogni pagina;
 *   - nell'indice a lato, la voce che si sta leggendo;
 *   - al telefono, il pulsante che apre e chiude l'indice.
 *
 * Il rimando delle vecchie ancore (`/docs/#voce`, quelle che apre il «?» delle
 * Impostazioni) non sta qui: è uno script nella testa di `/docs/`, che deve
 * partire prima che la pagina si disegni.
 *
 * Niente `innerHTML`: i risultati sono testo scritto da noi, ma costruire i nodi
 * uno per uno costa poco e leva di mezzo un'intera classe di difetti. */
(function () {
  "use strict";

  document.documentElement.classList.add("js");

  /* ── al telefono, l'indice ── */
  var menu = document.querySelector("[data-menu]");
  var lato = document.getElementById("indice");
  function chiudi() {
    lato.removeAttribute("data-aperto");
    menu.setAttribute("aria-expanded", "false");
  }
  if (menu && lato) {
    menu.hidden = false;
    menu.addEventListener("click", function () {
      if (lato.hasAttribute("data-aperto")) chiudi();
      else {
        lato.setAttribute("data-aperto", "");
        menu.setAttribute("aria-expanded", "true");
      }
    });
    lato.addEventListener("click", function (ev) {
      if (ev.target.closest && ev.target.closest("a")) chiudi();
    });
    document.addEventListener("keydown", function (ev) {
      if (ev.key === "Escape") chiudi();
    });
  }

  /* ── la voce che si sta leggendo ── */
  var collegamenti = {};
  Array.prototype.forEach.call(document.querySelectorAll("[data-voce]"), function (a) {
    collegamenti[a.getAttribute("data-voce")] = a;
  });
  var voci = Array.prototype.slice.call(document.querySelectorAll("article.voce"));
  if (voci.length && "IntersectionObserver" in window) {
    var osservatore = new IntersectionObserver(
      function (righe) {
        righe.forEach(function (r) {
          if (!r.isIntersecting) return;
          for (var id in collegamenti) {
            if (id === r.target.id) collegamenti[id].setAttribute("aria-current", "true");
            else collegamenti[id].removeAttribute("aria-current");
          }
        });
      },
      { rootMargin: "-90px 0px -65% 0px" },
    );
    voci.forEach(function (v) {
      osservatore.observe(v);
    });
  }

  /* ── la ricerca ── */
  var modulo = document.querySelector("[data-cerca]");
  if (!modulo || !window.fetch) return;
  var campo = modulo.querySelector("input");
  var elenco = modulo.querySelector("[data-risultati]");
  var nessuno = modulo.querySelector("[data-nessuno]");
  var base = modulo.getAttribute("data-base") || "";
  var indice = null;
  var inArrivo = null;
  modulo.hidden = false;

  function carica() {
    if (!inArrivo) {
      inArrivo = fetch(modulo.getAttribute("data-ricerca"))
        .then(function (r) {
          if (!r.ok) throw new Error(String(r.status));
          return r.json();
        })
        .then(function (dati) {
          indice = dati;
        })
        .catch(function () {
          inArrivo = null;
        });
    }
    return inArrivo;
  }

  function el(nome, testo) {
    var n = document.createElement(nome);
    if (testo !== undefined) n.textContent = testo;
    return n;
  }

  /** Tutte le parole, in qualunque ordine: «gapless resampler» trova la voce che le ha tutte e due. */
  function cerca() {
    var parole = campo.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
    while (elenco.firstChild) elenco.removeChild(elenco.firstChild);
    if (!parole.length || !indice) {
      elenco.hidden = true;
      nessuno.hidden = true;
      return;
    }
    var trovate = indice.filter(function (v) {
      var testo = v.t.toLowerCase() + " " + v.x;
      return parole.every(function (p) {
        return testo.indexOf(p) !== -1;
      });
    });
    // Prima quelle che hanno le parole nel titolo.
    trovate.sort(function (a, b) {
      var ta = parole.every(function (p) {
        return a.t.toLowerCase().indexOf(p) !== -1;
      });
      var tb = parole.every(function (p) {
        return b.t.toLowerCase().indexOf(p) !== -1;
      });
      return (tb ? 1 : 0) - (ta ? 1 : 0);
    });
    trovate.slice(0, 30).forEach(function (v) {
      var li = el("li");
      var a = el("a", v.t);
      a.href = base + v.u;
      a.appendChild(el("small", v.c));
      li.appendChild(a);
      elenco.appendChild(li);
    });
    elenco.hidden = trovate.length === 0;
    nessuno.hidden = trovate.length !== 0;
  }

  campo.addEventListener("focus", carica);
  campo.addEventListener("input", function () {
    if (indice) cerca();
    else carica().then(cerca);
  });
  modulo.addEventListener("submit", function (ev) {
    ev.preventDefault();
    var primo = elenco.querySelector("a");
    if (primo) window.location.href = primo.href;
  });
})();
