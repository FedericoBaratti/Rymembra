/**
 * Gli artisti della vista Artisti, tenuti in mano finché la libreria non cambia.
 *
 * Si chiedono tutti — sono un ordine di grandezza meno dei brani, e la vista li
 * mostra con un indice alfabetico invece che a pagine — ma non più a ogni
 * ritorno nella vista: quel ritorno rifaceva la richiesta e rimetteva i
 * segnaposto sopra una griglia che si stava guardando. Si richiedono quando
 * qualcosa li ha resi vecchi: `cartelle:cambiate` (una scansione, dei brani
 * scaricati, un profilo importato) o `ricaricaContorno` in `App` dopo un brano
 * tolto, che chiamano `invalidaArtisti`.
 *
 * `schermate/Artisti.tsx` disegna; qui c'è quando chiedere. L'hook si chiama
 * dove stava il suo effetto in `App`; lo stato, dichiarato in cima, nasce adesso
 * qui. Gli artisti in mano li legge anche il menù «Vai all'artista», che li
 * riusa invece di richiederli.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { ipc, type Artista } from "./ipc";
import type { Vista } from "./parti/Navigazione";

/** Gli artisti in mano, e come dire che sono vecchi. */
export interface ArtistiDellaFinestra {
  artisti: Artista[];
  /** Una richiesta è in volo: i segnaposto, ma solo la prima volta. */
  artistiInArrivo: boolean;
  /**
   * L'ultima lettura è fallita.
   *
   * Serve a non dire una bugia: senza, una lettura fallita lasciava l'elenco
   * vuoto, e la vista diceva «nessun artista» sopra una libreria che ne ha
   * duecento. Chi la disegna mostra il guasto e «Riprova», che è
   * `invalidaArtisti`.
   */
  artistiGuasto: boolean;
  /** Li rilegge subito se la vista è aperta, entrandoci se no. */
  invalidaArtisti: () => void;
}

/** Vedi il commento in testa. */
export function useArtisti(
  vista: Vista,
  cercando: boolean,
  segnalaErrore: (e: unknown) => void,
): ArtistiDellaFinestra {
  const [artisti, setArtisti] = useState<Artista[]>([]);
  /*
   * `artistiDaRileggere` è il segno, e `giroArtisti` lo fa rileggere subito se
   * la vista è aperta; se non lo è, la richiesta parte entrandoci.
   */
  const artistiDaRileggere = useRef(true);
  const [giroArtisti, setGiroArtisti] = useState(0);
  const [artistiInArrivo, setArtistiInArrivo] = useState(false);
  const [artistiGuasto, setArtistiGuasto] = useState(false);
  useEffect(() => {
    if (vista !== "artisti" || cercando) return;
    if (!artistiDaRileggere.current) return;
    artistiDaRileggere.current = false;
    setArtistiInArrivo(true);
    setArtistiGuasto(false);
    let annullato = false;
    let arrivati = false;
    ipc
      .artisti()
      .then((elenco) => {
        arrivati = true;
        if (!annullato) setArtisti(elenco);
      })
      .catch((e: unknown) => {
        artistiDaRileggere.current = true;
        if (!annullato) setArtistiGuasto(true);
        segnalaErrore(e);
      })
      .finally(() => {
        if (!annullato) setArtistiInArrivo(false);
      });
    return () => {
      annullato = true;
      // Una risposta che non si userà non ha letto niente: la prossima entrata
      // deve richiedere.
      if (!arrivati) artistiDaRileggere.current = true;
    };
  }, [vista, cercando, segnalaErrore, giroArtisti]);
  const invalidaArtisti = useCallback(() => {
    artistiDaRileggere.current = true;
    setGiroArtisti((n) => n + 1);
  }, []);

  return { artisti, artistiInArrivo, artistiGuasto, invalidaArtisti };
}
