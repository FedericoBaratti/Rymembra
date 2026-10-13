/**
 * Le cartelle sorvegliate, dal lato dei gesti: aggiungerne una, toglierla,
 * scegliere dove finiscono i brani scaricati, e la domanda sul trasloco.
 *
 * Quali cartelle siano, e cosa voglia dire che due sono la stessa, lo decide
 * il nucleo (`radici::ordina_cartelle`). Qui c'è quel che la finestra fa con la
 * sua risposta: rileggere `avvio`, aprire la conferma del trasloco
 * (`ConfermaTrasloco.tsx`) quando arriva una gemella, dire in una riga quando
 * una cartella era già dentro un'altra. Li usano le Impostazioni, lo stato
 * vuoto «nessuna cartella» e i file trascinati sulla finestra.
 *
 * Le funzioni si rifanno a ogni disegno, come quando stavano nel corpo di
 * `App`: leggono le cartelle di `avvio` di quel disegno. Chi le passa sotto
 * `memo` le avvolge in `useStabile`.
 */
import { open } from "@tauri-apps/plugin-dialog";
import type { Dispatch, SetStateAction } from "react";

import { ipc, type Avvio, type CartellaGemella, type CartelleOrdinate } from "./ipc";
import { t } from "./lingue";
import { useStatoPresente } from "./presenza";

/** Quel che le cartelle leggono da `App`. */
export interface IngressiCartelle {
  avvio: Avvio | null;
  /** Rilegge `avvio`: vedi `libreria.ts`. */
  ricarica: () => Promise<void>;
  setNotizia: (testo: string) => void;
  segnalaErrore: (e: unknown) => void;
}

/** I gesti sulle cartelle sorvegliate, e la domanda aperta. */
export interface CartelleSorvegliate {
  /** La gemella che aspetta una risposta: vedi `ConfermaTrasloco.tsx`. */
  gemella: CartellaGemella | null;
  setGemella: Dispatch<SetStateAction<CartellaGemella | null>>;
  /** Chiede una cartella e la aggiunge alle sorvegliate. */
  scegliCartella: () => Promise<void>;
  /** Quel che il nucleo ha deciso delle cartelle appena scritte, detto. */
  dopoLeCartelle: (esito: CartelleOrdinate) => void;
  /** Il «sì» della domanda sul trasloco. */
  trasloca: (quale: CartellaGemella) => Promise<void>;
  /** Chiede dove finiscono i brani scaricati. */
  scegliCartellaDownload: () => Promise<void>;
  /** Smette di sorvegliare una cartella. */
  togliCartella: (percorso: string) => Promise<void>;
}

/** Vedi il commento in testa. */
export function useCartelleSorvegliate({
  avvio,
  ricarica,
  setNotizia,
  segnalaErrore,
}: IngressiCartelle): CartelleSorvegliate {
  /**
   * Una cartella appena scelta che il nucleo ha riconosciuto come una già
   * sorvegliata scritta in un altro modo: vedi `ConfermaTrasloco.tsx`.
   */
  const [gemella, setGemella] = useStatoPresente<CartellaGemella | null>(null);

  const scegliCartella = async () => {
    try {
      const scelta = await open({ directory: true, multiple: false });
      if (typeof scelta !== "string" || !avvio) return;
      const cartelle = avvio.cartelle.includes(scelta)
        ? avvio.cartelle
        : [...avvio.cartelle, scelta];
      const esito = await ipc.impostaCartelle(cartelle);
      await ricarica();
      dopoLeCartelle(esito);
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * Quel che il nucleo ha deciso delle cartelle appena scritte, detto a chi le
   * ha scelte.
   *
   * Una gemella — la stessa cartella con un altro nome — apre la domanda sul
   * trasloco; una cartella assorbita — già dentro un'altra — si dice in una
   * riga. Una alla volta: sceglierne due insieme che siano tutte e due così è
   * il caso raro, e la seconda si ritrova riprovando.
   */
  const dopoLeCartelle = (esito: CartelleOrdinate) => {
    const prima = esito.gemelle[0];
    if (prima) {
      setGemella(prima);
      return;
    }
    const assorbita = esito.assorbite[0];
    if (assorbita) {
      setNotizia(
        t("folders.absorbed", {
          cartella: assorbita.cartella,
          dentro: assorbita.dentro,
        }),
      );
    }
  };

  /**
   * Il «sì» della domanda sul trasloco.
   *
   * Prima si riscrivono i percorsi dei brani e la cartella sorvegliata; poi,
   * se la cartella scelta **conteneva** quella già sorvegliata, la si aggiunge:
   * adesso le due hanno la stessa grafia, e la madre raccoglie la figlia senza
   * che nessun brano cambi nome una seconda volta.
   */
  const trasloca = async (quale: CartellaGemella) => {
    try {
      const fatto = await ipc.cartelleTrasloca(quale.esistente, quale.a);
      if (quale.a !== quale.nuova && avvio) {
        const rimappate = avvio.cartelle.map((c) =>
          c === quale.esistente ? quale.a : c,
        );
        await ipc.impostaCartelle([...rimappate, quale.nuova]);
      }
      setGemella(null);
      await ricarica();
      setNotizia(t("folders.moved", { n: fatto.percorsi }));
    } catch (e) {
      setGemella(null);
      segnalaErrore(e);
    }
  };

  /**
   * Sceglie dove finiscono i brani scaricati.
   *
   * Il dialogo si apre sulla cartella già scelta, o sulla prima sorvegliata:
   * chi la cambia quasi sempre la sposta lì accanto, e ripartire dalla radice
   * del disco ogni volta è la differenza fra due clic e otto.
   */
  const scegliCartellaDownload = async () => {
    try {
      const partenza = avvio?.cartellaDownload ?? avvio?.cartelle[0];
      const scelta = await open({
        directory: true,
        multiple: false,
        ...(partenza === undefined ? {} : { defaultPath: partenza }),
      });
      if (typeof scelta !== "string") return;
      await ipc.impostaCartellaDownload(scelta);
      await ricarica();
    } catch (e) {
      segnalaErrore(e);
    }
  };

  const togliCartella = async (percorso: string) => {
    if (!avvio) return;
    try {
      await ipc.impostaCartelle(avvio.cartelle.filter((c) => c !== percorso));
      await ricarica();
    } catch (e) {
      segnalaErrore(e);
    }
  };

  return {
    gemella,
    setGemella,
    scegliCartella,
    dopoLeCartelle,
    trasloca,
    scegliCartellaDownload,
    togliCartella,
  };
}
