/**
 * Il collegamento via internet nella finestra: la scheda «Dispositivi» delle
 * Impostazioni, e le finestrelle che possono comparire dovunque.
 *
 * - «Un dispositivo vuole collegarsi»: qualcuno ha letto il QR mostrato qui,
 *   e aspetta un sì (`StatoLontano.richieste`). Il nome lo sceglie lui, e si
 *   dice come dichiarato; accanto, l'impronta della sua chiave, che lui
 *   mostra mentre aspetta.
 * - «Collegarsi a "PC di casa"?»: un link `rymembra://collega/…` — aperto da
 *   fuori (`lontano:link`), letto dallo scanner o incollato — col nome che
 *   dichiara e l'impronta, da confrontare con quella accanto al QR
 *   sull'altro schermo (`archivioLink`).
 * - «Accendere "Raggiungibile da internet"?»: collegarsi o mostrare un QR col
 *   collegamento spento; il nucleo non lo accende da sé.
 * - «Usare i dati mobili?»: un brano dell'altro su una rete a consumo, con
 *   «Solo Wi-Fi» su «Chiedi» (`gestoLontano` in `lontano.ts`).
 *
 * Sono tutte domande di consenso: `consenso` su `Dialogo`, che le skin non
 * ridisegnano.
 *
 * Le apre `LontanoInApp`, montato una volta in `App`: è anche lui ad ascoltare
 * `lontano:stato` e a tenere l'archivio. Vedi `lontano.ts`.
 */
import { useEffect, useId, useState } from "react";

import { useArchivio } from "../archivio";
import { Dialogo } from "../Dialogo";
import { ConfermaGesto, type Domanda } from "../ConfermaGesto";
import { dataOraBreve } from "../formato";
import { MASSIMO_NOME, nomePulito } from "../insieme";
import {
  ipc,
  type DatiMobili,
  type DispositivoLontano,
  type DispositivoSincronia,
  type FidatoInsieme,
  type InvitoLontano,
  type LinkLontano,
  type StatoLontano,
  type StatoSincronia,
  type ScaricoLontano,
} from "../ipc";
import { t, tSe } from "../lingue";
import {
  accendiPoiCollega,
  annullatoDaQui,
  archivioDatiMobili,
  archivioLink,
  archivioLontano,
  archivioScarico,
  chiediIlPannello,
  comandoDelPannello,
  eTelefono,
  eventoDelPannello,
  linkDa,
  percorsoQr,
  qrScaduto,
  qrSoloInVista,
  scaricoConNonAudio,
  scaricoFinito,
  serveAccendere,
  tentativiDiCollegamento,
  unisciDispositivi,
} from "../lontano";
import { useAscolto } from "../pagine";
import { MOBILE } from "../piattaforma";
import { Copia } from "./Copia";
import { comanda, statoDelFidato, useStatoInsieme } from "./Insieme";
import { Icona } from "./Icone";
import { Interruttore } from "./Interruttore";
import { Segmentato } from "./Segmentato";

// ── in App ──────────────────────────────────────────────────────────────────

/**
 * La notizia di fine coda di «Scarica qui»: quanti, e — se un file non era
 * musica — perché non è sceso. Il conto dei non riusciti da solo non lo dice.
 */
function testoScaricoFinito(adesso: ScaricoLontano): string {
  if (adesso.falliti === 0) return t("devices.download.done", { n: adesso.fatti });
  const conto = t("devices.download.doneWithErrors", {
    n: adesso.fatti,
    falliti: adesso.falliti,
  });
  return scaricoConNonAudio(adesso) ? `${conto} ${t("devices.download.notAudio")}` : conto;
}

/**
 * Chi è l'altro, in una domanda di collegamento: il nome che **dichiara** —
 * lo sceglie lui, e lo si dice — e l'impronta della sua chiave, che è quel
 * che si confronta con l'altro schermo.
 */
function ChiE({ nome, impronta }: { nome: string | null; impronta: string }) {
  return (
    <>
      <p dir="auto">
        {nome !== null ? t("devices.identity.declared", { nome }) : t("devices.identity.noName")}
      </p>
      <p>
        {t("devices.identity.fingerprint")} <code translate="no">{impronta}</code>
      </p>
    </>
  );
}

/**
 * Il sì per accendere «Raggiungibile da internet», a parte: collegarsi o
 * mostrare un QR lo chiedono, e il nucleo non lo accende da sé. Il no per
 * primo, col fuoco.
 */
function ConsensoRaggiungibile({
  onAccendi,
  onAnnulla,
}: {
  onAccendi: () => void;
  onAnnulla: () => void;
}) {
  return (
    <Dialogo
      titolo={t("devices.reachable.title")}
      descrizione={t(MOBILE ? "devices.reachable.sub.phone" : "devices.reachable.sub")}
      ruolo="alertdialog"
      classe="finestrella stretta glass-modal"
      consenso
      onChiudi={onAnnulla}
    >
      <div className="tasti-finestrella">
        <button type="button" className="bottone primario" data-fuoco-iniziale onClick={onAnnulla}>
          {t("common.cancel")}
        </button>
        <button type="button" className="bottone" onClick={onAccendi}>
          {t("devices.reachable.yes")}
        </button>
      </div>
    </Dialogo>
  );
}

/**
 * L'ascolto del pannello e le finestrelle che non stanno in una pagina. Uno
 * solo, in `App`.
 */
export function LontanoInApp({
  onErrore,
  onNotizia,
}: {
  onErrore: (errore: unknown) => void;
  onNotizia: (testo: string) => void;
}) {
  const stato = useArchivio(archivioLontano);
  const riprova = useArchivio(archivioDatiMobili);
  const link = useArchivio(archivioLink);
  const [collegando, setCollegando] = useState(false);
  // «Collega» col collegamento spento: prima il sì per accenderlo.
  const [accendere, setAccendere] = useState(false);
  // «Smetti di aspettare» vale anche mentre il collegamento si accende.
  const [tentativi] = useState(tentativiDiCollegamento);

  useAscolto<StatoLontano>("lontano:stato", eventoDelPannello, true, () => {
    // La fotografia dopo l'apertura dell'ascolto: se nel frattempo è arrivato
    // un evento, quello è più nuovo e la fotografia si butta (R13-10).
    comandoDelPannello(() => ipc.lontanoStato()).catch(() => undefined);
  });

  // Il link aperto da fuori: dall'evento, o — a freddo, con l'app partita
  // proprio per lui — da quello rimasto in attesa. Preso una volta sola, e
  // esaminato: la finestrella dice con chi ci si collegherebbe.
  const prendiIlLink = () => {
    ipc
      .lontanoLinkInAttesa()
      .then((preso) => (preso === null ? null : ipc.lontanoLinkEsamina(preso)))
      .then((visto) => {
        if (visto !== null) archivioLink.scrivi(visto);
      })
      .catch(onErrore);
  };
  useAscolto<string>("lontano:link", () => prendiIlLink(), true, prendiIlLink);

  // Qui si mostrava il QR, e chi l'ha letto è stato accolto: lo si dice anche
  // qui (chi l'ha letto lo sa dal suo comando).
  useAscolto<string>("lontano:collegato", (nome) =>
    onNotizia(t("devices.linkedHere", { nome })),
  );

  // «Scarica qui»: l'avanzamento per la pagina «Sfoglia», e una notizia quando
  // la coda finisce — la pagina può non essere più aperta.
  useAscolto<ScaricoLontano>(
    "lontano:scarico",
    (adesso) => {
      const prima = archivioScarico.leggi();
      archivioScarico.scrivi(adesso);
      if (!scaricoFinito(prima, adesso)) return;
      if (adesso.fatti > 0 || adesso.falliti > 0) {
        onNotizia(testoScaricoFinito(adesso));
      }
    },
    true,
    () => {
      ipc
        .lontanoScaricoStato()
        .then((adesso) => archivioScarico.scrivi(adesso))
        .catch(() => undefined);
    },
  );

  const richiesta = stato?.richieste[0] ?? null;

  const collega = (daCollegare: LinkLontano) => {
    setCollegando(true);
    comandoDelPannello(() => ipc.lontanoCollega(daCollegare.link))
      .then(() => {
        onNotizia(t("devices.linked"));
        archivioLink.scrivi(null);
      })
      .catch((errore) => {
        archivioLink.scrivi(null);
        if (!annullatoDaQui(errore)) onErrore(errore);
      })
      .finally(() => setCollegando(false));
  };

  /** «Collega»: col collegamento spento, prima il sì per accenderlo. */
  const conferma = (daCollegare: LinkLontano) => {
    if (serveAccendere(stato)) setAccendere(true);
    else collega(daCollegare);
  };

  /**
   * Il sì per accendere è arrivato: si accende, e ci si collega — se intanto
   * la finestrella non è stata lasciata (`accendiPoiCollega`).
   */
  const accendiECollega = (daCollegare: LinkLontano) => {
    setAccendere(false);
    setCollegando(true);
    const annullato = tentativi.nuovo();
    accendiPoiCollega({
      attiva: () => ipc.lontanoAttiva(true),
      collega: () => collega(daCollegare),
      annullato,
    })
      .then((partito) => {
        // Lasciato mentre si accendeva: `collega` non c'è stato, e il suo
        // `finally` nemmeno.
        if (!partito) setCollegando(false);
      })
      .catch((errore) => {
        setCollegando(false);
        if (!annullato()) onErrore(errore);
      });
  };

  /**
   * Lascia il link: smette di aspettare, se si stava aspettando — anche un
   * collegamento che si sta ancora accendendo (reperto R13-2).
   */
  const lascia = () => {
    tentativi.lascia();
    if (collegando) ipc.lontanoCollegaAnnulla().catch(() => undefined);
    setAccendere(false);
    archivioLink.scrivi(null);
  };

  const datiMobili = (scelta: DatiMobili) => {
    const gesto = riprova;
    archivioDatiMobili.scrivi(null);
    comandoDelPannello(() => ipc.lontanoDatiMobili(scelta))
      .then(() => {
        if (scelta === "si" && gesto !== null) return gesto();
        return undefined;
      })
      .catch(onErrore);
  };

  return (
    <>
      {richiesta !== null && (
        <Dialogo
          titolo={t("devices.request.title")}
          descrizione={t(
            eTelefono(richiesta.piattaforma)
              ? "devices.request.sub.phone"
              : "devices.request.sub.computer",
          )}
          ruolo="alertdialog"
          classe="finestrella stretta glass-modal"
          consenso
          onChiudi={() => {
            ipc.lontanoRispondi(richiesta.id, false).catch(onErrore);
          }}
        >
          <ChiE nome={richiesta.nome} impronta={richiesta.impronta} />
          <p>{t("devices.request.compare")}</p>
          <div className="tasti-finestrella">
            {/* Il no per primo, e col fuoco: un Invio battuto per riflesso non
                deve collegare un dispositivo che non si conosce. */}
            <button
              type="button"
              className="bottone primario"
              data-fuoco-iniziale
              onClick={() => {
                ipc.lontanoRispondi(richiesta.id, false).catch(onErrore);
              }}
            >
              {t("devices.request.refuse")}
            </button>
            <button
              type="button"
              className="bottone"
              onClick={() => {
                ipc.lontanoRispondi(richiesta.id, true).catch(onErrore);
              }}
            >
              {t("devices.request.allow")}
            </button>
          </div>
        </Dialogo>
      )}

      {link !== null && richiesta === null && !accendere && (
        <Dialogo
          titolo={
            link.nome !== null
              ? t("devices.link.title", { nome: link.nome })
              : t("devices.link.title.unnamed")
          }
          descrizione={collegando ? t("devices.link.waiting") : t("devices.link.sub")}
          classe="finestrella stretta glass-modal"
          occupato={collegando}
          consenso
          onChiudi={lascia}
        >
          <ChiE nome={link.nome} impronta={link.impronta} />
          {/* Mentre si aspetta, la propria: chi decide dall'altra parte la
              confronta con quella che vede nella sua domanda. */}
          {collegando && stato !== null && stato.impronta !== null && (
            <p>
              {t("devices.link.yours")} <code translate="no">{stato.impronta}</code>
            </p>
          )}
          <div className="tasti-finestrella">
            {/* Anche mentre si aspetta: dall'altra parte la domanda sparisce. */}
            <button
              type="button"
              className="bottone primario"
              data-fuoco-iniziale
              onClick={lascia}
            >
              {collegando ? t("devices.link.cancel") : t("common.cancel")}
            </button>
            <button
              type="button"
              className="bottone"
              disabled={collegando}
              onClick={() => conferma(link)}
            >
              {t("devices.link.confirm")}
            </button>
          </div>
        </Dialogo>
      )}

      {link !== null && richiesta === null && accendere && (
        <ConsensoRaggiungibile onAccendi={() => accendiECollega(link)} onAnnulla={lascia} />
      )}

      {riprova !== null && (
        <Dialogo
          titolo={t("devices.mobileData.title")}
          descrizione={t("devices.mobileData.sub")}
          classe="finestrella stretta glass-modal"
          consenso
          onChiudi={() => archivioDatiMobili.scrivi(null)}
        >
          <div className="tasti-finestrella">
            <button
              type="button"
              className="bottone primario"
              data-fuoco-iniziale
              onClick={() => datiMobili("no")}
            >
              {t("devices.mobileData.no")}
            </button>
            <button type="button" className="bottone" onClick={() => datiMobili("si")}>
              {t("devices.mobileData.yes")}
            </button>
          </div>
        </Dialogo>
      )}
    </>
  );
}

// ── la scheda delle Impostazioni ────────────────────────────────────────────

/** Il QR mostrato, disegnato dalla matrice: vedi `percorsoQr`. */
function Qr({ invito }: { invito: InvitoLontano }) {
  // Quattro moduli di margine per lato: è quel che la norma chiede, e senza i
  // telefoni meno pazienti non lo leggono.
  const lato = invito.lato + 8;
  return (
    <svg
      className="qr-collega"
      viewBox={`-4 -4 ${lato} ${lato}`}
      role="img"
      aria-label={t("devices.qr.aria")}
      shapeRendering="crispEdges"
    >
      <rect x={-4} y={-4} width={lato} height={lato} fill="#fff" />
      <path d={percorsoQr(invito.moduli, invito.lato)} fill="#000" />
    </svg>
  );
}

/**
 * Quanto manca alla scadenza del QR, in minuti, e se è già scaduto:
 * ridisegnato ogni secondo.
 */
function useScadenzaQr(scadeMs: number | null): { minuti: number; scaduto: boolean } | null {
  const [adesso, setAdesso] = useState(() => Date.now());
  useEffect(() => {
    if (scadeMs === null) return;
    setAdesso(Date.now());
    const conto = window.setInterval(() => setAdesso(Date.now()), 1_000);
    return () => window.clearInterval(conto);
  }, [scadeMs]);
  if (scadeMs === null) return null;
  return {
    minuti: Math.max(0, Math.ceil((scadeMs - adesso) / 60_000)),
    scaduto: qrScaduto(scadeMs, adesso),
  };
}

/**
 * Il nome di una riga, o il campo per cambiarlo: Invio salva, Esc lascia,
 * uscire dal campo salva. Un nome vuoto o uguale non manda niente.
 */
function NomeRiga({
  nome,
  id,
  aria,
  inModifica,
  massimo = 64,
  onFine,
}: {
  nome: string;
  id: string;
  aria: string;
  inModifica: boolean;
  massimo?: number;
  /** Il nome nuovo, o `null` se non è cambiato. */
  onFine: (nuovo: string | null) => void;
}) {
  if (!inModifica) {
    return (
      <span className="nome-insieme" dir="auto" title={nome} id={id}>
        {nome}
      </span>
    );
  }
  // Un campo nuovo a ogni modifica: parte dal nome di adesso, non da quello
  // che la riga aveva quando è comparsa.
  return <CampoNome nome={nome} aria={aria} massimo={massimo} onFine={onFine} />;
}

/** Il campo di `NomeRiga`, montato solo mentre si scrive. */
function CampoNome({
  nome,
  aria,
  massimo,
  onFine,
}: {
  nome: string;
  aria: string;
  massimo: number;
  onFine: (nuovo: string | null) => void;
}) {
  const [scritto, setScritto] = useState(nome);
  const fine = () => {
    const pulito = scritto.trim();
    onFine(pulito === "" || pulito === nome ? null : pulito);
  };
  return (
    <input
      type="text"
      className="campo field-input"
      aria-label={aria}
      value={scritto}
      maxLength={massimo}
      autoFocus
      onChange={(e) => setScritto(e.target.value)}
      onBlur={fine}
      onKeyDown={(e) => {
        if (e.key === "Enter") fine();
        if (e.key === "Escape") onFine(null);
      }}
    />
  );
}

/** Le parole sotto il nome: che cosa è, e che cosa fa con questo dispositivo. */
function Etichette({ parti }: { parti: readonly string[] }) {
  return <span className="stato-dispositivo">{parti.join(" · ")}</span>;
}

/** Un dispositivo collegato via internet, con quel che è anche negli altri due elenchi. */
function RigaDispositivo({
  dispositivo,
  insieme,
  sincronia,
  onSfoglia,
  onErrore,
  onDomanda,
}: {
  dispositivo: DispositivoLontano;
  insieme: FidatoInsieme | null;
  sincronia: DispositivoSincronia | null;
  onSfoglia: (chiave: string) => void;
  onErrore: (errore: unknown) => void;
  onDomanda: (domanda: Domanda) => void;
}) {
  const [rinomina, setRinomina] = useState(false);
  const idNome = useId();
  const stato = dispositivo.revocato
    ? t("devices.state.revoked")
    : dispositivo.online
      ? t("devices.state.online")
      : dispositivo.vistoMs !== null
        ? t("devices.state.seen", { quando: dataOraBreve(dispositivo.vistoMs) })
        : t("devices.state.never");
  const parti = [
    t(eTelefono(dispositivo.piattaforma) ? "devices.kind.phone" : "devices.kind.computer"),
    stato,
    // Diritti è il caso normale, e non lo si dice; il relay sì: spiega una
    // musica che arriva più lenta, e il traffico che passa da un terzo.
    ...(dispositivo.online && dispositivo.via === "relay" ? [t("devices.via.relay")] : []),
    ...(insieme !== null ? [t("devices.tag.together")] : []),
    ...(sincronia !== null ? [t("devices.tag.sync")] : []),
  ];
  return (
    <li className="riga-insieme con-diritti" data-online={dispositivo.online || undefined}>
      <span className="chi-insieme">
        <NomeRiga
          nome={dispositivo.nome}
          id={idNome}
          aria={t("devices.rename.aria", { nome: dispositivo.nome })}
          inModifica={rinomina}
          onFine={(nuovo) => {
            setRinomina(false);
            if (nuovo === null) return;
            comandoDelPannello(() => ipc.lontanoRinomina(dispositivo.chiave, nuovo)).catch(
              onErrore,
            );
          }}
        />
        <Etichette parti={parti} />
      </span>
      <span className="azioni">
        {dispositivo.online && !dispositivo.revocato && (
          <button
            type="button"
            className="bottone minuto btn-ghost"
            aria-describedby={idNome}
            onClick={() => onSfoglia(dispositivo.chiave)}
          >
            {t("devices.browse")}
          </button>
        )}
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-describedby={idNome}
          onClick={() => setRinomina(true)}
        >
          {t("devices.rename")}
        </button>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-describedby={idNome}
          onClick={() =>
            onDomanda({
              titolo: t("devices.forget.title", { nome: dispositivo.nome }),
              descrizione: t("devices.forget.sub"),
              conferma: t("devices.forget"),
              consenso: true,
              fai: () => {
                comandoDelPannello(() => ipc.lontanoDimentica(dispositivo.chiave)).catch(onErrore);
              },
            })
          }
        >
          {t("devices.forget")}
        </button>
      </span>
    </li>
  );
}

/**
 * Un PC di «Ascolta insieme» che non è collegato via internet: i tre gesti
 * che stavano nella sua scheda. «Dimentica» chiede prima, come per i
 * collegati via internet (decisione D-28): si rimedia riassociandosi, ma col
 * codice, e un clic sbagliato non lo deve costare.
 */
function RigaFidato({
  fidato,
  onErrore,
  onDomanda,
}: {
  fidato: FidatoInsieme;
  onErrore: (errore: unknown) => void;
  onDomanda: (domanda: Domanda) => void;
}) {
  const [rinomina, setRinomina] = useState(false);
  const idNome = useId();
  return (
    <li className="riga-insieme con-diritti" data-online={fidato.presente || undefined}>
      <span className="chi-insieme">
        <NomeRiga
          nome={fidato.nome}
          id={idNome}
          aria={t("settings.together.trusted.name", { nome: fidato.nome })}
          inModifica={rinomina}
          massimo={MASSIMO_NOME}
          onFine={(nuovo) => {
            setRinomina(false);
            const pulito = nuovo === null ? "" : nomePulito(nuovo);
            if (pulito === "" || pulito === fidato.nome) return;
            comanda(ipc.insiemePariRinomina(fidato.id, pulito), onErrore);
          }}
        />
        <Etichette parti={[t("devices.kind.together"), statoDelFidato(fidato)]} />
      </span>
      <span className="azioni">
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-describedby={idNome}
          onClick={() => setRinomina(true)}
        >
          {t("devices.rename")}
        </button>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-describedby={idNome}
          aria-pressed={fidato.bloccato}
          onClick={() => comanda(ipc.insiemePariBlocca(fidato.id, !fidato.bloccato), onErrore)}
        >
          {fidato.bloccato
            ? t("settings.together.trusted.unblock")
            : t("settings.together.trusted.block")}
        </button>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-describedby={idNome}
          onClick={() =>
            onDomanda({
              titolo: t("devices.forget.title", { nome: fidato.nome }),
              descrizione: t("devices.forget.together.sub"),
              conferma: t("devices.forget"),
              consenso: true,
              fai: () => comanda(ipc.insiemePariDimentica(fidato.id), onErrore),
            })
          }
        >
          {t("devices.forget")}
        </button>
      </span>
    </li>
  );
}

/**
 * Un dispositivo della sincronia che non è collegato via internet: Accetta
 * (finché non ci si fida di quel che scrive), Rinomina e Dimentica, come
 * nella scheda della sincronia. «Dimentica» chiede prima (decisione D-28).
 *
 * Rinomina ha il suo comando e non passa da «Accetta»: dare un nome
 * riconoscibile a un dispositivo prima di decidere non è decidere (R13-3). La
 * riga si aggiorna con l'evento `sincronia:stato` che il comando manda.
 */
function RigaSincronia({
  dispositivo,
  onAccoppia,
  onDimentica,
  onDomanda,
  onErrore,
}: {
  dispositivo: DispositivoSincronia;
  onAccoppia: (id: string, nome: string | null) => void;
  onDimentica: (id: string) => void;
  onDomanda: (domanda: Domanda) => void;
  onErrore: (errore: unknown) => void;
}) {
  const [rinomina, setRinomina] = useState(false);
  const idNome = useId();
  const nome = dispositivo.nome ?? dispositivo.id;
  const parti = [
    t("devices.kind.sync"),
    ...(dispositivo.fidato ? [] : [t("devices.sync.pending")]),
    ...(dispositivo.vistoMs !== null
      ? [t("devices.state.seen", { quando: dataOraBreve(dispositivo.vistoMs) })]
      : []),
  ];
  return (
    <li className="riga-insieme con-diritti">
      <span className="chi-insieme">
        <NomeRiga
          nome={nome}
          id={idNome}
          aria={t("settings.sync.deviceName", { id: dispositivo.id })}
          inModifica={rinomina}
          onFine={(nuovo) => {
            setRinomina(false);
            if (nuovo !== null) ipc.sincroniaRinomina(dispositivo.id, nuovo).catch(onErrore);
          }}
        />
        <Etichette parti={parti} />
      </span>
      <span className="azioni">
        {!dispositivo.fidato && (
          <button
            type="button"
            className="bottone minuto btn-ghost"
            aria-describedby={idNome}
            onClick={() => onAccoppia(dispositivo.id, dispositivo.nome)}
          >
            {t("settings.sync.accept")}
          </button>
        )}
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-describedby={idNome}
          onClick={() => setRinomina(true)}
        >
          {t("devices.rename")}
        </button>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-describedby={idNome}
          onClick={() =>
            onDomanda({
              titolo: t("devices.forget.title", { nome }),
              descrizione: t("devices.forget.sync.sub"),
              conferma: t("devices.forget"),
              consenso: true,
              fai: () => onDimentica(dispositivo.id),
            })
          }
        >
          {t("devices.forget")}
        </button>
      </span>
    </li>
  );
}

/**
 * La scheda «Dispositivi»: l'interruttore, il relay, i dati mobili (sul
 * telefono), «Collega un dispositivo» e l'elenco dei collegati.
 */
export function SchedaDispositivi({
  onErrore,
  onNotizia,
  onSfoglia,
  sincronia,
  onSincroniaAccoppia,
  onSincroniaDimentica,
}: {
  onErrore: (errore: unknown) => void;
  onNotizia: (testo: string) => void;
  onSfoglia: (chiave: string) => void;
  /** La sincronia, per i suoi dispositivi nell'elenco unico. */
  sincronia: StatoSincronia | null;
  onSincroniaAccoppia: (id: string, nome: string | null) => void;
  onSincroniaDimentica: (id: string) => void;
}) {
  const stato = useArchivio(archivioLontano);
  const insieme = useStatoInsieme();
  const [occupato, setOccupato] = useState(false);
  // «Mostra il QR» col collegamento spento: prima il sì per accenderlo.
  const [accendiPerQr, setAccendiPerQr] = useState(false);
  const [incollato, setIncollato] = useState("");
  const [relay, setRelay] = useState<string | null>(null);
  const [domanda, setDomanda] = useState<Domanda | null>(null);
  const scadenza = useScadenzaQr(stato?.invito?.scadeMs ?? null);
  const idIncolla = useId();
  // Il QR vale finché questa scheda lo mostra: vedi `qrSoloInVista`.
  const [qr] = useState(() => qrSoloInVista(ipc.lontanoQrAnnulla));

  // Aperta la pagina, chi non rispondeva si riprova adesso: un PC appena
  // acceso non deve aspettare la pausa lunga del nodo.
  useEffect(() => {
    ipc.lontanoCercaOra().catch(() => undefined);
  }, []);

  useEffect(() => {
    qr.monta();
    return qr.smonta;
  }, [qr]);

  if (stato === null) return <p className="nota">{t("common.loading")}</p>;

  /** Un comando che risponde col pannello, uno alla volta. */
  const fai = (comando: () => Promise<StatoLontano | InvitoLontano | void>) => {
    setOccupato(true);
    // La risposta si adotta solo se nel frattempo non è arrivato un evento più
    // nuovo (R13-10): il conto parte prima del comando.
    const adotta = chiediIlPannello();
    comando()
      .then((nuovo) => {
        if (nuovo !== undefined && "dispositivi" in nuovo) adotta(nuovo);
      })
      .catch(onErrore)
      .finally(() => setOccupato(false));
  };

  /**
   * Un link letto o incollato: non ci si collega da qui, ma dalla stessa
   * finestrella dei link aperti da fuori (`LontanoInApp`), che dice prima con
   * chi — il nome dichiarato e l'impronta.
   */
  const collega = (testo: string) => {
    const link = linkDa(testo);
    if (link === null) {
      onNotizia(t("errors.lontano.linkNonValido"));
      return;
    }
    ipc
      .lontanoLinkEsamina(link)
      .then((visto) => {
        setIncollato("");
        archivioLink.scrivi(visto);
      })
      .catch(onErrore);
  };

  /** «Mostra il QR»: col collegamento spento, prima il sì per accenderlo. */
  const mostraQr = () => {
    if (serveAccendere(stato)) setAccendiPerQr(true);
    else fai(() => qr.crea(ipc.lontanoQrCrea));
  };

  const accendiEMostraQr = () => {
    setAccendiPerQr(false);
    fai(async () => {
      await comandoDelPannello(() => ipc.lontanoAttiva(true));
      return qr.crea(ipc.lontanoQrCrea);
    });
  };

  const scansiona = () => {
    ipc
      .telefonoScansionaQr()
      .then((esito) => {
        if (esito.negato) onNotizia(t("devices.scan.denied"));
        else if (esito.link !== null) collega(esito.link);
      })
      .catch(onErrore);
  };

  const righe = unisciDispositivi(
    stato.dispositivi,
    insieme?.fidati ?? [],
    sincronia?.dispositivi ?? [],
  );

  return (
    <>
      <Interruttore
        etichetta={t("settings.devices.reachable")}
        spiegazione={t(
          MOBILE ? "settings.devices.reachable.sub.phone" : "settings.devices.reachable.sub",
        )}
        acceso={stato.attivo}
        onCambia={(acceso) => fai(() => ipc.lontanoAttiva(acceso))}
      />
      {stato.errore !== null && (
        <p className="nota" role="status">
          {tSe(stato.errore, t("errors.lontano.spento"))}
        </p>
      )}
      {MOBILE && stato.attivo && !stato.acceso && stato.errore === null && (
        <p className="nota">{t("settings.devices.asleep")}</p>
      )}

      {MOBILE && (
        <div className="riga-opzione">
          <div className="che-cosa">
            <div className="etichetta">{t("settings.devices.mobileData")}</div>
          </div>
          <Segmentato<DatiMobili>
            etichetta={t("settings.devices.mobileData")}
            scelta={stato.datiMobili}
            voci={[
              { chiave: "chiedi", etichetta: t("settings.devices.mobileData.ask") },
              { chiave: "si", etichetta: t("settings.devices.mobileData.yes") },
              { chiave: "no", etichetta: t("settings.devices.mobileData.no") },
            ]}
            onScegli={(scelta) => fai(() => ipc.lontanoDatiMobili(scelta))}
          />
        </div>
      )}

      <h3 className="titoletto">{t("settings.devices.link")}</h3>
      {stato.invito !== null && scadenza?.scaduto === true ? (
        <div className="azioni">
          <p className="nota" role="status">
            {t("settings.devices.qr.expired")}
          </p>
          <button
            type="button"
            className="bottone btn-ghost"
            disabled={occupato}
            onClick={mostraQr}
          >
            {t("settings.devices.qr.new")}
          </button>
        </div>
      ) : stato.invito !== null ? (
        <div className="collega-qr">
          <Qr invito={stato.invito} />
          <p className="nota">
            {t(MOBILE ? "settings.devices.qr.sub.phone" : "settings.devices.qr.sub")}
          </p>
          {/* Chi legge il QR la vede prima di collegarsi: deve essere questa. */}
          {stato.impronta !== null && (
            <p className="nota">
              {t("settings.devices.qr.fingerprint")}{" "}
              <code translate="no">{stato.impronta}</code>
            </p>
          )}
          <p className="nota">
            {t("settings.devices.qr.expires", { n: scadenza?.minuti ?? 0 })}
          </p>
          <div className="azioni">
            <Copia testo={stato.invito.link} etichetta={t("settings.devices.qr.copy")} />
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={occupato}
              onClick={() => fai(() => ipc.lontanoQrAnnulla())}
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      ) : (
        <div className="azioni">
          {MOBILE && (
            <button
              type="button"
              className="bottone primario btn-accent"
              disabled={occupato}
              onClick={scansiona}
            >
              <Icona nome="i-qr" dim={15} />
              {t("settings.devices.scan")}
            </button>
          )}
          <button
            type="button"
            className={MOBILE ? "bottone btn-ghost" : "bottone primario btn-accent"}
            disabled={occupato}
            onClick={mostraQr}
          >
            {t("settings.devices.qr.show")}
          </button>
        </div>
      )}
      {accendiPerQr && (
        <ConsensoRaggiungibile
          onAccendi={accendiEMostraQr}
          onAnnulla={() => setAccendiPerQr(false)}
        />
      )}

      <form
        className="azioni"
        onSubmit={(e) => {
          e.preventDefault();
          collega(incollato);
        }}
      >
        <input
          id={idIncolla}
          type="text"
          className="campo field-input"
          placeholder="rymembra://collega/…"
          aria-label={t("settings.devices.paste")}
          value={incollato}
          onChange={(e) => setIncollato(e.target.value)}
        />
        <button
          type="submit"
          className="bottone btn-ghost"
          disabled={occupato || incollato.trim() === ""}
        >
          {t("settings.devices.paste.cta")}
        </button>
      </form>

      <h3 className="titoletto">{t("settings.devices.list")}</h3>
      {/* Un elenco solo per i tre registri — collegati via internet, PC di
          «Ascolta insieme», dispositivi della sincronia — uniti dove sono lo
          stesso dispositivo: vedi `unisciDispositivi`. */}
      {righe.length === 0 ? (
        <p className="nota">{t("settings.devices.none")}</p>
      ) : (
        <ul className="elenco-insieme">
          {righe.map((riga) => {
            switch (riga.tipo) {
              case "lontano":
                return (
                  <RigaDispositivo
                    key={`lontano:${riga.lontano.chiave}`}
                    dispositivo={riga.lontano}
                    insieme={riga.insieme}
                    sincronia={riga.sincronia}
                    onSfoglia={onSfoglia}
                    onErrore={onErrore}
                    onDomanda={setDomanda}
                  />
                );
              case "insieme":
                return (
                  <RigaFidato
                    key={`insieme:${riga.insieme.id}`}
                    fidato={riga.insieme}
                    onErrore={onErrore}
                    onDomanda={setDomanda}
                  />
                );
              case "sincronia":
                return (
                  <RigaSincronia
                    key={`sincronia:${riga.sincronia.id}`}
                    dispositivo={riga.sincronia}
                    onAccoppia={onSincroniaAccoppia}
                    onDimentica={onSincroniaDimentica}
                    onDomanda={setDomanda}
                    onErrore={onErrore}
                  />
                );
            }
          })}
        </ul>
      )}

      {/* Grigio, non giallo: è un'impostazione da esperti, non un avviso. */}
      <details className="diagnosi-servizio">
        <summary>{t("settings.devices.relay")}</summary>
        <p className="nota">{t("settings.devices.relay.sub")}</p>
        <form
          className="azioni"
          onSubmit={(e) => {
            e.preventDefault();
            const scelto = (relay ?? stato.relay ?? "").trim();
            fai(() => ipc.lontanoRelay(scelto === "" ? null : scelto));
            setRelay(null);
          }}
        >
          <input
            type="url"
            className="campo field-input"
            placeholder="https://relay.esempio.it"
            aria-label={t("settings.devices.relay")}
            value={relay ?? stato.relay ?? ""}
            onChange={(e) => setRelay(e.target.value)}
          />
          <button type="submit" className="bottone btn-ghost" disabled={occupato}>
            {t("settings.devices.relay.save")}
          </button>
          {stato.relay !== null && (
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={occupato}
              onClick={() => {
                setRelay(null);
                fai(() => ipc.lontanoRelay(null));
              }}
            >
              {t("settings.devices.relay.public")}
            </button>
          )}
        </form>
      </details>

      {domanda !== null && <ConfermaGesto domanda={domanda} onChiudi={() => setDomanda(null)} />}
    </>
  );
}
