/**
 * La playlist aperta: i suoi brani, e i gesti che la cambiano da dentro.
 *
 * Quale playlist sia aperta lo decide `App`, che è chi instrada le pagine; qui
 * c'è quel che ne segue. Le righe, chieste a ogni apertura e a ogni cambio
 * della playlist; il segno che dice «sono arrivate» (per non far lampeggiare
 * lo stato vuoto); togliere una riga; e il riordino, col suo indicatore di
 * rilascio. Nel corpo di `App` erano sei pezzi sparsi in seicento righe, che
 * parlavano tutti della stessa cosa.
 *
 * L'hook si chiama dove stava l'effetto che chiede i brani, dopo quello che
 * rilegge l'elenco delle playlist all'apertura.
 */
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";

import { ipc, type Brano, type Playlist } from "./ipc";
import type { Libreria } from "./libreria";

/** Quel che `usePlaylistAperta` legge da `App`. */
export interface IngressiPlaylistAperta {
  /** La playlist aperta, o `null`. */
  playlistAperta: Playlist | null;
  setPlaylistAperta: Dispatch<SetStateAction<Playlist | null>>;
  /** L'elenco delle playlist, da tenere allineato a quel che cambia qui. */
  libreria: Libreria;
  segnalaErrore: (e: unknown) => void;
}

/** La playlist aperta, vista dall'elenco che la disegna. */
export interface PlaylistAperta {
  /** I brani della playlist aperta, nell'ordine della playlist. */
  braniPlaylist: Brano[];
  /** Per chi li ritocca da fuori: un cuore, un voto, un brano tolto. */
  setBraniPlaylist: Dispatch<SetStateAction<Brano[]>>;
  /** Di quale playlist sono i brani in mano, quando sono arrivati. */
  playlistCaricata: number | null;
  /** Rimpiazza una playlist nell'elenco e, se è quella aperta, anche lì. */
  aggiornaPlaylist: (cambiata: Playlist) => void;
  /** Toglie dalla playlist aperta la riga in quella posizione. */
  togliDallaPlaylist: (indice: number) => void;
  /** Sposta una riga da una posizione a un'altra. */
  spostaNellaPlaylist: (da: number, a: number) => void;
  /** La riga che si sta portando in giro, o `null`. */
  trascinata: number | null;
  /** La riga su cui cadrebbe adesso, o `null`. */
  mirata: number | null;
  /** Comincia o smette di portare una riga. */
  presa: (indice: number | null) => void;
  /** La riga su cui il puntatore la farebbe cadere. */
  mira: Dispatch<SetStateAction<number | null>>;
  /** Lascia la riga portata in questa posizione. */
  lascia: (indice: number) => void;
}

/** Vedi il commento in testa. */
export function usePlaylistAperta({
  playlistAperta,
  setPlaylistAperta,
  libreria,
  segnalaErrore,
}: IngressiPlaylistAperta): PlaylistAperta {
  const [braniPlaylist, setBraniPlaylist] = useState<Brano[]>([]);
  /**
   * Di quale playlist sono i brani in `braniPlaylist`, quando sono arrivati.
   *
   * Serve a una cosa sola: distinguere «la playlist è vuota» da «i brani non
   * sono ancora arrivati». Senza, lo stato vuoto lampeggerebbe a ogni apertura.
   */
  const [playlistCaricata, setPlaylistCaricata] = useState<number | null>(null);

  // I brani della playlist aperta. `updatedAt` fa parte della dipendenza: ogni
  // comando restituisce la playlist aggiornata, e senza quel campo un'aggiunta
  // alla playlist che si sta già guardando non ricaricherebbe l'elenco.
  //
  // Come per l'album: si azzera solo cambiando playlist — un'aggiunta a quella
  // aperta non deve farla lampeggiare vuota — e la risposta di una richiesta
  // superata si butta.
  const playlistDiPrima = useRef<number | null>(null);
  useEffect(() => {
    if (!playlistAperta) {
      playlistDiPrima.current = null;
      return;
    }
    if (playlistDiPrima.current !== playlistAperta.id) setBraniPlaylist([]);
    playlistDiPrima.current = playlistAperta.id;
    let superata = false;
    const quale = playlistAperta.id;
    ipc
      .playlistBrani(quale)
      .then((brani) => {
        if (superata) return;
        setBraniPlaylist(brani);
        setPlaylistCaricata(quale);
      })
      .catch((e: unknown) => {
        if (!superata) segnalaErrore(e);
      });
    return () => {
      superata = true;
    };
  }, [playlistAperta, segnalaErrore]);

  /** Rimpiazza una playlist nell'elenco e, se è quella aperta, anche lì. */
  const aggiornaPlaylist = useCallback(
    (cambiata: Playlist) => {
      libreria.playlist.scrivi((prima) =>
        prima.map((p) => (p.id === cambiata.id ? cambiata : p)),
      );
      setPlaylistAperta((prima) =>
        prima && prima.id === cambiata.id ? cambiata : prima,
      );
    },
    [libreria, setPlaylistAperta],
  );

  /** Toglie dalla playlist aperta la riga in quella posizione. */
  const togliDallaPlaylist = useCallback(
    (indice: number) => {
      if (!playlistAperta) return;
      ipc
        .playlistTogli(playlistAperta.id, indice)
        .then(aggiornaPlaylist)
        .catch(segnalaErrore);
    },
    [playlistAperta, aggiornaPlaylist, segnalaErrore],
  );

  /**
   * Il riordino dentro una playlist.
   *
   * Due stati e non uno: quale riga si sta portando in giro, e su quale
   * cadrebbe adesso. Il secondo esiste per **mostrarlo** — il pannello della
   * coda trascinava già senza dire dove sarebbe finita la riga, ed era il
   * difetto che il brief chiamava «nessun indicatore di rilascio».
   */
  const [trascinata, setTrascinata] = useState<number | null>(null);
  const [mirata, setMirata] = useState<number | null>(null);

  const spostaNellaPlaylist = useCallback(
    (da: number, a: number) => {
      if (!playlistAperta || da === a) return;
      // Il taglio qui e non nel nucleo: `Alt+↑` sulla prima riga e `Alt+↓`
      // sull'ultima sono gesti legittimi che non devono fare niente, non
      // errori da mostrare.
      if (a < 0 || a >= braniPlaylist.length) return;
      // Ottimistico come il cuore: la riga si muove sotto il dito, e se la
      // scrittura fallisce l'elenco si rilegge dalla playlist che torna.
      setBraniPlaylist((prima) => {
        const dopo = [...prima];
        const [presa] = dopo.splice(da, 1);
        if (presa) dopo.splice(a, 0, presa);
        return dopo;
      });
      ipc
        .playlistRiordina(playlistAperta.id, da, a)
        .then(aggiornaPlaylist)
        .catch((e: unknown) => {
          segnalaErrore(e);
          ipc
            .playlistBrani(playlistAperta.id)
            .then(setBraniPlaylist)
            .catch(segnalaErrore);
        });
    },
    [playlistAperta, braniPlaylist.length, aggiornaPlaylist, segnalaErrore],
  );

  const presa = useCallback((indice: number | null) => {
    setTrascinata(indice);
    if (indice === null) setMirata(null);
  }, []);

  const lascia = useCallback(
    (indice: number) => {
      if (trascinata !== null) spostaNellaPlaylist(trascinata, indice);
      setTrascinata(null);
      setMirata(null);
    },
    [trascinata, spostaNellaPlaylist],
  );

  return {
    braniPlaylist,
    setBraniPlaylist,
    playlistCaricata,
    aggiornaPlaylist,
    togliDallaPlaylist,
    spostaNellaPlaylist,
    trascinata,
    mirata,
    presa,
    mira: setMirata,
    lascia,
  };
}
