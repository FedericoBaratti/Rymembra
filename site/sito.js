/* La pagina di Rymembra: quello che serve il JavaScript.
 *
 * La pagina è completa senza: ogni scena è ferma nel suo stato finale, il disco
 * dietro l'apertura sta fermo, e nessun contenuto arriva da qui. Questo file
 * aggiunge:
 *
 *   - Il viaggio: per ogni tappa scrive `--p`, da 0 a 1, quanto se n'è già
 *     scorso, e `--e`, quanto la sua scena è entrata nello schermo; segna nella
 *     rotta la tappa in cui si è. Le scene sono CSS.
 *   - La musica: «Machina» di Scott Buckley (CC BY 4.0), che si scarica solo
 *     al primo clic su «Ascolta» nella testata ed è livellata a −18 LUFS come
 *     in Rymembra. Il punto in cui si è fermata si ricorda: il clic dopo, anche
 *     in un'altra pagina, riprende da lì. Mentre suona, il disco dietro
 *     l'apertura gira a 33 giri, e la luce dietro le scene, le barre e l'onda
 *     della tappa del suono sono quelle del brano vero.
 *   - La prova della tappa del suono: due tracce di un brano continuo, con e
 *     senza pausa, con e senza livellamento.
 *   - I titoli che si mettono a fuoco quando arrivano, e il pulsante «Scarica»
 *     che si avvicina al mouse.
 *   - Il pulsante «Scarica» della testata, che compare quando quello
 *     dell'apertura esce dallo schermo.
 *   - Il menu dello schermo stretto (un `details`): si chiude quando si sceglie
 *     un collegamento, con un clic fuori e con Esc.
 *   - Le schermate delle tappe in grande, in un `dialog`, al clic.
 *   - Per chi non è su Windows, il pulsante che condivide o copia il link
 *     della pagina; l'avviso lo mostra lo script nella testa della pagina.
 *   - La lingua: sulla radice, se il browser parla una delle altre tre e chi
 *     legge non ha ancora scelto, una striscia la offre. Nessun redirect: lo
 *     sconsiglia Google, e lo sconsiglia chi viaggia con il computer di un
 *     altro. La scelta fatta con un clic si ricorda.
 *
 * Con il movimento ridotto le scene e il disco restano fermi e i titoli non si
 * sfocano; la musica e la prova si ascoltano lo stesso. */
(function () {
  "use strict";

  var radice = document.documentElement;
  radice.classList.add("js");
  var ridotto = window.matchMedia("(prefers-reduced-motion: reduce)");
  var stretto = window.matchMedia("(max-width: 860px)");
  var mouse = window.matchMedia("(hover: hover) and (pointer: fine)");
  var CHIAVE = "rymembra-lang";
  var CHIAVE_MUSICA = "rymembra-musica";

  function leggi(chiave) {
    try {
      return localStorage.getItem(chiave);
    } catch (e) {
      return null;
    }
  }
  function scrivi(chiave, valore) {
    try {
      localStorage.setItem(chiave, valore);
    } catch (e) {
      /* senza memoria si richiede la prossima volta */
    }
  }
  function tutti(selettore) {
    return Array.prototype.slice.call(document.querySelectorAll(selettore));
  }
  function limita(x) {
    return x < 0 ? 0 : x > 1 ? 1 : x;
  }

  /* ── la lingua ── */
  document.addEventListener("click", function (ev) {
    var a = ev.target.closest && ev.target.closest("[data-lingua]");
    if (a) scrivi(CHIAVE, a.getAttribute("data-lingua"));
  });
  var striscia = document.querySelector("[data-suggerimento]");
  if (striscia) {
    var scelta = leggi(CHIAVE);
    var lingua = scelta || ((navigator.languages && navigator.languages[0]) || navigator.language || "").slice(0, 2).toLowerCase();
    var offerta = scelta === "en" ? null : striscia.querySelector('[data-per="' + lingua + '"]');
    if (offerta) {
      Array.prototype.forEach.call(striscia.children, function (p) {
        p.hidden = p !== offerta;
      });
      striscia.hidden = false;
    }
    striscia.addEventListener("click", function (ev) {
      if (!ev.target.closest("[data-chiudi]")) return;
      striscia.hidden = true;
      scrivi(CHIAVE, "en");
    });
  }

  /* ── chi non è su Windows ──
     L'avviso e il suo pulsante li mostra lo script nella testa della pagina,
     prima che si disegni; qui si risponde al clic. Il collegamento è quello
     della pagina, da aprire poi sul computer. */
  tutti("[data-condividi]").forEach(function (b) {
    b.addEventListener("click", function () {
      var url = b.getAttribute("data-url");
      if (navigator.share) {
        navigator.share({ title: "Rymembra", url: url }).catch(function () {
          /* chiusa senza scegliere */
        });
      } else if (navigator.clipboard) {
        navigator.clipboard.writeText(url).then(
          function () {
            b.querySelector(".altrove-copia").textContent = b.getAttribute("data-copiato");
          },
          function () {
            /* il browser non ha permesso: il link resta da copiare a mano */
          },
        );
      }
    });
  });

  /* ── il menu dello schermo stretto ──
     Si apre e chiude da solo (è un `details`); qui si chiude quando si sceglie
     un collegamento, quando si fa clic fuori, e con Esc. */
  var tendina = document.querySelector("[data-tendina]");
  if (tendina) {
    tendina.addEventListener("click", function (ev) {
      if (ev.target.closest(".tendina-pannello a")) tendina.open = false;
    });
    document.addEventListener("click", function (ev) {
      if (tendina.open && !tendina.contains(ev.target)) tendina.open = false;
    });
    document.addEventListener("keydown", function (ev) {
      if (ev.key !== "Escape" || !tendina.open) return;
      tendina.open = false;
      tendina.querySelector("summary").focus();
    });
  }

  /* ── le schermate in grande ──
     Il collegamento porta all'immagine; dove c'è `dialog` la si apre sopra la
     pagina invece. Esc e il ritorno del fuoco al collegamento sono del browser.
     Un clic sull'immagine o sul fondo chiude. */
  var lente = document.querySelector("[data-lente]");
  if (lente && lente.showModal) {
    var grande = lente.querySelector("img");
    tutti("[data-lente-apri]").forEach(function (a) {
      a.addEventListener("click", function (ev) {
        if (ev.button !== 0 || ev.ctrlKey || ev.metaKey || ev.shiftKey) return;
        ev.preventDefault();
        var piccola = a.querySelector("img");
        grande.src = a.getAttribute("href");
        grande.alt = piccola.alt;
        lente.setAttribute("aria-label", piccola.alt);
        lente.showModal();
      });
    });
    lente.addEventListener("click", function (ev) {
      if (ev.target === lente || ev.target === grande) lente.close();
    });
  }

  /* ── il pulsante della testata ── */
  var testata = document.querySelector("[data-testata]");
  var principale = document.querySelector("[data-scarica]");
  if (testata && principale && "IntersectionObserver" in window) {
    new IntersectionObserver(function (righe) {
      testata.classList.toggle("mini", !righe[0].isIntersecting && righe[0].boundingClientRect.top < 0);
    }).observe(principale);
  } else if (testata) {
    testata.classList.add("mini");
  }

  /* ── i pezzi del viaggio ── */
  var tappe = tutti("[data-tappa]");
  var scene = tappe.map(function (t) {
    return t.querySelector(".scena");
  });
  var rotta = {};
  tutti("[data-rotta]").forEach(function (a) {
    rotta[a.getAttribute("data-rotta")] = a;
  });
  var viaggio = document.getElementById("viaggio");
  var arrivo = document.getElementById("scarica");
  var bagliore = document.querySelector(".bagliore");
  var luce = document.querySelector(".bagliore-luce");

  var tappaSegnata;
  function segna(id) {
    if (id === tappaSegnata) return;
    tappaSegnata = id;
    for (var r in rotta) {
      if (r === id) rotta[r].setAttribute("aria-current", "step");
      else rotta[r].removeAttribute("aria-current");
    }
  }

  /* ── le misure ──
     Quello che non cambia con lo scorrimento si misura una volta, e di nuovo
     quando cambia la pagina: allo scorrimento restano solo conti. */
  var m = null;
  var misureScene = new WeakMap();
  function sopra(el) {
    return el.getBoundingClientRect().top + window.scrollY;
  }
  function rimisura() {
    misureScene = new WeakMap();
    m = {
      alto: window.innerHeight,
      viaggio: viaggio ? sopra(viaggio) : 0,
      arrivo: arrivo ? sopra(arrivo) : 0,
    };
    chiedi();
  }

  /* Quanto di una tappa si è scorso. Accanto al testo la scena sta ferma
     (`position: sticky`) per un tratto: conta quel tratto, cioè di quanto la
     scena si è spostata dentro la sua tappa, da dove parte a dove si ferma,
     e prima ancora l'ultima salita, da quando è entrata tutta (`--e` a 1) a
     quando si ferma: con tappe alte uno schermo, il solo tratto da ferma di
     una scena alta sarebbe un attimo. Arriva alla fine un po' prima di
     ripartire, perché lo stato finale si veda fermo. Al telefono la scena
     scorre con il resto, e conta il suo passaggio sullo schermo.

     Dove sta la scena si calcola dalla tappa, come farebbe il browser, e non
     si legge: il suo rettangolo comprende lo spostamento con cui entra, che
     dipende da `--e`, che dipende da dove sta. Le sue misure non cambiano
     scorrendo, e si prendono una volta. */
  var ENTRATA = 0.45;
  function scenaMisure(tappa, scena) {
    var g = misureScene.get(scena);
    if (!g) {
      var stile = window.getComputedStyle(scena);
      g = {
        inizio: stretto.matches ? scena.offsetTop - tappa.offsetTop : parseFloat(stile.marginTop) || 0,
        fermo: stretto.matches ? null : parseFloat(stile.top) || 0,
        alta: scena.offsetHeight,
      };
      misureScene.set(scena, g);
    }
    return g;
  }
  function sopraScena(r, g) {
    if (g.fermo === null) return r.top + g.inizio;
    return Math.min(Math.max(r.top + g.inizio, g.fermo), r.bottom - g.alta);
  }
  function avanzamento(r, g, sTop, alto) {
    if (g.fermo === null) return limita((alto * 0.92 - sTop) / (alto * 0.62));
    var salita = Math.max(0, alto * (1 - ENTRATA) - g.fermo);
    var fatta = Math.min(salita, Math.max(0, alto * (1 - ENTRATA) - sTop));
    var corsaScena = r.height - g.alta - g.inizio;
    return limita((fatta + sTop - r.top - g.inizio) / Math.max(1, salita + corsaScena * 0.8));
  }

  /* ── lo scorrimento ── */
  function misura() {
    if (!m) return;
    var alto = m.alto;
    var sy = window.scrollY;
    var corrente = null;
    tappe.forEach(function (t, i) {
      var r = t.getBoundingClientRect();
      if (r.top < alto * 0.5 && r.bottom > alto * 0.5) corrente = t.id;
      if (ridotto.matches || r.bottom < -alto || r.top > alto * 2) return;
      var g = scenaMisure(t, scene[i]);
      var sTop = sopraScena(r, g);
      t.style.setProperty("--p", avanzamento(r, g, sTop, alto).toFixed(3));
      t.style.setProperty("--e", limita((alto - sTop) / (alto * (stretto.matches ? 0.35 : ENTRATA))).toFixed(3));
    });
    if (arrivo) {
      var a = arrivo.getBoundingClientRect();
      if (a.top < alto * 0.5 && a.bottom > 0) corrente = "scarica";
    }
    segna(corrente);

    if (bagliore) bagliore.classList.toggle("acceso", sy + alto > m.viaggio && sy < m.arrivo + alto * 0.4);
    if (testata) testata.classList.toggle("scorsa", sy > 8);
  }

  var inCoda = false;
  function chiedi() {
    if (inCoda) return;
    inCoda = true;
    window.requestAnimationFrame(function () {
      inCoda = false;
      misura();
    });
  }
  window.addEventListener("scroll", chiedi, { passive: true });
  /* Le misure si prendono in un fotogramma, mai nel mezzo del caricamento: la
     prima impaginazione è del browser, e il caricamento della pagina, dei font
     e i cambi di misura del contenuto finiscono in una misura sola. */
  var misureChieste = false;
  function rimisuraPresto() {
    if (misureChieste) return;
    misureChieste = true;
    window.requestAnimationFrame(function () {
      misureChieste = false;
      rimisura();
    });
  }
  window.addEventListener("resize", rimisuraPresto);
  window.addEventListener("load", rimisuraPresto);
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(rimisuraPresto);
  var contenuto = document.getElementById("contenuto");
  if ("ResizeObserver" in window && contenuto) new ResizeObserver(rimisuraPresto).observe(contenuto);
  [ridotto, stretto].forEach(function (q) {
    if (q.addEventListener) q.addEventListener("change", rimisuraPresto);
  });

  /* ── la musica ── */
  var colonna = document.querySelector("[data-colonna]");
  var tasto = document.querySelector("[data-suona]");
  var suona = false;
  var ctx = null;
  var analisi = null;
  var frequenze = null;
  var onda = null;
  // Rymembra porta ogni brano a −18 LUFS: «Machina» ne misura −14,6.
  var LIVELLO = Math.pow(10, -3.4 / 20);
  var salvato = parseFloat(leggi(CHIAVE_MUSICA)) || 0;

  function suonoPronto() {
    var Contesto = window.AudioContext || window.webkitAudioContext;
    if (!ctx && Contesto) {
      ctx = new Contesto();
      analisi = ctx.createAnalyser();
      analisi.fftSize = 2048;
      analisi.smoothingTimeConstant = 0.8;
      analisi.minDecibels = -88;
      analisi.maxDecibels = -22;
      analisi.connect(ctx.destination);
      frequenze = new Float32Array(analisi.frequencyBinCount);
      onda = new Uint8Array(analisi.fftSize);
      var livello = ctx.createGain();
      livello.gain.value = LIVELLO;
      livello.connect(analisi);
      ctx.createMediaElementSource(colonna).connect(livello);
    }
    if (!ctx) colonna.volume = LIVELLO;
    else if (ctx.state === "suspended") ctx.resume();
  }

  function aggiorna() {
    var ora = (colonna && !colonna.paused) || prova.inCorso;
    if (ora !== suona) {
      suona = ora;
      radice.classList.toggle("suona", ora);
      if (ora) avviaCiclo();
      chiedi();
    }
    // Il pulsante dice la colonna sonora, non la prova: durante la prova un
    // clic fa partire la musica, e il pulsante lo dice.
    var fermo = !colonna || colonna.paused;
    if (tasto) {
      tasto.classList.toggle("attiva", !fermo);
      // Il nome dice che cosa fa il clic («Ascolta», «Metti in pausa»): con
      // `aria-pressed` in più, un lettore di schermo direbbe due cose opposte.
      tasto.setAttribute("aria-label", tasto.getAttribute(fermo ? "data-ascolta" : "data-pausa"));
    }
  }

  function salva() {
    if (colonna && colonna.currentTime > 0) scrivi(CHIAVE_MUSICA, colonna.currentTime.toFixed(1));
  }

  function alterna() {
    if (prova.inCorso) prova.ferma(false);
    if (!colonna.paused) {
      colonna.pause();
      return;
    }
    suonoPronto();
    if (salvato && colonna.currentTime < 0.5) {
      try {
        colonna.currentTime = salvato;
      } catch (e) {
        /* si riprende appena il file dice quanto è lungo */
      }
    }
    var p = colonna.play();
    if (p && p.catch) p.catch(aggiorna);
  }

  if (colonna && tasto) {
    // `pause` arriva anche alla fine e quando la scheda lo chiede: si ricorda il punto.
    ["play", "playing", "pause", "ended", "error"].forEach(function (nome) {
      colonna.addEventListener(nome, aggiorna);
    });
    colonna.addEventListener("pause", salva);
    colonna.addEventListener("loadedmetadata", function () {
      if (salvato && colonna.currentTime < 0.5 && salvato < colonna.duration - 1) colonna.currentTime = salvato;
      salvato = 0;
    });
    window.addEventListener("pagehide", salva);
    tasto.hidden = false;
    tasto.addEventListener("click", alterna);
  }

  /* ── la prova: due tracce di un brano continuo ──
     Una registrazione sola, 12 secondi di «Machina», divisa al sesto secondo
     nel momento di suonarla: due file separati avrebbero ciascuno il silenzio
     che l'MP3 mette ai bordi, e la pausa ci sarebbe sempre. «Senza pause»
     spento lascia fra le due tracce lo spazio di un lettore che non è gapless;
     «Volume livellato» spento fa sentire la seconda come un master più forte
     di 8 dB. */
  var prova = { inCorso: false, ferma: function () {} };
  var nodoProva = document.querySelector("[data-prova]");
  if (nodoProva && colonna && (window.AudioContext || window.webkitAudioContext) && window.fetch) {
    var tastoProva = nodoProva.querySelector("[data-prova-suona]");
    var etichettaProva = tastoProva.querySelector("span");
    var opzioni = { pause: true, livello: true };
    var registrazione = null;
    var sorgenti = [];
    var riprendere = false;
    var TAGLIO = 6;

    prova.seconda = Infinity;
    prova.ferma = function (riprendi) {
      sorgenti.forEach(function (s) {
        s.onended = null;
        try {
          s.stop();
        } catch (e) {
          /* già finita */
        }
      });
      sorgenti = [];
      prova.inCorso = false;
      tastoProva.classList.remove("attiva");
      etichettaProva.textContent = tastoProva.getAttribute("data-ascolta");
      if (riprendi && riprendere) colonna.play();
      riprendere = false;
      aggiorna();
    };

    var carica = function () {
      if (!registrazione) {
        registrazione = fetch(nodoProva.getAttribute("data-src"))
          .then(function (r) {
            return r.arrayBuffer();
          })
          .then(function (dati) {
            return new Promise(function (ok, no) {
              ctx.decodeAudioData(dati, ok, no);
            });
          });
        registrazione.catch(function () {
          registrazione = null;
        });
      }
      return registrazione;
    };

    var traccia = function (buffer, quando, da, durata, guadagno, sfuma) {
      var s = ctx.createBufferSource();
      var g = ctx.createGain();
      s.buffer = buffer;
      g.gain.value = guadagno;
      // Senza gapless la traccia si interrompe e riparte: 8 ms di rampa
      // evitano il clic, che il lettore vero non farebbe sentire comunque.
      if (sfuma === "fine") {
        g.gain.setValueAtTime(guadagno, quando + durata - 0.008);
        g.gain.linearRampToValueAtTime(0, quando + durata);
      } else if (sfuma === "inizio") {
        g.gain.setValueAtTime(0, quando);
        g.gain.linearRampToValueAtTime(guadagno, quando + 0.008);
      }
      s.connect(g);
      g.connect(analisi);
      if (durata) s.start(quando, da, durata);
      else s.start(quando, da);
      return s;
    };

    var suonaProva = function () {
      suonoPronto();
      riprendere = !colonna.paused;
      prova.inCorso = true;
      tastoProva.classList.add("attiva");
      etichettaProva.textContent = tastoProva.getAttribute("data-ferma");
      if (riprendere) colonna.pause();
      aggiorna();
      carica().then(
        function (buffer) {
          if (!prova.inCorso) return;
          var pausa = opzioni.pause ? 0 : 0.9;
          var prima = opzioni.livello ? LIVELLO : Math.pow(10, -8 / 20);
          var seconda = opzioni.livello ? LIVELLO : 1;
          var t0 = ctx.currentTime + 0.06;
          var a = traccia(buffer, t0, 0, TAGLIO, prima, pausa ? "fine" : null);
          var b = traccia(buffer, t0 + TAGLIO + pausa, TAGLIO, 0, seconda, pausa ? "inizio" : null);
          b.onended = function () {
            prova.ferma(true);
          };
          sorgenti = [a, b];
          prova.seconda = t0 + TAGLIO;
        },
        function () {
          prova.ferma(true);
        },
      );
    };

    tastoProva.addEventListener("click", function () {
      if (prova.inCorso) prova.ferma(true);
      else suonaProva();
    });
    tutti("[data-interruttore]").forEach(function (b) {
      b.addEventListener("click", function () {
        var acceso = b.getAttribute("aria-checked") !== "true";
        b.setAttribute("aria-checked", acceso ? "true" : "false");
        opzioni[b.getAttribute("data-interruttore")] = acceso;
        // Cambiato a metà, la prova ricomincia: il confronto si sente subito.
        if (prova.inCorso) {
          prova.ferma(false);
          suonaProva();
        }
      });
    });
    nodoProva.hidden = false;
  }

  /* ── quello che si muove mentre suona ──
     Un ciclo solo, acceso mentre la musica suona o il disco rallenta: la
     velocità del disco (33 giri, un giro ogni 1,8 secondi, con un attimo per
     prenderli e per fermarsi), la luce, e le barre e l'onda quando la tappa
     del suono è sullo schermo. */
  var barre = document.querySelector("[data-barre]");
  var colonne = barre ? Array.prototype.slice.call(barre.querySelectorAll(":scope > span")) : [];
  // Da dove a dove salgono le barre, in dB: una musica a −18 LUFS sta attorno
  // alla linea del riferimento, a metà.
  var SOGLIA = -63;
  var ESTENSIONE = 42;
  var tela = document.querySelector("[data-onda]");
  var senzaPause = tela && tela.parentNode;
  var disegno = tela && tela.getContext && tela.getContext("2d");
  var storia = [];
  var energia = 0;
  var velocita = 0;
  var cicloAcceso = false;
  var prima = 0;
  var suonoInVista = false;
  var vivo = false;

  if (barre && "IntersectionObserver" in window) {
    new IntersectionObserver(function (righe) {
      suonoInVista = righe[0].isIntersecting;
    }).observe(barre.closest(".scena") || barre);
  }

  /* Otto bande, a passi uguali sulla scala dei toni, da 60 Hz a 12 kHz: la
     potenza media di ciascuna, in dB, più 4,5 dB per ottava. La musica ha meno
     energia in alto, e senza la pendenza le ultime barre starebbero a terra. */
  function bande() {
    var n = colonne.length;
    var hz = ctx.sampleRate / analisi.fftSize;
    var fuori = [];
    for (var i = 0; i < n; i++) {
      var da = Math.max(1, Math.round((60 * Math.pow(200, i / n)) / hz));
      var a = Math.max(da + 1, Math.round((60 * Math.pow(200, (i + 1) / n)) / hz));
      var potenza = 0;
      for (var k = da; k < a; k++) potenza += Math.pow(10, frequenze[k] / 10);
      var db = 10 * Math.log10(potenza / (a - da)) + 4.5 * Math.log2(Math.pow(200, (i + 0.5) / n));
      fuori.push(limita((db - SOGLIA) / ESTENSIONE));
    }
    return fuori;
  }

  function disegnaOnda(dopo) {
    var w = tela.width;
    var h = tela.height;
    var passo = 5;
    var quante = Math.floor(w / passo);
    disegno.clearRect(0, 0, w, h);
    for (var i = 0; i < storia.length && i < quante; i++) {
      var punto = storia[storia.length - 1 - i];
      var alt = Math.max(2, punto.a * h);
      disegno.fillStyle = punto.seconda ? "#4fd6e0" : "#b3a8ff";
      disegno.fillRect(w - (i + 1) * passo, (h - alt) / 2, 2, alt);
    }
    if (storia.length > quante) storia.splice(0, storia.length - quante);
  }

  function ascolta() {
    if (!analisi) return;
    analisi.getByteTimeDomainData(onda);
    var somma = 0;
    for (var i = 0; i < onda.length; i++) {
      var x = (onda[i] - 128) / 128;
      somma += x * x;
    }
    var rms = Math.sqrt(somma / onda.length);
    energia += (rms - energia) * 0.22;
    if (luce && !ridotto.matches) {
      luce.style.transform = "scale(" + (1 + energia * 1.3).toFixed(3) + ")";
      luce.style.opacity = Math.min(1, 0.45 + energia * 2.4).toFixed(3);
    }
    if (!suonoInVista || ridotto.matches) return;
    analisi.getFloatFrequencyData(frequenze);
    if (!vivo) {
      vivo = true;
      barre.classList.add("dal-vivo");
      if (senzaPause) senzaPause.classList.add("dal-vivo");
    }
    bande().forEach(function (l, i) {
      colonne[i].style.setProperty("--l", l.toFixed(3));
    });
    if (disegno) {
      storia.push({ a: Math.min(1, rms * 3.2), seconda: prova.inCorso && ctx.currentTime >= prova.seconda });
      disegnaOnda();
    }
  }

  function spegni() {
    vivo = false;
    storia = [];
    if (barre) barre.classList.remove("dal-vivo");
    if (senzaPause) senzaPause.classList.remove("dal-vivo");
    if (luce) {
      luce.style.transform = "";
      luce.style.opacity = "";
    }
  }

  /* Il giro del disco è un'animazione del browser, che la scheda video fa
     andare da sola: da qui cambia solo la sua velocità, e solo quando prende
     o perde i giri. In pausa si ferma dov'è. */
  var disco = document.querySelector(".fondo-disco .v-disco");
  var giro = null;
  var impostata = 0;
  function gira() {
    if (!disco || !disco.animate || Math.abs(velocita - impostata) < 0.002) return;
    if (!giro) giro = disco.animate([{ transform: "rotate(0turn)" }, { transform: "rotate(1turn)" }], { duration: 1800, iterations: Infinity });
    impostata = velocita;
    if (impostata === 0) {
      giro.pause();
      return;
    }
    giro.updatePlaybackRate(impostata);
    if (giro.playState !== "running") giro.play();
  }

  function ciclo(t) {
    var dt = prima ? Math.min(0.05, (t - prima) / 1000) : 0;
    prima = t;
    var obiettivo = suona && !ridotto.matches ? 1 : 0;
    velocita += (obiettivo - velocita) * Math.min(1, dt * 2.4);
    if (Math.abs(obiettivo - velocita) < 0.003) velocita = obiettivo;
    gira();
    if (suona) ascolta();
    if (suona || velocita > 0) {
      window.requestAnimationFrame(ciclo);
    } else {
      cicloAcceso = false;
      prima = 0;
      spegni();
    }
  }
  function avviaCiclo() {
    if (cicloAcceso) return;
    cicloAcceso = true;
    prima = 0;
    window.requestAnimationFrame(ciclo);
  }

  /* ── i titoli si mettono a fuoco ──
     Solo quelli ancora sotto lo schermo: chi è già lì non deve lampeggiare. Il
     primo sguardo lo dà un osservatore, senza chiedere misure alla pagina
     mentre si sta ancora impaginando; quello che è sotto, e quindi non si
     vede, si sfoca e aspetta. */
  if (!ridotto.matches && "IntersectionObserver" in window) {
    var fuoco = new IntersectionObserver(
      function (righe) {
        righe.forEach(function (r) {
          if (!r.isIntersecting) return;
          r.target.classList.add("a-fuoco");
          fuoco.unobserve(r.target);
        });
      },
      { rootMargin: "0px 0px -12% 0px" },
    );
    var sguardo = new IntersectionObserver(function (righe) {
      righe.forEach(function (r) {
        sguardo.unobserve(r.target);
        if (r.isIntersecting || r.boundingClientRect.top < 0) return;
        r.target.classList.add("entra");
        if (r.target.tagName === "P") r.target.classList.add("dopo");
        fuoco.observe(r.target);
      });
    });
    tutti(".viaggio-testa h2, .viaggio-testa p, .tappa-testo h2, .arrivo-testa h2, .arrivo-testa p, .domande h2, .sostieni h2").forEach(function (el) {
      sguardo.observe(el);
    });
  }

  /* ── il pulsante che si avvicina ──
     Entro 70 px dal bordo il pulsante va un poco verso il mouse. Il centro si
     misura togliendo lo spostamento già fatto, se no inseguirebbe sé stesso. */
  var magneti = tutti(".eroe .cta[data-scarica], .riga-cta .azione").map(function (el) {
    el.classList.add("magnete");
    return { el: el, x: 0, y: 0 };
  });
  var puntatore = null;
  function attira() {
    puntatore = null;
    magneti.forEach(function (c) {
      var r = c.el.getBoundingClientRect();
      var dx = c.ultimo.x - (r.left + r.width / 2 - c.x);
      var dy = c.ultimo.y - (r.top + r.height / 2 - c.y);
      var raggio = Math.max(r.width, r.height) / 2 + 70;
      var k = 1 - Math.hypot(dx, dy) / raggio;
      var x = k > 0 ? dx * 0.22 * k : 0;
      var y = k > 0 ? dy * 0.34 * k : 0;
      if (x === c.x && y === c.y) return;
      c.x = x;
      c.y = y;
      c.el.style.translate = x || y ? x.toFixed(1) + "px " + y.toFixed(1) + "px" : "";
    });
  }
  if (magneti.length) {
    document.addEventListener(
      "pointermove",
      function (ev) {
        if (!mouse.matches || ridotto.matches) return;
        magneti.forEach(function (c) {
          c.ultimo = { x: ev.clientX, y: ev.clientY };
        });
        if (!puntatore) puntatore = window.requestAnimationFrame(attira);
      },
      { passive: true },
    );
  }

  rimisuraPresto();
})();
