/**
 * La terza colonna: aperta o chiusa, e chi l'ha chiusa.
 *
 * Il patto è quello scritto in testa ad `App.tsx`: chiudendo la colonna il
 * lettore flottante torna identico a com'era, e sotto i 1100 pixel si chiude
 * da sé. Qui c'è il «da sé», e la memoria che serve perché il «da sé» non
 * annulli una scelta fatta a mano.
 *
 * L'hook si chiama dove stava l'ascolto del ridimensionamento in `App`; lo
 * stato, dichiarato in cima, nasce adesso qui, col suo valore di partenza
 * letto dalla finestra.
 */
import { useCallback, useEffect, useRef } from "react";

import { useStatoPresente } from "./presenza";

/**
 * Sotto questa larghezza la terza colonna si chiude da sé.
 *
 * Con la navigazione aperta, tre colonne su una finestra da 1100 lascerebbero
 * all'elenco meno spazio della sua intestazione: la griglia degli album
 * scenderebbe a due schede per riga, e le colonne «album» e «durata» delle
 * righe sparirebbero. Il minimo della finestra è 880, quindi questo caso
 * capita davvero.
 */
const LARGHEZZA_TRE_COLONNE = 1100;

/** La terza colonna, e i due modi di aprirla o chiuderla. */
export interface TerzaColonna {
  colonnaAperta: boolean;
  /** Apre o chiude senza dire chi: il giro guidato, che rimette com'era. */
  setColonnaAperta: (aperta: boolean) => void;
  /** Il gesto di chi apre o chiude la colonna a mano: vince sulla finestra. */
  colonnaAMano: (aperta: boolean) => void;
}

/** Vedi il commento in testa. */
export function useTerzaColonna(): TerzaColonna {
  const [colonnaAperta, setColonnaAperta] = useStatoPresente(
    () => window.innerWidth >= LARGHEZZA_TRE_COLONNE,
  );
  // La colonna si chiude da sé quando la finestra si stringe, e si riapre da
  // sé quando torna larga **solo se a chiuderla è stata la finestra**. Prima
  // non si riapriva mai, con la ragione che riaprirla avrebbe annullato una
  // chiusura decisa a mano — ma così annullava anche l'apertura decisa a mano:
  // chi rimpiccioliva la finestra un momento per affiancarla a un'altra
  // ritrovava la colonna chiusa per sempre. La chiusura a mano ora si ricorda a
  // parte, e quella resta.
  const colonnaChiusaDallaFinestra = useRef(false);
  // Letta dal gestore del `resize`, che si registra una volta sola: senza il
  // riferimento vedrebbe per sempre il valore del primo disegno.
  const colonnaApertaAdesso = useRef(colonnaAperta);
  colonnaApertaAdesso.current = colonnaAperta;
  useEffect(() => {
    const guarda = () => {
      const larga = window.innerWidth >= LARGHEZZA_TRE_COLONNE;
      const aperta = colonnaApertaAdesso.current;
      if (!larga && aperta) {
        colonnaChiusaDallaFinestra.current = true;
        setColonnaAperta(false);
      } else if (larga && !aperta && colonnaChiusaDallaFinestra.current) {
        colonnaChiusaDallaFinestra.current = false;
        setColonnaAperta(true);
      }
    };
    window.addEventListener("resize", guarda);
    return () => window.removeEventListener("resize", guarda);
  }, [setColonnaAperta]);
  const colonnaAMano = useCallback(
    (aperta: boolean) => {
      colonnaChiusaDallaFinestra.current = false;
      setColonnaAperta(aperta);
    },
    [setColonnaAperta],
  );
  return { colonnaAperta, setColonnaAperta, colonnaAMano };
}
