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
 * (`accentoDaRimettere`), o che non va chiesto affatto: `null`, mentre la
 * finestra indossa la bozza di «Prova sull'app».
 *
 * # L'ordine delle due letture
 *
 * L'hook si chiama nel punto del corpo di `App` in cui stava la lettura della
 * skin all'apertura: subito dopo `avvio`, prima del tema. La rilettura
 * dell'elenco, che stava una ventina di effetti più in giù, parte adesso
 * subito dopo; sono due domande indipendenti al nucleo, e l'ordine in cui
 * partono non decide niente di quel che si vede.
 */
import { open } from "./dialoghi";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { applicaSkin } from "./aspetto";
import { ESTENSIONE_SKIN_PRECEDENTE } from "./eredita";
import {
  ipc,
  type InstallazioneSkin,
  type Skin,
  type SostituzioneSkin,
  type VersioneSkin,
  type VoceSkin,
} from "./ipc";
import { t } from "./lingue";
import { useStatoPresente } from "./presenza";
import { segnoAccento } from "./segnoAccento";
import { vinceLUltima } from "./ultima";

/**
 * Un file `.ryskin` con l'id di una skin installata e diversa: la domanda
 * aperta, e il file da reinstallare se la risposta è sì.
 */
export interface DaSostituire {
  percorso: string;
  domanda: SostituzioneSkin;
}

/**
 * Una sostituzione appena fatta: chi ha preso il posto di chi, e il file da cui
 * rimettere quella di prima. È la notizia con «Rimettila».
 */
export interface Sostituita {
  nuova: VersioneSkin;
  vecchia: VersioneSkin;
  percorso: string;
}

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
   * `null` mentre la finestra indossa la bozza di «Prova sull'app»: lì
   * l'accento non si chiede. Vedi `segnoAccento.ts`.
   */
  accentoDaRimettere: number | null;
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
  /**
   * Installa un file `.ryskin` già scelto — trascinato sulla finestra, o dal
   * selettore — chiedendo prima di sostituire una skin con lo stesso id.
   */
  installaDaFile: (percorso: string) => Promise<void>;
  /** Una skin appena installata da un file: indossata subito, o prima guardata. */
  dopoInstallazione: (installata: VoceSkin) => Promise<void>;
  /** La sostituzione che aspetta una risposta: vedi `ConfermaSostituzione`. */
  daSostituire: DaSostituire | null;
  /** «Sostituisci»: il file si reinstalla, e quella di prima va da parte. */
  confermaSostituzione: () => Promise<void>;
  /** «Annulla»: non si scrive niente. */
  annullaSostituzione: () => void;
  /** La sostituzione appena fatta, per la notizia con «Rimettila». */
  sostituita: Sostituita | null;
  /** Chiude la notizia della sostituzione. */
  chiudiSostituita: () => void;
  /**
   * Rimette la skin che un'installazione aveva sostituito: la copia tenuta da
   * parte (`VoceSkin.sostituita.percorso`) si reinstalla, e quella di adesso va
   * da parte al suo posto.
   */
  rimettiSostituita: (percorso: string) => Promise<void>;
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
export function useSkinAttiva(
  segnalaErrore: (e: unknown) => void,
  /**
   * Una scelta ha chiuso «Prova sull'app»: l'id della skin scelta, per
   * l'avviso. Vedi `scegliSkin`.
   */
  onProvaChiusa: (id: string) => void = () => undefined,
): SkinDellaFinestra {
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
  /**
   * Chiude la prova **anche nel ref**, subito: `provaAdesso` si allinea solo
   * al disegno dopo, e un gesto che arriva prima — «Tienila» che poi sceglie,
   * un secondo clic su «Prova» — vedrebbe ancora la prova chiusa.
   */
  const chiudiProva = useCallback(() => {
    provaAdesso.current = null;
    setProva(null);
  }, []);
  /** Il ref e non la funzione: chi la passa la scrive a ogni disegno. */
  const provaChiusa = useRef(onProvaChiusa);
  provaChiusa.current = onProvaChiusa;
  /**
   * La skin dell'ultima prova, come oggetto: finché è lei quella indossata
   * l'accento della copertina non si chiede. Non si azzera mai a mano — conta
   * l'identità con `skinAttiva`, che cambia appena a schermo arriva un'altra
   * skin. Vedi `segnoAccento.ts` e `accentoDaRimettere`.
   */
  const [bozzaIndossata, setBozzaIndossata] = useState<Skin | null>(null);

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
      // Una scelta durante «Prova sull'app» — dalle Impostazioni, da un tema
      // nuovo, da un `.ryskin` trascinato — chiude la prova (D-67). Fino alla
      // 3.2 la lasciava accesa: la finestra indossava la skin scelta con la
      // pillola che diceva ancora «Stai provando», e «Torna» rimetteva la
      // skin di prima della prova, contro quel che il database diceva.
      const inProva = provaAdesso.current;
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
      // Solo se è ancora la stessa prova: «Tienila» la chiude prima di
      // scegliere, e lì non c'è niente da dire.
      const chiusa = inProva !== null && provaAdesso.current === inProva;
      if (chiusa) chiudiProva();
      await ricaricaSkin();
      if (chiusa) provaChiusa.current(scelta.id);
    },
    [ricaricaSkin, rimettiDopoUnGuasto, segnalaErrore, tocca, chiudiProva],
  );

  /**
   * «Prova sull'app»: la finestra indossa la bozza compilata, lo Studio si
   * nasconde senza perdere niente, e la pillola riporta indietro.
   */
  const iniziaProva = useCallback(
    (bozza: { skin: Skin; sorgente: string; id: string }) => {
      giroSkin.current += 1;
      // Con una prova già in corso — il secondo clic di un doppio clic su
      // «Prova» — la skin «di prima» resta quella della prima: la indossata
      // adesso è già la bozza, e «Torna» la rimetterebbe addosso (R14-8).
      const nuova = { ...bozza, reale: provaAdesso.current?.reale ?? skinAttivaAdesso.current };
      provaAdesso.current = nuova;
      setProva(nuova);
      applicaSkin(bozza.skin);
      setSkinAttiva(bozza.skin);
      // Niente accento della copertina sulla bozza: il nucleo lo calcolerebbe
      // sulla skin scelta. Torna da sé con la skin che la sostituisce.
      setBozzaIndossata(bozza.skin);
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
    chiudiProva();
  }, [chiudiProva]);
  /**
   * «Tienila» in volo: un doppio clic installava due volte e sceglieva due
   * volte (R14-8). La prova resta accesa finché l'installazione non riesce —
   * se fallisce si può ancora tornare — quindi la guardia è a parte.
   */
  const tenendo = useRef(false);
  /** «Tienila»: la bozza si installa e si indossa, e si torna allo Studio. */
  const tieniProva = useCallback(async () => {
    const quale = provaAdesso.current;
    if (quale === null || tenendo.current) return;
    tenendo.current = true;
    try {
      const voce = await ipc.skinInstallaSorgente(quale.sorgente, quale.id);
      chiudiProva();
      await scegliSkin(voce.id);
    } catch (e) {
      segnalaErrore(e);
    } finally {
      tenendo.current = false;
    }
  }, [scegliSkin, segnalaErrore, chiudiProva]);
  const scordaProva = chiudiProva;

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
   *
   * Vale anche per «Prova sull'app», ma lì la bozza diventa `skinAttiva` e ci
   * resta per tutta la prova: l'effetto dell'accento ripartiva appena la prova
   * cominciava, e poi a ogni copertina nuova, e dipingeva sulla bozza il colore
   * calcolato per la skin scelta. Così il segno che esce di qui è `null` —
   * l'effetto toglie l'accento e non chiede niente — finché la bozza è la skin
   * indossata; torna un numero quando a schermo arriva un'altra skin, e
   * l'accento con lui. Vedi `segnoAccento.ts`.
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

  /**
   * Un file con l'id di una skin installata e diversa: la domanda aperta.
   *
   * # Perché si chiede
   *
   * Fino alla 3.2 il file prendeva il posto della skin installata in silenzio,
   * e il backup portava la sostituzione su Drive: la skin fatta nello Studio,
   * con i suoi caratteri e la sua miniatura, se ne andava per un file di
   * qualcun altro trascinato sulla finestra. Adesso il nucleo non scrive niente
   * e risponde con le due — nome, versione, autore — e la risposta arriva da
   * qui (`ConfermaSostituzione` in `ConfermaSkin.tsx`). Con il sì, quella di
   * prima resta da parte e si rimette con `rimettiSostituita`.
   */
  const [daSostituire, setDaSostituire] = useStatoPresente<DaSostituire | null>(null);
  /** La sostituzione appena fatta: la notizia con «Rimettila». */
  const [sostituita, setSostituita] = useStatoPresente<Sostituita | null>(null);

  /**
   * Quel che risponde un'installazione da file: la skin installata prosegue
   * come sempre, una sostituzione da chiedere apre la domanda.
   */
  const dopoRisposta = useCallback(
    async (percorso: string, risposta: InstallazioneSkin) => {
      if (risposta.daSostituire !== null) {
        setDaSostituire({ percorso, domanda: risposta.daSostituire });
        return;
      }
      if (risposta.installata !== null) await dopoInstallazione(risposta.installata);
    },
    [dopoInstallazione, setDaSostituire],
  );

  /**
   * Il trascinamento e il selettore passano tutti e due di qui: una regola sola
   * per la domanda, e nessuna strada che la salti.
   */
  const installaDaFile = useCallback(
    async (percorso: string) => {
      try {
        await dopoRisposta(percorso, await ipc.skinInstalla(percorso));
      } catch (e) {
        segnalaErrore(e);
      }
    },
    [dopoRisposta, segnalaErrore],
  );

  const confermaSostituzione = useCallback(async () => {
    const quale = daSostituire;
    if (quale === null) return;
    setDaSostituire(null);
    try {
      const risposta = await ipc.skinInstalla(quale.percorso, true);
      const installata = risposta.installata;
      if (installata === null) return;
      if (installata.sostituita !== undefined) {
        setSostituita({
          nuova: quale.domanda.nuova,
          vecchia: quale.domanda.presente,
          percorso: installata.sostituita.percorso,
        });
      }
      await dopoInstallazione(installata);
    } catch (e) {
      segnalaErrore(e);
    }
  }, [daSostituire, dopoInstallazione, segnalaErrore, setDaSostituire, setSostituita]);

  const annullaSostituzione = useCallback(() => setDaSostituire(null), [setDaSostituire]);
  const chiudiSostituita = useCallback(() => setSostituita(null), [setSostituita]);

  /**
   * La copia tenuta da parte torna al suo posto. È un'installazione con il sì
   * già dato: chi preme «Rimettila» ha già risposto, e il nucleo tiene da
   * parte quella di adesso, così il gesto si può rifare al contrario.
   */
  const rimettiSostituita = useCallback(
    async (percorso: string) => {
      setSostituita(null);
      try {
        const risposta = await ipc.skinInstalla(percorso, true);
        if (risposta.installata !== null) await dopoInstallazione(risposta.installata);
      } catch (e) {
        segnalaErrore(e);
      }
    },
    [dopoInstallazione, segnalaErrore, setSostituita],
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
      await installaDaFile(scelta);
    } catch (e) {
      segnalaErrore(e);
    }
  }, [installaDaFile, segnalaErrore]);

  return useMemo(
    () => ({
      skin,
      skinAttiva,
      prova,
      daIndossare,
      setDaIndossare,
      accentoDaRimettere: segnoAccento(skinAttiva, bozzaIndossata, accentoDaRimettere),
      ricaricaSkin,
      rileggiSkin,
      scegliSkin,
      anteprimaSkin,
      disinstallaSkin,
      installaSkin,
      installaDaFile,
      dopoInstallazione,
      daSostituire,
      confermaSostituzione,
      annullaSostituzione,
      sostituita,
      chiudiSostituita,
      rimettiSostituita,
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
      bozzaIndossata,
      accentoDaRimettere,
      ricaricaSkin,
      rileggiSkin,
      scegliSkin,
      anteprimaSkin,
      disinstallaSkin,
      installaSkin,
      installaDaFile,
      dopoInstallazione,
      daSostituire,
      confermaSostituzione,
      annullaSostituzione,
      sostituita,
      chiudiSostituita,
      rimettiSostituita,
      iniziaProva,
      finisciProva,
      tieniProva,
      scordaProva,
    ],
  );
}
