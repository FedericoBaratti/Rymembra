/**
 * «Configura la tua app Microsoft»: i passi nella console, e i campi in cui
 * incollare quel che la console ha dato.
 *
 * # Perché un componente solo, in tre posti
 *
 * Dalla 3.2 ogni app OAuth è dell'utente (vedi `appNuvola.ts`), e la si
 * incontra da tre parti: la finestrella «Aggiungi una cartella», quando una
 * linguetta non ha ancora la sua app; la scheda «Le tue app» delle
 * Impostazioni; il backup su Drive. Tre moduli vorrebbero dire tre copie dei
 * passi, e i passi sono la parte che invecchia — una console che cambia il
 * nome di un menu — quindi la parte da tenere in un posto solo.
 *
 * # I passi
 *
 * Numerati, come quelli dell'archivio di Spotify (`ArchivioSpotify.tsx`), e
 * per la stessa ragione: cose da fare **in ordine** su un sito che non è
 * questo, e chi torna dal browser deve ritrovare il punto in cui era. Gli
 * indirizzi di ritorno e i permessi non sono scritti qui: arrivano dal nucleo
 * (`PiattaformaApp.ritorni` e `.permessi`), che è chi li usa davvero — un
 * indirizzo copiato a mano in due posti è il modo in cui uno dei due smette di
 * essere vero. Ognuno con «Copia» accanto: vanno scritti **esatti**, e una
 * barra in più alla fine basta perché il consenso finisca su una pagina
 * d'errore del fornitore.
 *
 * Aperti quando l'app non c'è ancora, chiusi dietro un tasto quando c'è: chi
 * torna a modificarla torna per incollare, non per rileggere come si fa.
 *
 * # Il segreto
 *
 * Solo per Google, e come le altre password della finestra: `type=password`,
 * niente completamento, via dallo stato appena salvato. Modificando un'app il
 * campo può restare vuoto — vuol dire «tieni quello che c'è» —, e lo dice.
 *
 * # Le conferme
 *
 * Cambiare l'identificativo di un'app che qualcuno usa spegne i suoi conti, e
 * se è l'app del backup scollega anche quello. Lo si chiede prima, nominando
 * chi: ma la domanda la disegna chi ospita il modulo (`onChiedi`), accanto
 * alla sua finestrella e non dentro — una finestrella dentro un'altra
 * erediterebbe la sua trasformazione d'ingresso, e il velo coprirebbe soltanto
 * lei.
 */
import { useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";

import {
  cambiaIdentificativo,
  chiLaUsa,
  difettoIdApp,
  difettoSegreto,
  idCanonico,
  nomeNuovo,
  nomePulito,
  type DifettoId,
} from "../appNuvola";
import type { Domanda } from "../ConfermaGesto";
import {
  eErroreIpc,
  ipc,
  testoErrore,
  type AppDaSalvare,
  type AppNuvolaIpc,
  type AppUtente,
  type ChiavePiattaforma,
  type PiattaformaApp,
} from "../ipc";
import { t, type Chiave } from "../lingue";
import { Trans } from "../lingue/Trans";
import { Copia } from "./Copia";
import { Icona } from "./Icone";

/** Un passo nella console: cosa fare, e i valori da copiare esatti. */
interface Passo {
  cosa: ReactNode;
  valori?: readonly string[];
}

/** Il nome di un sito, scritto come lo si scrive nella barra dell'indirizzo. */
function sito(indirizzo: string): ReactNode {
  return <code>{indirizzo}</code>;
}

/**
 * I passi di una piattaforma.
 *
 * Una funzione e non una costante: i testi vengono dal catalogo delle lingue,
 * e una costante di modulo li fisserebbe sulla lingua del primo `import`.
 */
function passiDi(piattaforma: PiattaformaApp): readonly Passo[] {
  switch (piattaforma.chiave) {
    case "microsoft":
      return [
        { cosa: t("cloudApp.microsoft.step1") },
        {
          cosa: (
            <Trans k="cloudApp.microsoft.step2" v={{ console: sito("entra.microsoft.com") }} />
          ),
        },
        { cosa: t("cloudApp.microsoft.step3") },
        { cosa: t("cloudApp.microsoft.step4"), valori: piattaforma.ritorni },
        { cosa: t("cloudApp.microsoft.step5") },
        { cosa: t("cloudApp.microsoft.step6"), valori: piattaforma.permessi },
        { cosa: t("cloudApp.microsoft.step7") },
      ];
    case "dropbox":
      return [
        {
          cosa: (
            <Trans
              k="cloudApp.dropbox.step1"
              v={{ console: sito("dropbox.com/developers/apps") }}
            />
          ),
        },
        {
          cosa: (
            <Trans
              k="cloudApp.dropbox.step2"
              v={{ prima: <strong>{t("cloudApp.dropbox.step2.before")}</strong> }}
            />
          ),
          valori: piattaforma.permessi,
        },
        { cosa: t("cloudApp.dropbox.step3"), valori: piattaforma.ritorni },
        { cosa: t("cloudApp.dropbox.step4") },
        { cosa: t("cloudApp.dropbox.step5") },
      ];
    case "google":
      // Nessun passo per gli indirizzi di ritorno: un client «App desktop»
      // non ne registra (`ritorni` è vuoto), e un passo che dice «non fare
      // niente» sarebbe un passo da leggere per niente.
      return [
        {
          cosa: (
            <Trans k="cloudApp.google.step1" v={{ console: sito("console.cloud.google.com") }} />
          ),
        },
        { cosa: t("cloudApp.google.step2"), valori: piattaforma.permessi },
        { cosa: t("cloudApp.google.step3") },
        { cosa: t("cloudApp.google.step4") },
        { cosa: t("cloudApp.google.step5") },
        { cosa: t("cloudApp.google.step6") },
      ];
  }
}

/** Il tasto che apre la console, per piattaforma: dice dove porta. */
const CONSOLE: Readonly<Record<ChiavePiattaforma, Chiave>> = {
  microsoft: "cloudApp.console.microsoft",
  google: "cloudApp.console.google",
  dropbox: "cloudApp.console.dropbox",
};

/** Come si chiama l'identificativo nella console di ciascuna. */
const ETICHETTA_ID: Readonly<Record<ChiavePiattaforma, Chiave>> = {
  microsoft: "cloudApp.id.microsoft",
  google: "cloudApp.id.google",
  dropbox: "cloudApp.id.dropbox",
};

/** La riga sotto il campo dell'identificativo: com'è fatto, per riconoscerlo. */
const SUGGERIMENTO_ID: Readonly<Record<ChiavePiattaforma, Chiave>> = {
  microsoft: "cloudApp.id.hint.microsoft",
  google: "cloudApp.id.hint.google",
  dropbox: "cloudApp.id.hint.dropbox",
};

/** La forma sbagliata, detta con la forma giusta di quella piattaforma. */
const FORMA_ID: Readonly<Record<ChiavePiattaforma, Chiave>> = {
  microsoft: "cloudApp.bad.shape.microsoft",
  google: "cloudApp.bad.shape.google",
  dropbox: "cloudApp.bad.shape.dropbox",
};

/** Un esempio di forma, nel campo vuoto. */
const SEGNAPOSTO_ID: Readonly<Record<ChiavePiattaforma, string>> = {
  microsoft: "00000000-0000-0000-0000-000000000000",
  google: "….apps.googleusercontent.com",
  dropbox: "",
};

function fraseDifettoId(piattaforma: ChiavePiattaforma, difetto: DifettoId): string {
  switch (difetto) {
    case "vuoto":
      return t("cloudApp.bad.empty");
    case "spazi":
      return t("cloudApp.bad.spaces");
    case "lungo":
      return t("cloudApp.bad.long");
    case "forma":
      return t(FORMA_ID[piattaforma]);
  }
}

function fraseDifettoSegreto(difetto: DifettoId): string {
  switch (difetto) {
    case "spazi":
      return t("cloudApp.bad.secret.spaces");
    case "lungo":
      return t("cloudApp.bad.secret.long");
    // Del formato non si giura niente: «forma» vuol dire solo le virgolette.
    case "forma":
      return t("cloudApp.bad.secret.quotes");
    case "vuoto":
      return t("cloudApp.bad.secret.empty");
  }
}

/**
 * Il guasto di un salvataggio, a parole.
 *
 * `sync.busy` qui non è «una sincronizzazione in corso», che è la sua frase
 * del catalogo: è un collegamento che aspetta il consenso, o una passata del
 * backup che tiene in mano l'app di adesso. La cosa da dire è un'altra —
 * riprova fra poco —, e la si dice qui.
 */
export function fraseGuastoApp(guasto: unknown): string {
  return eErroreIpc(guasto) && guasto.code === "sync.busy"
    ? t("cloudApp.busy")
    : testoErrore(guasto);
}

/** I difetti detti dopo un invio. */
interface Difetti {
  id: DifettoId | null;
  segreto: DifettoId | null;
  nome: boolean;
}

/** Vedi il commento in testa. */
export function AppServizio({
  piattaforma,
  app,
  tutte,
  onSalvata,
  onAnnulla,
  onChiedi,
  passiAperti = false,
  fuoco,
  livello = 3,
}: {
  piattaforma: PiattaformaApp;
  /** L'app da modificare, o `null` per registrarne una nuova. */
  app: AppUtente | null;
  /** Tutte le app registrate: per proporre un nome che non c'è già. */
  tutte: readonly AppUtente[];
  /**
   * È salvata: le app come sono adesso, e quella appena scritta (`null` solo
   * se il nucleo, nel frattempo, l'avesse già tolta).
   */
  onSalvata: (dopo: AppNuvolaIpc, salvata: AppUtente | null) => void;
  /** C'è un «Annulla»: chi apre il modulo sa come richiuderlo. */
  onAnnulla?: (() => void) | undefined;
  /** Chiede conferma prima di un cambio che tocca chi usa l'app: vedi il commento in testa. */
  onChiedi: (domanda: Domanda) => void;
  /** I passi aperti anche per un'app che c'è già: per rivederli dopo un rifiuto. */
  passiAperti?: boolean | undefined;
  /**
   * Dove mettere il fuoco appena il modulo compare. Solo quando lo apre un
   * gesto: comparso da sé — una linguetta percorsa con le frecce — il modulo
   * non deve portare via il fuoco a chi lo sta muovendo.
   */
  fuoco?: "titolo" | "segreto" | undefined;
  /** Il livello del titolo, sotto quello di chi lo ospita. */
  livello?: 3 | 4 | undefined;
}) {
  const [clientId, setClientId] = useState(app?.clientId ?? "");
  const [segreto, setSegreto] = useState("");
  const [nome, setNome] = useState(() => app?.nome ?? nomeNuovo(tutte, piattaforma.nome));
  const [passi, setPassi] = useState(app === null || passiAperti);
  const [difetti, setDifetti] = useState<Difetti | null>(null);
  const [salvando, setSalvando] = useState(false);
  const [guasto, setGuasto] = useState<unknown>(null);
  const titolo = useRef<HTMLHeadingElement>(null);
  const campoId = useRef<HTMLInputElement>(null);
  const campoSegreto = useRef<HTMLInputElement>(null);
  const campoNome = useRef<HTMLInputElement>(null);
  const tastoSalva = useRef<HTMLButtonElement>(null);
  const vivo = useRef(true);
  const base = useId();
  const id = {
    titolo: `${base}-titolo`,
    passi: `${base}-passi`,
    idSugg: `${base}-id-sugg`,
    idDifetto: `${base}-id-difetto`,
    segretoSugg: `${base}-segreto-sugg`,
    segretoDifetto: `${base}-segreto-difetto`,
    nomeSugg: `${base}-nome-sugg`,
    nomeDifetto: `${base}-nome-difetto`,
  };

  useLayoutEffect(() => {
    vivo.current = true;
    if (fuoco === "titolo") titolo.current?.focus();
    else if (fuoco === "segreto") (campoSegreto.current ?? titolo.current)?.focus();
    return () => {
      vivo.current = false;
    };
    // Solo alla nascita, e apposta senza `fuoco` fra le dipendenze: è il
    // modulo che compare a prendere il fuoco, non ogni disegno.
  }, []);

  /*
   * Un salvataggio fallito rimette il fuoco su «Salva». Mentre salvava, il
   * campo da cui è partito l'Invio — o il tasto a cui la conferma l'aveva
   * restituito — era spento, e il fuoco era caduto sulla pagina: senza questo,
   * chi naviga da tastiera ripartirebbe dalla cima. Solo se il fuoco è caduto
   * davvero: chi nel frattempo è andato altrove ci resta.
   */
  useLayoutEffect(() => {
    if (guasto === null) return;
    const adesso = document.activeElement;
    if (adesso === null || adesso === document.body) tastoSalva.current?.focus();
  }, [guasto]);

  const conSegreto = piattaforma.segreto;
  const nuova = app === null;

  const salva = (dati: AppDaSalvare) => {
    setSalvando(true);
    setGuasto(null);
    ipc.nuvolaAppSalva(dati).then(
      (dopo) => {
        const salvata =
          dopo.app.find((a) =>
            dati.id !== null
              ? a.id === dati.id
              : a.piattaforma === dati.piattaforma && a.clientId === dati.clientId,
          ) ?? null;
        if (vivo.current) {
          // Il segreto se ne va appena il nucleo l'ha nel portachiavi: da qui
          // in poi tenerlo nello stato non servirebbe a nessuno.
          setSegreto("");
          setSalvando(false);
        }
        onSalvata(dopo, salvata);
      },
      (e: unknown) => {
        if (!vivo.current) return;
        setSalvando(false);
        setGuasto(e);
      },
    );
  };

  const invia = () => {
    if (salvando) return;
    const nomeBuono = nomePulito(nome);
    const difettoId = difettoIdApp(piattaforma.chiave, clientId);
    const scritto = segreto.trim() !== "";
    // Un'app Google nuova il segreto lo vuole; modificandone una, un campo
    // vuoto vuol dire «tieni quello salvato», e solo uno scritto si guarda.
    const difettoSeg = conSegreto && (nuova || scritto) ? difettoSegreto(segreto) : null;
    setDifetti({ id: difettoId, segreto: difettoSeg, nome: nomeBuono === null });
    // Il fuoco sul primo campo che non va, nell'ordine in cui si vedono.
    if (difettoId !== null) {
      campoId.current?.focus();
      return;
    }
    if (difettoSeg !== null) {
      campoSegreto.current?.focus();
      return;
    }
    if (nomeBuono === null) {
      campoNome.current?.focus();
      return;
    }
    const dati: AppDaSalvare = {
      id: app?.id ?? null,
      piattaforma: piattaforma.chiave,
      nome: nomeBuono,
      clientId: idCanonico(piattaforma.chiave, clientId),
      // Mai un segreto a Microsoft o a Dropbox: per loro è un errore, o un
      // segreto finito dove non doveva.
      segreto: conSegreto && scritto ? segreto.trim() : null,
    };
    if (app !== null && cambiaIdentificativo(app, clientId)) {
      const chi = chiLaUsa(app);
      if (chi.conti.length > 0 || chi.backup) {
        const pezzi: string[] = [];
        if (chi.conti.length > 0) {
          pezzi.push(t("cloudApp.change.confirm.accounts", { conti: chi.conti.join(", ") }));
        }
        if (chi.backup) pezzi.push(t("cloudApp.change.confirm.backup"));
        onChiedi({
          titolo: t("cloudApp.change.confirm.title", { nome: app.nome }),
          descrizione: pezzi.join(" "),
          conferma: t("cloudApp.change.confirm.ok"),
          fai: () => salva(dati),
        });
        return;
      }
    }
    salva(dati);
  };

  const apriConsole = () => {
    ipc.apriDocumento(`console-${piattaforma.chiave}`).catch((e: unknown) => {
      if (vivo.current) setGuasto(e);
    });
  };

  /** I nomi di chi descrive un campo: il difetto prima, se c'è, poi la riga di sempre. */
  const descritto = (difetto: boolean, idDifetto: string, idSugg: string) =>
    difetto ? `${idDifetto} ${idSugg}` : idSugg;

  const Titolo = livello === 4 ? "h4" : "h3";
  const suggerimentoSegreto = nuova
    ? t("cloudApp.secret.hint")
    : app.segretoSalvato
      ? t("cloudApp.secret.keep")
      : t("cloudApp.secret.missing");

  return (
    <form
      className="modulo-app"
      aria-labelledby={id.titolo}
      aria-busy={salvando}
      noValidate
      onSubmit={(e) => {
        e.preventDefault();
        invia();
      }}
    >
      <Titolo id={id.titolo} ref={titolo} className="titolo-app" tabIndex={-1}>
        {nuova
          ? t("cloudApp.title.new", { piattaforma: piattaforma.nome })
          : t("cloudApp.title.edit", { nome: app.nome })}
      </Titolo>

      <div className="tasti-app">
        <button type="button" className="bottone btn-ghost" onClick={apriConsole}>
          <Icona nome="i-external" dim={15} />
          {t(CONSOLE[piattaforma.chiave])}
        </button>
        {!nuova && (
          <button
            type="button"
            className="bottone btn-ghost"
            aria-expanded={passi}
            onClick={() => setPassi((prima) => !prima)}
          >
            <Icona nome={passi ? "i-chev-u" : "i-chev-d"} dim={15} />
            {passi ? t("cloudApp.steps.hide") : t("cloudApp.steps.show")}
          </button>
        )}
      </div>

      {passi && (
        <ol
          id={id.passi}
          className="passi"
          aria-label={t("cloudApp.steps.label", { piattaforma: piattaforma.nome })}
        >
          {passiDi(piattaforma).map((passo, i) => (
            // L'indice come chiave: i passi sono scritti qui sopra e non si
            // riordinano mai.
            // eslint-disable-next-line react/no-array-index-key
            <li key={i}>
              {passo.cosa}
              {passo.valori !== undefined && passo.valori.length > 0 && (
                <ul className="valori-app">
                  {passo.valori.map((valore) => (
                    <li key={valore}>
                      <code>{valore}</code>
                      <Copia testo={valore} />
                    </li>
                  ))}
                </ul>
              )}
            </li>
          ))}
        </ol>
      )}

      <label>
        {t(ETICHETTA_ID[piattaforma.chiave])}
        <input
          ref={campoId}
          type="text"
          className="campo mono field-input"
          value={clientId}
          placeholder={SEGNAPOSTO_ID[piattaforma.chiave]}
          autoComplete="off"
          spellCheck={false}
          disabled={salvando}
          aria-invalid={(difetti?.id ?? null) !== null || undefined}
          aria-describedby={descritto(
            (difetti?.id ?? null) !== null,
            id.idDifetto,
            id.idSugg,
          )}
          onChange={(e) => {
            setClientId(e.target.value);
            setDifetti((prima) => (prima === null ? prima : { ...prima, id: null }));
          }}
        />
      </label>
      {difetti?.id != null && (
        <p id={id.idDifetto} className="difetto-campo">
          {fraseDifettoId(piattaforma.chiave, difetti.id)}
        </p>
      )}
      <p id={id.idSugg} className="nota-campo">
        {t(SUGGERIMENTO_ID[piattaforma.chiave])}
      </p>

      {conSegreto && (
        <>
          <label>
            {t("cloudApp.secret")}
            <input
              ref={campoSegreto}
              type="password"
              className="campo mono field-input"
              value={segreto}
              autoComplete="off"
              spellCheck={false}
              disabled={salvando}
              aria-invalid={(difetti?.segreto ?? null) !== null || undefined}
              aria-describedby={descritto(
                (difetti?.segreto ?? null) !== null,
                id.segretoDifetto,
                id.segretoSugg,
              )}
              onChange={(e) => {
                setSegreto(e.target.value);
                setDifetti((prima) => (prima === null ? prima : { ...prima, segreto: null }));
              }}
            />
          </label>
          {difetti?.segreto != null && (
            <p id={id.segretoDifetto} className="difetto-campo">
              {fraseDifettoSegreto(difetti.segreto)}
            </p>
          )}
          <p
            id={id.segretoSugg}
            className="nota-campo"
            data-manca={(!nuova && !app.segretoSalvato) || undefined}
          >
            {suggerimentoSegreto}
          </p>
        </>
      )}

      <label>
        {t("cloudApp.name")}
        <input
          ref={campoNome}
          type="text"
          className="campo field-input"
          value={nome}
          maxLength={120}
          autoComplete="off"
          disabled={salvando}
          aria-invalid={difetti?.nome === true || undefined}
          aria-describedby={descritto(difetti?.nome === true, id.nomeDifetto, id.nomeSugg)}
          onChange={(e) => {
            setNome(e.target.value);
            setDifetti((prima) => (prima === null ? prima : { ...prima, nome: false }));
          }}
        />
      </label>
      {difetti?.nome === true && (
        <p id={id.nomeDifetto} className="difetto-campo">
          {t("cloudApp.bad.name")}
        </p>
      )}
      <p id={id.nomeSugg} className="nota-campo">
        {t("cloudApp.name.hint")}
      </p>

      {guasto !== null && (
        <div className="errore" role="alert">
          {fraseGuastoApp(guasto)}
        </div>
      )}

      <div className="tasti-app">
        <button
          ref={tastoSalva}
          type="submit"
          className="bottone primario btn-accent"
          disabled={salvando}
        >
          {salvando
            ? t("cloudApp.saving")
            : nuova
              ? t("cloudApp.save.new")
              : t("cloudApp.save.edit")}
        </button>
        {onAnnulla !== undefined && (
          <button type="button" className="bottone btn-ghost" disabled={salvando} onClick={onAnnulla}>
            {t("common.cancel")}
          </button>
        )}
      </div>
      <p className="solo-lettori" role="status">
        {salvando ? t("cloudApp.saving") : ""}
      </p>
    </form>
  );
}
