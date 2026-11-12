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
import { FINESTRA_MAIUSC_MS, maiuscAllAvvio } from "./tieniSkin";
import { vinceLUltima } from "./ultima";

/** La skin di serie: quella a cui riporta Maiusc tenuto all'avvio. */
const DI_SERIE = "plain";

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
  /**
   * L'impronta di quella appena installata: «Rimettila» scambia le due solo se
   * è ancora lei, e un secondo clic non torna indietro (R04b-4).
   */
  impronta: string | null;
}

/**
 * Dove si torna se chi ha indossato una skin da un file non la tiene.
 *
 * Un'altra skin installata, oppure — se il file ha sostituito proprio quella
 * indossata — la sua versione di prima, che sta fra le copie.
 */
export type Ritorno =
  | { tipo: "skin"; id: string; nome: string }
  | { tipo: "copia"; nome: string; percorso: string; impronta: string | null };

/** «Tieni questa skin?»: la skin appena indossata da un file, e il ritorno. */
export interface DaTenere {
  nome: string;
  ritorno: Ritorno;
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
  /**
   * Una skin appena installata da un file: indossata subito, o prima guardata.
   * Con un `ritorno`, indossata chiede se tenerla.
   */
  dopoInstallazione: (installata: VoceSkin, ritorno?: Ritorno | null) => Promise<void>;
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
   * Rimette la skin che l'ultima sostituzione ha messo da parte: la copia si
   * reinstalla, e quella di adesso va da parte al suo posto. Senza domande, ma
   * solo se quella installata è ancora quella della notizia.
   */
  rimettiSostituita: () => Promise<void>;
  /** «Indossala» dall'esame della skin appena installata. */
  indossaEsaminata: () => void;
  /**
   * «Non ora» dall'esame. Se il file aveva sostituito proprio la skin
   * indossata, torna la sua versione di prima: rifiutare deve voler dire non
   * indossarla (R04b-3).
   */
  nonOraEsaminata: () => void;
  /** «Tieni questa skin?» aperta, o `null`. Vedi `tieniSkin.ts`. */
  daTenere: DaTenere | null;
  /** «Tienila». */
  tieniSkin: () => void;
  /** «Torna indietro», o i quindici secondi passati senza risposta. */
  tornaIndietro: () => Promise<void>;
  /** La skin di prima, se all'avvio Maiusc tenuto ha rimesso Plain. */
  tornataAPlain: { id: string; nome: string } | null;
  /** Rimette la skin che Maiusc all'avvio aveva tolto. */
  rimettiDopoMaiusc: () => Promise<void>;
  /** Chiude la notizia di Maiusc all'avvio. */
  chiudiTornataAPlain: () => void;
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
  /** L'elenco di adesso, per chi lo legge da un gesto: i nomi del ritorno. */
  const skinAdesso = useRef(skin);
  skinAdesso.current = skin;
  /**
   * `scegliSkin`, per chi la chiama da un ascoltatore nato prima di lei: il
   * Maiusc all'avvio.
   */
  const scegliSkinRef = useRef<(id: string) => Promise<void>>(() => Promise.resolve());
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
        avvio.current = performance.now();
      })
      .catch((e: unknown) => segnalaErrore(e));
    // Una volta sola, all'apertura: `segnalaErrore` è stabile.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /**
   * Maiusc tenuto all'avvio: la finestra si rimette Plain (D-48).
   *
   * È l'uscita da una skin che rende la finestra inservibile, e per questo non
   * passa da niente che la skin disegni: un tasto tenuto mentre l'app si apre.
   * La scelta si scrive — al riavvio dopo resta Plain — e la notizia offre di
   * rimettere quella di prima. Vedi `maiuscAllAvvio` in `tieniSkin.ts`.
   */
  const avvio = useRef<number | null>(null);
  const [tornataAPlain, setTornataAPlain] = useStatoPresente<{ id: string; nome: string } | null>(
    null,
  );
  const scattato = useRef(false);
  useEffect(() => {
    const sente = (e: KeyboardEvent) => {
      const quando = avvio.current;
      if (quando === null || scattato.current) return;
      const trascorsi = performance.now() - quando;
      if (trascorsi > FINESTRA_MAIUSC_MS) {
        window.removeEventListener("keydown", sente, true);
        return;
      }
      if (!maiuscAllAvvio(e, trascorsi)) return;
      scattato.current = true;
      window.removeEventListener("keydown", sente, true);
      const prima = skinAttivaAdesso.current;
      if (prima === null || prima.id === DI_SERIE) return;
      const nome = skinAdesso.current.find((v) => v.id === prima.id)?.nome ?? prima.id;
      void scegliSkinRef.current(DI_SERIE).then(() => setTornataAPlain({ id: prima.id, nome }));
    };
    window.addEventListener("keydown", sente, true);
    return () => window.removeEventListener("keydown", sente, true);
  }, [setTornataAPlain]);

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
  scegliSkinRef.current = scegliSkin;

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
   * Il ritorno di «Tieni questa skin?» e dell'esame, mentre la domanda è
   * aperta: un ref e non lo stato della domanda, perché `daIndossare` resta la
   * voce che `App` mostra (vedi `ConfermaSkin`).
   */
  const ritornoDellEsame = useRef<Ritorno | null>(null);
  /** «Tieni questa skin?»: vedi `tieniSkin.ts`. */
  const [daTenere, setDaTenere] = useStatoPresente<DaTenere | null>(null);

  /**
   * Dove si torna se una skin indossata da un file non si tiene: la skin
   * scelta adesso, se è un'altra. `null` se non c'è niente da cui tornare —
   * è già quella, o non si sa quale fosse.
   */
  const ritornoAllaScelta = useCallback((id: string): Ritorno | null => {
    const prima = sceltaAdesso.current;
    if (prima === undefined || prima === id) return null;
    const nome = skinAdesso.current.find((v) => v.id === prima)?.nome ?? prima;
    return { tipo: "skin", id: prima, nome };
  }, []);

  /**
   * Indossa una skin arrivata da un file e chiede se tenerla (D-48).
   *
   * La domanda nasce **solo** qui, cioè dai file: «Rimettila», il Maiusc
   * all'avvio, lo Studio e un profilo applicato rimettono o scelgono skin che
   * si conoscono, e ritornare da sé dopo quindici secondi sarebbe un dispetto.
   */
  const indossaDaFile = useCallback(
    async (voce: VoceSkin, ritorno: Ritorno | null) => {
      await scegliSkin(voce.id);
      if (ritorno !== null) setDaTenere({ nome: voce.nome, ritorno });
    },
    [scegliSkin, setDaTenere],
  );

  /**
   * Una skin appena installata da un file: indossata subito, o prima guardata.
   *
   * Installare e non provare sarebbe metà del gesto: chi sceglie un file di
   * skin vuole vederla. Ma una skin arriva da altri, e quando il nucleo dice
   * che c'è qualcosa da sapere prima — un testo che non si legge, un budget
   * sforato, una cornice trasparente (`esame.daConfermare`) — la si indossa
   * dopo averlo detto. Intanto è già nell'elenco, quindi l'elenco si ricarica
   * comunque.
   *
   * `ritorno` è dove si torna se non la si tiene: senza, non si chiede.
   */
  const dopoInstallazione = useCallback(
    async (installata: VoceSkin, ritorno: Ritorno | null = null) => {
      if (installata.esame?.daConfermare === true) {
        ritornoDellEsame.current = ritorno;
        setDaIndossare(installata);
        await ricaricaSkin();
        return;
      }
      await indossaDaFile(installata, ritorno);
    },
    [indossaDaFile, ricaricaSkin, setDaIndossare],
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
      const installata = risposta.installata;
      if (installata !== null) {
        await dopoInstallazione(installata, ritornoAllaScelta(installata.id));
      }
    },
    [dopoInstallazione, ritornoAllaScelta, setDaSostituire],
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

  /**
   * «Sostituisci». Il sì porta l'impronta della skin che la domanda mostrava:
   * se nel frattempo ne è arrivata un'altra, il nucleo non scrive e la domanda
   * si riapre con quella vera (R04b-5).
   *
   * Se il file sostituisce proprio la skin indossata, il ritorno non è «la
   * skin di prima» — è la stessa — ma la sua versione di prima, che il nucleo
   * ha appena messo fra le copie (R04b-3).
   */
  const confermaSostituzione = useCallback(async () => {
    const quale = daSostituire;
    if (quale === null) return;
    setDaSostituire(null);
    try {
      const risposta = await ipc.skinInstalla(quale.percorso, true, quale.domanda.impronta);
      if (risposta.daSostituire !== null) {
        setDaSostituire({ percorso: quale.percorso, domanda: risposta.daSostituire });
        return;
      }
      const installata = risposta.installata;
      if (installata === null) return;
      const copia = installata.sostituita;
      if (copia !== undefined) {
        setSostituita({
          nuova: quale.domanda.nuova,
          vecchia: quale.domanda.presente,
          percorso: copia.percorso,
          impronta: risposta.impronta,
        });
      }
      const ritorno: Ritorno | null =
        installata.id === sceltaAdesso.current && copia !== undefined
          ? {
              tipo: "copia",
              nome: quale.domanda.presente.nome,
              percorso: copia.percorso,
              impronta: risposta.impronta,
            }
          : ritornoAllaScelta(installata.id);
      await dopoInstallazione(installata, ritorno);
    } catch (e) {
      segnalaErrore(e);
    }
  }, [
    daSostituire,
    dopoInstallazione,
    ritornoAllaScelta,
    segnalaErrore,
    setDaSostituire,
    setSostituita,
  ]);

  const annullaSostituzione = useCallback(() => setDaSostituire(null), [setDaSostituire]);
  const chiudiSostituita = useCallback(() => setSostituita(null), [setSostituita]);

  /**
   * Una copia tenuta da parte torna al suo posto, e si indossa: la versione di
   * prima della skin che si sta indossando. Senza domanda di ritorno — è quella
   * che c'era — e solo se l'installata è ancora quella che il gesto sa di
   * togliere: altrimenti il nucleo non scrive, e la domanda si apre.
   */
  const rimettiCopia = useCallback(
    async (percorso: string, impronta: string | null) => {
      const risposta = await ipc.skinInstalla(percorso, true, impronta);
      if (risposta.daSostituire !== null) {
        setDaSostituire({ percorso, domanda: risposta.daSostituire });
        return;
      }
      if (risposta.installata !== null) await scegliSkin(risposta.installata.id);
    },
    [scegliSkin, setDaSostituire],
  );

  /**
   * «Rimettila» dalla notizia. È un'installazione col sì già dato: chi la preme
   * ha già risposto, e il nucleo tiene da parte quella di adesso, così il gesto
   * si può rifare al contrario. L'impronta lega il gesto alla skin che la
   * notizia nomina: un secondo clic arrivato prima che la notizia sparisca
   * trova installata un'altra skin, e non scambia di nuovo (R04b-4).
   */
  const rimettiSostituita = useCallback(async () => {
    const quale = sostituita;
    if (quale === null) return;
    setSostituita(null);
    setDaTenere(null);
    try {
      await rimettiCopia(quale.percorso, quale.impronta);
    } catch (e) {
      segnalaErrore(e);
    }
  }, [rimettiCopia, segnalaErrore, setDaTenere, setSostituita, sostituita]);

  /** «Indossala» dall'esame: si indossa, e si chiede se tenerla. */
  const indossaEsaminata = useCallback(() => {
    const quale = daIndossare;
    if (quale === null) return;
    const ritorno = ritornoDellEsame.current;
    ritornoDellEsame.current = null;
    setDaIndossare(null);
    void indossaDaFile(quale, ritorno);
  }, [daIndossare, indossaDaFile, setDaIndossare]);

  /**
   * «Non ora» dall'esame. Di solito non cambia niente: la skin resta
   * installata e non indossata. Ma se il file ha sostituito proprio quella
   * indossata, la scelta punta già all'id nuovo, e al primo riavvio — o al
   * primo passaggio del puntatore sulle schede — si sarebbe indossata la
   * versione mai accettata. Torna quella di prima, dalle copie (R04b-3).
   */
  const nonOraEsaminata = useCallback(() => {
    const ritorno = ritornoDellEsame.current;
    ritornoDellEsame.current = null;
    setDaIndossare(null);
    if (ritorno?.tipo !== "copia") return;
    setSostituita(null);
    rimettiCopia(ritorno.percorso, ritorno.impronta).catch(segnalaErrore);
  }, [rimettiCopia, segnalaErrore, setDaIndossare, setSostituita]);

  const tieniSkin = useCallback(() => setDaTenere(null), [setDaTenere]);

  /** «Torna indietro», o il tempo scaduto: la skin di prima, o la versione di prima. */
  const tornaIndietro = useCallback(async () => {
    const quale = daTenere;
    if (quale === null) return;
    setDaTenere(null);
    try {
      if (quale.ritorno.tipo === "skin") {
        await scegliSkin(quale.ritorno.id);
      } else {
        setSostituita(null);
        await rimettiCopia(quale.ritorno.percorso, quale.ritorno.impronta);
      }
    } catch (e) {
      segnalaErrore(e);
    }
  }, [daTenere, rimettiCopia, scegliSkin, segnalaErrore, setDaTenere, setSostituita]);

  /** «Rimettila» dalla notizia di Maiusc all'avvio. */
  const rimettiDopoMaiusc = useCallback(async () => {
    const quale = tornataAPlain;
    if (quale === null) return;
    setTornataAPlain(null);
    await scegliSkin(quale.id);
  }, [scegliSkin, setTornataAPlain, tornataAPlain]);
  const chiudiTornataAPlain = useCallback(() => setTornataAPlain(null), [setTornataAPlain]);


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
      indossaEsaminata,
      nonOraEsaminata,
      daTenere,
      tieniSkin,
      tornaIndietro,
      tornataAPlain,
      rimettiDopoMaiusc,
      chiudiTornataAPlain,
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
      indossaEsaminata,
      nonOraEsaminata,
      daTenere,
      tieniSkin,
      tornaIndietro,
      tornataAPlain,
      rimettiDopoMaiusc,
      chiudiTornataAPlain,
      iniziaProva,
      finisciProva,
      tieniProva,
      scordaProva,
    ],
  );
}
