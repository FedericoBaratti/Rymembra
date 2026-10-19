/**
 * La skin che la finestra indossa, e quelle che potrebbe indossare.
 *
 * Stava nel corpo di `App`, e ci stava per una ragione che non c'è più: era
 * l'unico posto da cui la si potesse cambiare. Adesso la cambiano le
 * Impostazioni, lo Studio («Prova sull'app», «Salva e usa»), un file `.ryskin`
 * lasciato cadere sulla finestra e la conferma di una skin importata — e tutti
 * passano dalle stesse cinque funzioni, che vivono qui con lo stato che
 * toccano.
 *
 * # Cosa resta ad `App`
 *
 * Quel che non è della skin: lo Studio aperto, la finestrella che battezza un
 * tema nuovo, e il tema chiaro o scuro, che è di `aspettoFinestra.ts` e legge
 * la skin attiva da qui. L'accento della copertina sta di là per la stessa
 * ragione, e da qui riceve soltanto il segno che va rimesso
 * (`accentoDaRimettere`).
 *
 * # L'ordine delle due letture
 *
 * L'hook si chiama nel punto del corpo di `App` in cui stava la lettura della
 * skin all'apertura: subito dopo `avvio`, prima del tema. La rilettura
 * dell'elenco, che stava una ventina di effetti più in giù, parte adesso
 * subito dopo; sono due domande indipendenti al nucleo, e l'ordine in cui
 * partono non decide niente di quel che si vede.
 */
import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { applicaSkin } from "./aspetto";
import { ESTENSIONE_SKIN_PRECEDENTE } from "./eredita";
import { ipc, type Skin, type VoceSkin } from "./ipc";
import { t } from "./lingue";
import { useStatoPresente } from "./presenza";
import { vinceLUltima } from "./ultima";

/** Una bozza dello Studio provata sulla finestra. */
export interface Prova {
  skin: Skin;
  sorgente: string;
  id: string;
  /** La skin indossata prima della prova, da rimettere tornando. */
  reale: Skin | null;
}

/** Quel che la finestra sa delle skin, e come le cambia. */
export interface SkinDellaFinestra {
  /** Le skin installate. */
  skin: VoceSkin[];
  /** La skin indossata, o `null` finché il nucleo non ha risposto. */
  skinAttiva: Skin | null;
  /** La bozza che la finestra sta provando, mentre lo Studio è nascosto. */
  prova: Prova | null;
  /** La skin appena installata che aspetta d'essere guardata: vedi `ConfermaSkin.tsx`. */
  daIndossare: VoceSkin | null;
  /** Chiude la conferma della skin importata, o la apre su un'altra. */
  setDaIndossare: (voce: VoceSkin | null) => void;
  /**
   * Quante volte un'anteprima ha rimesso a schermo la skin scelta: il segno
   * per cui l'accento della copertina si ricalcola. Vedi `anteprimaSkin`.
   */
  accentoDaRimettere: number;
  /** Rilegge le skin installate. Vince l'ultima. */
  ricaricaSkin: () => Promise<void>;
  /** Rilegge e indossa la skin attiva: dopo un profilo applicato. */
  rileggiSkin: () => void;
  /** Sceglie e indossa una skin installata. */
  scegliSkin: (id: string) => Promise<void>;
  /** Prova una skin senza sceglierla; `null` torna a quella scelta. */
  anteprimaSkin: (id: string | null) => void;
  /** Toglie una skin installata. */
  disinstallaSkin: (id: string) => Promise<void>;
  /** Chiede un file `.ryskin` e lo installa. */
  installaSkin: () => Promise<void>;
  /** Una skin appena installata da un file: indossata subito, o prima guardata. */
  dopoInstallazione: (installata: VoceSkin) => Promise<void>;
  /** «Prova sull'app». */
  iniziaProva: (bozza: { skin: Skin; sorgente: string; id: string }) => void;
  /** Torna dalla prova alla skin di prima. */
  finisciProva: () => void;
  /** «Tienila»: la bozza si installa e si indossa. */
  tieniProva: () => Promise<void>;
  /** Lascia cadere la prova senza rimettere niente: lo Studio si chiude. */
  scordaProva: () => void;
}

/**
 * Segue la skin della finestra per tutta la sua vita.
 *
 * Va chiamato una volta sola, in `App`: è lo stato della finestra, e due copie
 * indosserebbero due skin a turno.
 */
export function useSkinAttiva(segnalaErrore: (e: unknown) => void): SkinDellaFinestra {
  const [skin, setSkin] = useState<VoceSkin[]>([]);
  /**
   * La skin appena installata che il nucleo vuole far guardare prima di
   * indossarla: vedi `ConfermaSkin.tsx` e `dopoInstallazione`.
   */
  const [daIndossare, setDaIndossare] = useStatoPresente<VoceSkin | null>(null);
  const [skinAttiva, setSkinAttiva] = useState<Skin | null>(null);
  /** La skin indossata adesso, per chi la legge da un gesto e non da un disegno. */
  const skinAttivaAdesso = useRef(skinAttiva);
  skinAttivaAdesso.current = skinAttiva;
  /**
   * La bozza che la finestra sta provando, mentre lo Studio è nascosto.
   *
   * Tiene anche la skin che si indossava prima, per rimetterla tornando: la
   * prova non è una scelta, e non tocca né l'elenco né il database.
   */
  const [prova, setProva] = useState<Prova | null>(null);
  const provaAdesso = useRef(prova);
  provaAdesso.current = prova;

  // La skin, appena si può. Fino a quando non arriva valgono i token che
  // `stile.css` porta sotto `:root`, che sono gli stessi della skin di serie —
  // quindi non c'è un fotogramma del colore sbagliato.
  //
  // Che questa chiamata **non fallisca** per colpa della skin scelta lo
  // garantisce il nucleo, non questo `catch`: `skin()` ripiega su quella di
  // serie e lo dichiara in `ripiego`. Qui dentro non c'è niente da fare se non
  // indossare quel che arriva, perché senza `skinAttiva` non c'è impaginazione
  // e senza impaginazione la finestra è vuota — vedi `skin.rs`. Del ripiego si
  // parla in Impostazioni, dove si va a cercare il perché.
  useEffect(() => {
    ipc
      .skin()
      .then((s) => {
        applicaSkin(s);
        setSkinAttiva(s);
      })
      .catch((e: unknown) => segnalaErrore(e));
    // Una volta sola, all'apertura: `segnalaErrore` è stabile.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Rilegge le skin installate. Vince l'ultima, come `ricarica` in `libreria.ts`. */
  const [turnoSkin] = useState(vinceLUltima);
  const ricaricaSkin = useCallback(async () => {
    const mio = turnoSkin();
    try {
      const elenco = await ipc.skinElenco();
      if (mio()) setSkin(elenco);
    } catch (e) {
      if (mio()) segnalaErrore(e);
    }
  }, [turnoSkin, segnalaErrore]);

  useEffect(() => {
    void ricaricaSkin();
  }, [ricaricaSkin]);

  /** La skin attiva riletta e indossata: un profilo applicato può averla cambiata. */
  const rileggiSkin = useCallback(() => {
    ipc
      .skin()
      .then((s) => {
        applicaSkin(s);
        setSkinAttiva(s);
      })
      .catch(segnalaErrore);
  }, [segnalaErrore]);

  /**
   * Quale richiesta di foglio è ancora quella buona.
   *
   * Scegliere una skin e provarne una sono **due chiamate asincrone senza un
   * ordine fra loro**, e finiscono nello stesso posto: il testo di un unico
   * `<style>`. Quando il puntatore lascia una scheda parte
   * `anteprima(null)`, che rimette la skin attiva *di quel momento*; se quella
   * risposta arriva dopo la scelta appena fatta, riscrive il foglio con la skin
   * di prima. La finestra resta del colore vecchio mentre l'elenco dice già
   * «in uso» sulla nuova — e siccome dipende da chi risponde prima, capita una
   * volta sì e una no.
   *
   * Un contatore, e vince l'ultima partita. Non basta ignorare le anteprime
   * durante una scelta: un'anteprima cominciata **dopo** deve poter vincere,
   * altrimenti passare col mouse sulle schede subito dopo aver scelto non
   * mostrerebbe più niente.
   */
  const giroSkin = useRef(0);

  /**
   * Quante scelte (o disinstallazioni) aspettano ancora il nucleo, e il giro
   * dell'ultima revoca che le sta aspettando.
   *
   * `anteprimaSkin(null)` rimette «la skin scelta», e la leggeva dall'elenco
   * dell'ultimo disegno. Ma finché la scelta non ha risposto l'elenco dice
   * ancora quella di prima: clic su una scheda, il puntatore esce prima che
   * l'elenco si ricarichi, e la revoca rimetteva la **vecchia** con un giro più
   * nuovo — così la scelta, arrivata dopo, non scriveva più il foglio. La
   * finestra restava della skin di prima, con l'elenco che diceva «in uso»
   * sulla nuova.
   *
   * Con una scelta in volo la revoca non chiede niente al nucleo: prende il
   * suo giro (così le anteprime partite prima non vincono più) e lo lascia
   * qui, e la scelta, quando risponde, indossa quel che il nucleo ha scritto
   * anche se il giro non è più il suo. Un'anteprima partita **dopo** la revoca
   * prende un giro ancora più nuovo, e la scelta torna a non toccare il foglio.
   */
  const scelteInVolo = useRef(0);
  const revocaRimandata = useRef<number | null>(null);
  /**
   * La skin scelta, per la revoca: dall'elenco, e prima ancora dalla risposta
   * di una scelta appena arrivata. Un ref e non l'elenco del disegno: fra la
   * risposta e il disegno con l'elenco nuovo c'è un giro di IPC, e una revoca
   * in quel giro rimetterebbe ancora la vecchia.
   */
  const sceltaAdesso = useRef<string | undefined>(undefined);
  useEffect(() => {
    sceltaAdesso.current = skin.find((s) => s.attiva)?.id;
  }, [skin]);
  /** Il foglio si scrive: è il giro della chiamata, o una revoca la aspettava. */
  const tocca = useCallback(
    (giro: number) => giro === giroSkin.current || revocaRimandata.current === giroSkin.current,
    [],
  );

  /**
   * Una scelta in volo è fallita, e una revoca la aspettava: si rimette quel
   * che il nucleo ha davvero come skin scelta. Senza argomento `skin()` legge
   * il database, che a scelta fallita è rimasto quello di prima.
   */
  const rimettiDopoUnGuasto = useCallback(() => {
    if (revocaRimandata.current !== giroSkin.current || scelteInVolo.current > 0) return;
    const giro = (giroSkin.current += 1);
    ipc
      .skin()
      .then((s) => {
        if (giro !== giroSkin.current) return;
        applicaSkin(s);
        setAccentoDaRimettere((n) => n + 1);
      })
      .catch(segnalaErrore);
  }, [segnalaErrore]);

  // `useCallback` e non una funzione qualunque: la tengono fra le dipendenze
  // `dopoInstallazione` e `tieniProva`, e attraverso loro le Impostazioni, che
  // sotto `memo` si ridisegnerebbero per una funzione nuova a ogni disegno.
  const scegliSkin = useCallback(
    async (id: string) => {
      const giro = (giroSkin.current += 1);
      scelteInVolo.current += 1;
      let scelta: Skin;
      try {
        scelta = await ipc.skinScegli(id);
      } catch (e) {
        scelteInVolo.current -= 1;
        rimettiDopoUnGuasto();
        segnalaErrore(e);
        return;
      }
      sceltaAdesso.current = scelta.id;
      scelteInVolo.current -= 1;
      // Il foglio si scrive solo se nel frattempo non è partita un'anteprima
      // più recente (vedi `scelteInVolo`); la scelta invece è già nel
      // database, quindi l'elenco e la skin attiva si aggiornano comunque.
      if (tocca(giro)) applicaSkin(scelta);
      setSkinAttiva(scelta);
      await ricaricaSkin();
    },
    [ricaricaSkin, rimettiDopoUnGuasto, segnalaErrore, tocca],
  );

  /**
   * «Prova sull'app»: la finestra indossa la bozza compilata, lo Studio si
   * nasconde senza perdere niente, e la pillola riporta indietro.
   */
  const iniziaProva = useCallback(
    (bozza: { skin: Skin; sorgente: string; id: string }) => {
      giroSkin.current += 1;
      setProva({ ...bozza, reale: skinAttivaAdesso.current });
      applicaSkin(bozza.skin);
      setSkinAttiva(bozza.skin);
    },
    [],
  );
  const finisciProva = useCallback(() => {
    const quale = provaAdesso.current;
    if (quale === null) return;
    giroSkin.current += 1;
    if (quale.reale !== null) {
      applicaSkin(quale.reale);
      setSkinAttiva(quale.reale);
    }
    setProva(null);
  }, []);
  /** «Tienila»: la bozza si installa e si indossa, e si torna allo Studio. */
  const tieniProva = useCallback(async () => {
    const quale = provaAdesso.current;
    if (quale === null) return;
    try {
      const voce = await ipc.skinInstallaSorgente(quale.sorgente, quale.id);
      setProva(null);
      await scegliSkin(voce.id);
    } catch (e) {
      segnalaErrore(e);
    }
  }, [scegliSkin, segnalaErrore]);
  const scordaProva = useCallback(() => setProva(null), []);

  /**
   * Toglie una skin installata.
   *
   * Il comando risponde con la skin che **resta attiva** — quella di serie, se
   * si è appena tolta quella indossata — quindi il foglio si riscrive con
   * quella e non c'è un istante in cui la finestra è dipinta da un pacchetto
   * che non esiste più. Passa dal contatore come la scelta e l'anteprima, per
   * la stessa ragione: se nel frattempo il puntatore è finito su un'altra
   * scheda, l'anteprima più recente vince.
   */
  const disinstallaSkin = useCallback(
    async (id: string) => {
      const giro = (giroSkin.current += 1);
      scelteInVolo.current += 1;
      let resta: Skin;
      try {
        resta = await ipc.skinDisinstalla(id);
      } catch (e) {
        scelteInVolo.current -= 1;
        rimettiDopoUnGuasto();
        segnalaErrore(e);
        return;
      }
      sceltaAdesso.current = resta.id;
      scelteInVolo.current -= 1;
      if (tocca(giro)) applicaSkin(resta);
      setSkinAttiva(resta);
      await ricaricaSkin();
    },
    [ricaricaSkin, rimettiDopoUnGuasto, segnalaErrore, tocca],
  );

  /**
   * Quante volte un'anteprima ha rimesso a schermo la skin scelta.
   *
   * `applicaSkin` toglie l'accento della copertina, e deve farlo: uno ritagliato
   * sul contrasto di un'altra skin non vale niente. Ma provare una skin col
   * mouse non cambia né il brano, né il tema, né la skin scelta — cioè nessuna
   * delle dipendenze dell'effetto che lo ricalcola, in `aspettoFinestra.ts` —
   * e senza questo contatore il passaggio del puntatore su una scheda
   * lascerebbe l'accento della skin fino al brano dopo.
   *
   * Lo rimette **solo la revoca**, non l'anteprima: mentre si guarda un'altra
   * skin il colore giusto non si può nemmeno calcolare, perché il nucleo taglia
   * l'accento sulle superfici della skin *scelta* e a schermo c'è quella
   * provata. Un'anteprima senza accento dinamico è una risposta onesta; una con
   * l'accento sbagliato no.
   */
  const [accentoDaRimettere, setAccentoDaRimettere] = useState(0);

  /**
   * Prova una skin senza sceglierla.
   *
   * `skin(id)` compila e basta: la scelta persistente la scrive solo
   * `skin_scegli`. È un comando che esisteva già e che nessuno chiamava con un
   * argomento — cioè un'anteprima già pronta nel nucleo, aspettando che
   * l'interfaccia se ne accorgesse.
   *
   * Passa dal contatore come la scelta, e per la stessa ragione: passare veloci
   * su tre schede manda tre richieste, e senza un ordine la terza può arrivare
   * per prima e lasciare a schermo la seconda. Vedi `giroSkin`. La revoca, con
   * una scelta ancora in volo, la aspetta: vedi `scelteInVolo`.
   */
  const anteprimaSkin = useCallback(
    (id: string | null) => {
      if (id === null && scelteInVolo.current > 0) {
        revocaRimandata.current = giroSkin.current += 1;
        return;
      }
      const quale = id ?? sceltaAdesso.current;
      if (quale === undefined) return;
      const giro = (giroSkin.current += 1);
      ipc
        .skin(quale)
        .then((s) => {
          if (giro !== giroSkin.current) return;
          applicaSkin(s);
          // Tornati alla skin scelta, l'accento della copertina può tornare
          // anche lui. Vedi `accentoDaRimettere`.
          if (id === null) setAccentoDaRimettere((n) => n + 1);
        })
        .catch(segnalaErrore);
    },
    [segnalaErrore],
  );

  /**
   * Una skin appena installata da un file: indossata subito, o prima guardata.
   *
   * Installare e non provare sarebbe metà del gesto: chi sceglie un file di
   * skin vuole vederla. Ma una skin arriva da altri, e quando il nucleo dice
   * che c'è qualcosa da sapere prima — un testo che non si legge, un budget
   * sforato (`esame.daConfermare`) — la si indossa dopo averlo detto. Intanto
   * è già nell'elenco, quindi l'elenco si ricarica comunque.
   */
  const dopoInstallazione = useCallback(
    async (installata: VoceSkin) => {
      if (installata.esame?.daConfermare === true) {
        setDaIndossare(installata);
        await ricaricaSkin();
        return;
      }
      await scegliSkin(installata.id);
    },
    [ricaricaSkin, scegliSkin, setDaIndossare],
  );

  const installaSkin = useCallback(async () => {
    try {
      const scelta = await open({
        multiple: false,
        // Anche l'estensione di prima: una skin salvata con la versione
        // precedente si installa ancora, e un filtro che la nascondesse la
        // renderebbe irraggiungibile. Il salvataggio scrive solo la nuova.
        filters: [{ name: t("file.skin"), extensions: ["ryskin", ESTENSIONE_SKIN_PRECEDENTE] }],
      });
      if (typeof scelta !== "string") return;
      await dopoInstallazione(await ipc.skinInstalla(scelta));
    } catch (e) {
      segnalaErrore(e);
    }
  }, [dopoInstallazione, segnalaErrore]);

  return useMemo(
    () => ({
      skin,
      skinAttiva,
      prova,
      daIndossare,
      setDaIndossare,
      accentoDaRimettere,
      ricaricaSkin,
      rileggiSkin,
      scegliSkin,
      anteprimaSkin,
      disinstallaSkin,
      installaSkin,
      dopoInstallazione,
      iniziaProva,
      finisciProva,
      tieniProva,
      scordaProva,
    }),
    [
      skin,
      skinAttiva,
      prova,
      daIndossare,
      setDaIndossare,
      accentoDaRimettere,
      ricaricaSkin,
      rileggiSkin,
      scegliSkin,
      anteprimaSkin,
      disinstallaSkin,
      installaSkin,
      dopoInstallazione,
      iniziaProva,
      finisciProva,
      tieniProva,
      scordaProva,
    ],
  );
}
