/**
 * La finestra.
 *
 * Non decide niente sulla libreria: chiede al nucleo e disegna quel che torna.
 * Ogni volta che qui comparisse una regola — quali file sono musica, quando due
 * brani sono lo stesso, cosa si può cancellare — sarebbe una regola che Android
 * dovrà riscrivere, ed è esattamente il modo in cui i due alberi sono divergiti.
 */
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  ipc,
  testoErrore,
  urlCopertina,
  type Album,
  type Avanzamento,
  type Avvio,
  type Brano,
  type EsitoScansione,
  type Ordine,
} from "./ipc";

type Vista = "album" | "brani" | "preferiti";

/** Millisecondi in `m:ss`, o `h:mm:ss` quando serve. */
function durata(ms: number): string {
  const totale = Math.round(ms / 1000);
  const s = totale % 60;
  const m = Math.floor(totale / 60) % 60;
  const h = Math.floor(totale / 3600);
  const dueCifre = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${dueCifre(m)}:${dueCifre(s)}` : `${m}:${dueCifre(s)}`;
}

/** Millisecondi in ore, per il pannello laterale. */
function ore(ms: number): string {
  const h = ms / 3_600_000;
  return h >= 10 ? `${Math.round(h)} h` : `${h.toFixed(1)} h`;
}

/** «1 brano», «2 brani». Metà della libreria ha un brano solo. */
function brani_(n: number): string {
  return n === 1 ? "1 brano" : `${n} brani`;
}

function Copertina({
  hash,
  titolo,
  classe = "copertina",
}: {
  hash: string | null;
  titolo: string;
  classe?: string;
}) {
  const url = urlCopertina(hash);
  if (!url) {
    return (
      <div className={`${classe} vuota`} aria-hidden="true">
        ♪
      </div>
    );
  }
  return (
    <img
      className={classe}
      src={url}
      alt={titolo}
      /* `lazy`: una griglia di novecento album non deve chiedere novecento
         immagini all'apertura, ma quelle che entrano nello schermo. */
      loading="lazy"
      decoding="async"
      draggable={false}
    />
  );
}

function RigaBrano({
  brano,
  indice,
  onPreferito,
}: {
  brano: Brano;
  indice: number;
  onPreferito: (b: Brano) => void;
}) {
  return (
    <div className="riga">
      <div className="indice">{brano.trackNumber ?? indice + 1}</div>
      <Copertina hash={brano.coverArtHash} titolo={brano.album} classe="" />
      <div className="nome" title={brano.title}>
        {brano.title}
      </div>
      <div className="autore" title={`${brano.artist} · ${brano.album}`}>
        {brano.artist} · {brano.album}
      </div>
      <div className="durata">{durata(brano.durationMs)}</div>
      <button
        type="button"
        className="cuore"
        aria-pressed={brano.liked}
        aria-label={brano.liked ? "Togli dai preferiti" : "Aggiungi ai preferiti"}
        onClick={() => onPreferito(brano)}
      >
        {brano.liked ? "♥" : "♡"}
      </button>
    </div>
  );
}

const PAGINA = 200;

export function App() {
  const [avvio, setAvvio] = useState<Avvio | null>(null);
  const [errore, setErrore] = useState<string | null>(null);
  const [vista, setVista] = useState<Vista>("album");
  const [query, setQuery] = useState("");
  const [album, setAlbum] = useState<Album[]>([]);
  const [brani, setBrani] = useState<Brano[]>([]);
  const [aperto, setAperto] = useState<Album | null>(null);
  const [braniAperto, setBraniAperto] = useState<Brano[]>([]);
  const [ordine, setOrdine] = useState<Ordine>("scaffale");
  const [scansione, setScansione] = useState<Avanzamento | null>(null);
  const [esito, setEsito] = useState<EsitoScansione | null>(null);
  const contenuto = useRef<HTMLDivElement>(null);

  const ricarica = useCallback(async () => {
    try {
      setAvvio(await ipc.avvio());
      setErrore(null);
    } catch (e) {
      setErrore(testoErrore(e));
    }
  }, []);

  useEffect(() => {
    void ricarica();
  }, [ricarica]);

  useEffect(() => {
    const promessa = listen<Avanzamento>("scansione:avanzamento", (evento) =>
      setScansione(evento.payload),
    );
    return () => {
      void promessa.then((stop) => stop());
    };
  }, []);

  // La ricerca ha la precedenza su qualunque vista: quel che si sta cercando è
  // ciò che si vuole vedere.
  const cercando = query.trim().length > 0;

  useEffect(() => {
    if (!cercando) return;
    let annullato = false;
    // Un ritardo prima di chiedere: digitando «subsonica» sarebbero nove
    // ricerche, di cui otto già superate quando tornano.
    const attesa = setTimeout(() => {
      ipc
        .cerca(query)
        .then((risultati) => {
          if (!annullato) setBrani(risultati);
        })
        .catch((e: unknown) => setErrore(testoErrore(e)));
    }, 140);
    return () => {
      annullato = true;
      clearTimeout(attesa);
    };
  }, [query, cercando]);

  const caricaVista = useCallback(async () => {
    if (cercando || !avvio) return;
    try {
      if (vista === "album") {
        setAlbum(await ipc.album(0, 400));
      } else if (vista === "brani") {
        setBrani(await ipc.brani(ordine, 0, PAGINA));
      } else {
        // I preferiti sono pochi e stanno in una pagina: quando non sarà più
        // vero servirà una query dedicata, non un limite più grande.
        const tutti = await ipc.brani("titolo", 0, 2000);
        setBrani(tutti.filter((b) => b.liked));
      }
      contenuto.current?.scrollTo({ top: 0 });
    } catch (e) {
      setErrore(testoErrore(e));
    }
  }, [vista, ordine, cercando, avvio]);

  useEffect(() => {
    void caricaVista();
  }, [caricaVista]);

  useEffect(() => {
    if (!aperto) return;
    ipc
      .braniAlbum(aperto.albumKey)
      .then(setBraniAperto)
      .catch((e: unknown) => setErrore(testoErrore(e)));
  }, [aperto]);

  const scegliCartella = async () => {
    const scelta = await open({ directory: true, multiple: false });
    if (typeof scelta !== "string" || !avvio) return;
    const cartelle = avvio.cartelle.includes(scelta)
      ? avvio.cartelle
      : [...avvio.cartelle, scelta];
    await ipc.impostaCartelle(cartelle);
    await ricarica();
  };

  const scansiona = async () => {
    setEsito(null);
    setScansione({ fatti: 0, totale: 0 });
    try {
      const risultato = await ipc.scansiona();
      setEsito(risultato);
      await ricarica();
      await caricaVista();
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setScansione(null);
    }
  };

  const cambiaPreferito = async (brano: Brano) => {
    const valore = !brano.liked;
    const aggiorna = (elenco: Brano[]) =>
      elenco.map((b) => (b.id === brano.id ? { ...b, liked: valore } : b));
    // Ottimistico: il cuoricino deve rispondere al dito, non al disco. Se la
    // scrittura fallisce si ricarica, e la riga torna com'era.
    setBrani(aggiorna);
    setBraniAperto(aggiorna);
    try {
      await ipc.preferito(brano.id, valore);
      setAvvio((prima) =>
        prima
          ? {
              ...prima,
              numeri: {
                ...prima.numeri,
                liked: prima.numeri.liked + (valore ? 1 : -1),
              },
            }
          : prima,
      );
    } catch (e) {
      setErrore(testoErrore(e));
      await caricaVista();
    }
  };

  const percentuale = useMemo(() => {
    if (!scansione || scansione.totale === 0) return 0;
    return Math.round((scansione.fatti / scansione.totale) * 100);
  }, [scansione]);

  const numeri = avvio?.numeri;
  const senzaCartelle = avvio !== null && avvio.cartelle.length === 0;
  const vuota = numeri !== undefined && numeri.tracks === 0;

  return (
    <div className="telaio">
      <header className="barra">
        <div className="marchio">
          <div className="anello" />
          Aether
        </div>
        <input
          className="ricerca"
          type="search"
          placeholder="Cerca fra titoli, artisti e album…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          spellCheck={false}
        />
        {vista === "brani" && !cercando && (
          <select
            className="bottone minuto"
            value={ordine}
            onChange={(e) => setOrdine(e.target.value as Ordine)}
          >
            <option value="scaffale">Per scaffale</option>
            <option value="recenti">Aggiunti di recente</option>
            <option value="ascoltati">Più ascoltati</option>
            <option value="titolo">Per titolo</option>
          </select>
        )}
      </header>

      <nav className="laterale">
        <div className="navigazione">
          {(
            [
              ["album", "Album", numeri?.albums],
              ["brani", "Brani", numeri?.tracks],
              ["preferiti", "Preferiti", numeri?.liked],
            ] as const
          ).map(([chiave, etichetta, conteggio]) => (
            <button
              key={chiave}
              type="button"
              className="voce"
              aria-current={vista === chiave && !cercando}
              onClick={() => {
                setQuery("");
                setAperto(null);
                setVista(chiave);
              }}
            >
              {etichetta}
              <span className="conteggio">{conteggio ?? "—"}</span>
            </button>
          ))}
        </div>

        <div className="blocco">
          <h2>Cartelle</h2>
          {avvio?.cartelle.map((c) => (
            <div className="cartella" key={c} title={c}>
              <span>{c}</span>
            </div>
          ))}
          <button type="button" className="bottone" onClick={scegliCartella}>
            Aggiungi cartella…
          </button>
          <button
            type="button"
            className="bottone primario"
            onClick={scansiona}
            disabled={scansione !== null || senzaCartelle}
          >
            {scansione ? "Scansione…" : "Scansiona"}
          </button>
          {scansione && (
            <>
              <div className="avanzamento">
                <div style={{ width: `${percentuale}%` }} />
              </div>
              <div className="conteggio">
                {scansione.totale > 0
                  ? `${scansione.fatti} / ${scansione.totale}`
                  : "confronto col disco…"}
              </div>
            </>
          )}
          {esito && (
            <div className="conteggio">
              +{esito.inseriti} · ~{esito.aggiornati} · ↦{esito.spostati} · −
              {esito.tolti} in {(esito.durataMs / 1000).toFixed(1)} s
            </div>
          )}
        </div>

        {numeri && (
          <div className="blocco">
            <h2>Libreria</h2>
            <div className="conteggio">
              {numeri.artists} artisti · {ore(numeri.durationMs)} d&apos;ascolto
            </div>
            {avvio && <div className="percorso">{avvio.dataDir}</div>}
          </div>
        )}
      </nav>

      <main className="contenuto" ref={contenuto}>
        {errore && (
          <div className="errore" style={{ marginBottom: "var(--spazio-4)" }}>
            {errore}
          </div>
        )}

        {senzaCartelle && !errore ? (
          <div className="vuoto">
            <h2>Nessuna cartella sorvegliata</h2>
            <p>
              Aggiungi la cartella dove tieni la musica: Aether la legge, non la
              sposta e non la modifica finché non glielo chiedi.
            </p>
            <button type="button" className="bottone primario" onClick={scegliCartella}>
              Scegli una cartella…
            </button>
          </div>
        ) : vuota && !scansione ? (
          <div className="vuoto">
            <h2>Libreria vuota</h2>
            <p>Le cartelle ci sono. Manca una scansione.</p>
            <button type="button" className="bottone primario" onClick={scansiona}>
              Scansiona ora
            </button>
          </div>
        ) : cercando ? (
          <>
            <h2 style={{ marginTop: 0 }}>
              {brani.length === 1 ? "1 risultato" : `${brani.length} risultati`}{" "}
              per «{query}»
            </h2>
            <div className="elenco">
              {brani.map((b, i) => (
                <RigaBrano key={b.id} brano={b} indice={i} onPreferito={cambiaPreferito} />
              ))}
            </div>
          </>
        ) : aperto ? (
          <>
            <button
              type="button"
              className="bottone minuto"
              style={{ marginBottom: "var(--spazio-3)" }}
              onClick={() => setAperto(null)}
            >
              ← Album
            </button>
            <div className="intestazione">
              <Copertina hash={aperto.coverArtHash} titolo={aperto.title} />
              <div>
                <h1>{aperto.title}</h1>
                <div className="meta">
                  {aperto.artist}
                  {aperto.year ? ` · ${aperto.year}` : ""} ·{" "}
                  {brani_(aperto.totalTracks)}
                  {aperto.genre ? ` · ${aperto.genre}` : ""}
                </div>
              </div>
            </div>
            <div className="elenco">
              {braniAperto.map((b, i) => (
                <RigaBrano key={b.id} brano={b} indice={i} onPreferito={cambiaPreferito} />
              ))}
            </div>
          </>
        ) : vista === "album" ? (
          <div className="griglia">
            {album.map((a) => (
              <button
                key={a.albumKey}
                type="button"
                className="scheda"
                onClick={() => setAperto(a)}
              >
                <Copertina hash={a.coverArtHash} titolo={a.title} />
                <div className="titolo" title={a.title}>
                  {a.title}
                </div>
                <div className="sotto" title={a.artist}>
                  {a.artist}
                  {a.totalTracks > 1 ? ` · ${a.totalTracks}` : ""}
                </div>
              </button>
            ))}
          </div>
        ) : (
          <div className="elenco">
            {brani.map((b, i) => (
              <RigaBrano key={b.id} brano={b} indice={i} onPreferito={cambiaPreferito} />
            ))}
          </div>
        )}
      </main>
    </div>
  );
}
