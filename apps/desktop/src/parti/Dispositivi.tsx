/**
 * Il collegamento via internet nella finestra: la scheda «Dispositivi» delle
 * Impostazioni, e le tre finestrelle che possono comparire dovunque.
 *
 * - «"Nothing Phone" vuole collegarsi»: qualcuno ha letto il QR mostrato qui,
 *   e aspetta un sì (`StatoLontano.richieste`).
 * - «Collegarsi a questo dispositivo?»: un link `rymembra://collega/…` aperto
 *   da fuori — il QR letto con la fotocamera del sistema (`lontano:link`).
 * - «Usare i dati mobili?»: un brano dell'altro su una rete a consumo, con
 *   «Solo Wi-Fi» su «Chiedi» (`gestoLontano` in `lontano.ts`).
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
  type StatoLontano,
  type StatoSincronia,
  type ScaricoLontano,
} from "../ipc";
import { t, tSe } from "../lingue";
import {
  annullatoDaQui,
  archivioDatiMobili,
  archivioLontano,
  archivioScarico,
  eTelefono,
  linkDa,
  percorsoQr,
  qrScaduto,
  scaricoConNonAudio,
  scaricoFinito,
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
  const [link, setLink] = useState<string | null>(null);
  const [collegando, setCollegando] = useState(false);

  useAscolto<StatoLontano>(
    "lontano:stato",
    (nuovo) => archivioLontano.scrivi(nuovo),
    true,
    () => {
      ipc
        .lontanoStato()
        .then((nuovo) => archivioLontano.scrivi(nuovo))
        .catch(() => undefined);
    },
  );

  // Il link aperto da fuori: dall'evento, o — a freddo, con l'app partita
  // proprio per lui — da quello rimasto in attesa. Preso una volta sola.
  const prendiIlLink = () => {
    ipc
      .lontanoLinkInAttesa()
      .then((preso) => {
        if (preso !== null) setLink(preso);
      })
      .catch(() => undefined);
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

  const collega = (daCollegare: string) => {
    setCollegando(true);
    ipc
      .lontanoCollega(daCollegare)
      .then((nuovo) => {
        archivioLontano.scrivi(nuovo);
        onNotizia(t("devices.linked"));
        setLink(null);
      })
      .catch((errore) => {
        setLink(null);
        if (!annullatoDaQui(errore)) onErrore(errore);
      })
      .finally(() => setCollegando(false));
  };

  /** Lascia il link: smette di aspettare, se si stava aspettando. */
  const lascia = () => {
    if (collegando) ipc.lontanoCollegaAnnulla().catch(() => undefined);
    setLink(null);
  };

  const datiMobili = (scelta: DatiMobili) => {
    const gesto = riprova;
    archivioDatiMobili.scrivi(null);
    ipc
      .lontanoDatiMobili(scelta)
      .then((nuovo) => {
        archivioLontano.scrivi(nuovo);
        if (scelta === "si" && gesto !== null) return gesto();
        return undefined;
      })
      .catch(onErrore);
  };

  return (
    <>
      {richiesta !== null && (
        <Dialogo
          titolo={t("devices.request.title", { nome: richiesta.nome })}
          descrizione={t(
            eTelefono(richiesta.piattaforma)
              ? "devices.request.sub.phone"
              : "devices.request.sub.computer",
          )}
          ruolo="alertdialog"
          classe="finestrella stretta glass-modal"
          onChiudi={() => {
            ipc.lontanoRispondi(richiesta.id, false).catch(onErrore);
          }}
        >
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

      {link !== null && richiesta === null && (
        <Dialogo
          titolo={t("devices.link.title")}
          descrizione={collegando ? t("devices.link.waiting") : t("devices.link.sub")}
          classe="finestrella stretta glass-modal"
          occupato={collegando}
          onChiudi={lascia}
        >
          <div className="tasti-finestrella">
            {/* Anche mentre si aspetta: dall'altra parte la domanda sparisce. */}
            <button
              type="button"
              className="bottone primario"
              data-fuoco-iniziale
              onClick={lascia}
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              className="bottone"
              disabled={collegando}
              onClick={() => collega(link)}
            >
              {t("devices.link.confirm")}
            </button>
          </div>
        </Dialogo>
      )}

      {riprova !== null && (
        <Dialogo
          titolo={t("devices.mobileData.title")}
          descrizione={t("devices.mobileData.sub")}
          classe="finestrella stretta glass-modal"
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
            ipc
              .lontanoRinomina(dispositivo.chiave, nuovo)
              .then((pannello) => archivioLontano.scrivi(pannello))
              .catch(onErrore);
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
              fai: () => {
                ipc
                  .lontanoDimentica(dispositivo.chiave)
                  .then((pannello) => archivioLontano.scrivi(pannello))
                  .catch(onErrore);
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
 * che stavano nella sua scheda. Senza domanda, come lì: si rimedia
 * riassociando, e non si perde niente che non torni.
 */
function RigaFidato({
  fidato,
  onErrore,
}: {
  fidato: FidatoInsieme;
  onErrore: (errore: unknown) => void;
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
          onClick={() => comanda(ipc.insiemePariDimentica(fidato.id), onErrore)}
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
 * nella scheda della sincronia.
 */
function RigaSincronia({
  dispositivo,
  onAccoppia,
  onDimentica,
}: {
  dispositivo: DispositivoSincronia;
  onAccoppia: (id: string, nome: string | null) => void;
  onDimentica: (id: string) => void;
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
            if (nuovo !== null) onAccoppia(dispositivo.id, nuovo);
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
          onClick={() => onDimentica(dispositivo.id)}
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
  // A parte da `occupato`: «Aspetto la conferma…» vale solo per un
  // collegamento, non per l'interruttore o per il relay.
  const [collegando, setCollegando] = useState(false);
  const [incollato, setIncollato] = useState("");
  const [relay, setRelay] = useState<string | null>(null);
  const [domanda, setDomanda] = useState<Domanda | null>(null);
  const scadenza = useScadenzaQr(stato?.invito?.scadeMs ?? null);
  const idIncolla = useId();

  // Aperta la pagina, chi non rispondeva si riprova adesso: un PC appena
  // acceso non deve aspettare la pausa lunga del nodo.
  useEffect(() => {
    ipc.lontanoCercaOra().catch(() => undefined);
  }, []);

  if (stato === null) return <p className="nota">{t("common.loading")}</p>;

  /** Un comando che risponde col pannello, uno alla volta. */
  const fai = (comando: () => Promise<StatoLontano | InvitoLontano | void>) => {
    setOccupato(true);
    comando()
      .then((nuovo) => {
        if (nuovo !== undefined && "dispositivi" in nuovo) archivioLontano.scrivi(nuovo);
      })
      .catch(onErrore)
      .finally(() => setOccupato(false));
  };

  const collega = (testo: string) => {
    const link = linkDa(testo);
    if (link === null) {
      onNotizia(t("errors.lontano.linkNonValido"));
      return;
    }
    setOccupato(true);
    setCollegando(true);
    ipc
      .lontanoCollega(link)
      .then((nuovo) => {
        archivioLontano.scrivi(nuovo);
        setIncollato("");
        onNotizia(t("devices.linked"));
      })
      .catch((errore) => {
        if (!annullatoDaQui(errore)) onErrore(errore);
      })
      .finally(() => {
        setOccupato(false);
        setCollegando(false);
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
            onClick={() => fai(() => ipc.lontanoQrCrea())}
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
            onClick={() => fai(() => ipc.lontanoQrCrea())}
          >
            {t("settings.devices.qr.show")}
          </button>
        </div>
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
      {collegando && (
        <div className="azioni">
          <p className="nota" role="status">
            {t("devices.link.waiting")}
          </p>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            onClick={() => {
              ipc.lontanoCollegaAnnulla().catch(onErrore);
            }}
          >
            {t("devices.link.cancel")}
          </button>
        </div>
      )}

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
                  />
                );
              case "sincronia":
                return (
                  <RigaSincronia
                    key={`sincronia:${riga.sincronia.id}`}
                    dispositivo={riga.sincronia}
                    onAccoppia={onSincroniaAccoppia}
                    onDimentica={onSincroniaDimentica}
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
