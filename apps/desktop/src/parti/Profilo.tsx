/**
 * Il profilo: portarsi dietro Aether su un altro computer.
 *
 * # Cosa è cambiato, e cosa questo pannello deve dire adesso
 *
 * Fino alla 2.3.0 il profilo era un `.json` di diciassette preferenze, e questa
 * schermata poteva dire «un file con le tue scelte» ed essere completa. Dalla
 * 2.3.1 è un archivio `.aeprofile` che porta anche ascolti, voti, playlist,
 * correzioni, testi, copertine e pacchetti skin — e allora tre cose che prima
 * non c'erano diventano obbligatorie da mostrare:
 *
 * 1. **Quanto pesa.** Un file da trecento megabyte non si mette su una
 *    chiavetta senza saperlo.
 * 2. **Le radici che di qua non esistono, e dove metterle.** È il caso normale
 *    quando il profilo arriva da un'altra macchina, e senza la rimappatura la
 *    libreria che si importa nomina un disco che non c'è.
 * 3. **Che l'annullamento rimette solo la configurazione.** Ascolti sommati e
 *    playlist fuse non si disfano: dirlo dopo, o non dirlo, sarebbe una
 *    promessa che il bottone non può mantenere.
 *
 * # Perché il piano, anche qui
 *
 * Importare un profilo cambia scelte fatte a mano — la skin, il volume, le
 * cartelle sorvegliate — e adesso anche la libreria. È la stessa famiglia di
 * `Ripristino.tsx` e `Importa.tsx`: prima si **legge** cosa cambierebbe, poi si
 * conferma. `profilo_piano` è la stessa funzione di `profilo_importa` dentro
 * una transazione che viene abbandonata, quindi l'elenco che si legge qui non è
 * una previsione: è il risultato.
 *
 * # Perché l'elenco delle chiavi lasciate si mostra
 *
 * Il profilo porta un elenco di **inclusioni**: `nuvola.dispositivo`, la coda
 * di riproduzione, l'uscita audio e la latenza non devono viaggiare, perché
 * descrivono questa macchina e non chi ascolta. Il rovescio di un elenco di
 * inclusioni è che dimenticarsi una chiave è silenzioso — quindi non lo è:
 * l'esportazione dice quali chiavi ha lasciato indietro, e chi ne riconosce una
 * che invece voleva può dirlo.
 */
import { useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";

import type {
  AvanzamentoProfilo,
  EsportazioneProfilo,
  PianoProfilo,
  RimappaturaRadice,
} from "../ipc";
import { ipc } from "../ipc";
import { useAscolto } from "../pagine";
import { Icona } from "./Icone";
import type { Chiave } from "../lingue";
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";
import { dataOra } from "../formato";

/** Come si legge una data del profilo. */
function quandoScritto(ms: number): string {
  return ms <= 0 ? t("profile.unknownDate") : dataOra(ms);
}

/**
 * Il peso di un file, in megabyte.
 *
 * Non in `formato.ts` perché è l'unico posto dell'interfaccia in cui un numero
 * di byte si mostra a qualcuno, e una funzione condivisa con un solo chiamante
 * è una funzione che si va a cercare per niente. Megabyte fissi e non l'unità
 * più adatta: un profilo sta fra i cinque e i cinquecento, e un'unità che
 * cambia da sola renderebbe due esportazioni non confrontabili a colpo
 * d'occhio.
 */
function inMegabyte(byte: number): number {
  return Math.round(byte / (1024 * 1024));
}

/** Un valore, accorciato quanto basta a stare su una riga. */
function breve(valore: string): string {
  return valore.length > 60 ? `${valore.slice(0, 57)}…` : valore;
}

/**
 * Le voci del riepilogo che hanno un numero diverso da zero.
 *
 * Solo quelle: un elenco di dodici righe di cui dieci dicono «0» è un elenco
 * che non si legge, e la riga che conta ci si perde dentro.
 *
 * Si tiene la **chiave** e non la frase già tradotta, perché il numero deve
 * entrare dentro `t()`: è l'unico modo perché «1 ascolto» e «2 ascolti» siano
 * due frasi e non un numero appiccicato davanti a un plurale.
 */
function conta(voci: [Chiave, number][]): [Chiave, number][] {
  return voci.filter(([, quanti]) => quanti > 0);
}

export function Profilo({
  onErrore,
  onNotizia,
  onImportato,
}: {
  onErrore: (e: unknown) => void;
  onNotizia: (testo: string) => void;
  /** Il profilo è stato applicato: quel che sta in `App` va riletto. */
  onImportato: () => void;
}) {
  /** Il piano da confermare, col percorso da cui è venuto. */
  const [daApplicare, setDaApplicare] = useState<{
    percorso: string;
    piano: PianoProfilo;
  } | null>(null);
  const [inVolo, setInVolo] = useState(false);
  /**
   * Un piano si sta rifacendo: i campi non si toccano, ma «Applica» sì.
   *
   * Separato da `inVolo` per una ragione precisa. Rifare il piano parte
   * dall'`onBlur` del campo di destinazione, cioè **mentre** si clicca su
   * «Applica»: fra il premere e il rilasciare il pulsante diventava `disabled`,
   * e un clic su un pulsante disabilitato non è un clic. Il primo si perdeva
   * sempre, e chi lo faceva imparava a premere due volte.
   *
   * «Applica» non ha bisogno del piano nuovo: il piano serve a mostrare i
   * numeri, mentre quel che si porta al nucleo sono le `rimappature`, che sono
   * già quelle. Si aspetta solo che il conto in volo torni — vedi `ripiana`.
   */
  const [ripianificando, setRipianificando] = useState(false);
  /** Il conto del piano in volo, se ce n'è uno: `applica` lo aspetta. */
  const pianoInVolo = useRef<Promise<void> | null>(null);
  /**
   * Quante volte il pannello del piano è stato chiuso.
   *
   * Un piano che torna dopo che si è applicato o rinunciato parla di una
   * conferma che non c'è più, e scriverlo rimetterebbe in piedi il pannello.
   */
  const giro = useRef(0);
  /** L'ultima esportazione, per dire cosa ha portato e quanto pesa. */
  const [uscito, setUscito] = useState<EsportazioneProfilo | null>(null);
  /** Le destinazioni scelte per le radici che di qua non esistono. */
  const [rimappature, setRimappature] = useState<RimappaturaRadice[]>([]);
  /** Si può ancora tornare indietro dall'ultima importazione. */
  const [annullabile, setAnnullabile] = useState(false);
  /**
   * Chi importa ha detto che la libreria del profilo è la sua, anche con
   * un'identità diversa. Vedi `Ambiente::unisci_comunque` nel nucleo: è la via
   * d'uscita di chi ha esportato dal computer nuovo prima di importare il
   * vecchio. Torna a «no» a ogni file scelto.
   */
  const [unisci, setUnisci] = useState(false);
  const [avanzamento, setAvanzamento] = useState<AvanzamentoProfilo | null>(
    null,
  );

  useAscolto<AvanzamentoProfilo>("profilo:avanzamento", setAvanzamento);

  const percentuale =
    avanzamento && avanzamento.totale > 0
      ? Math.round((avanzamento.fatti / avanzamento.totale) * 100)
      : 0;

  const esporta = async () => {
    try {
      const scelta = await save({
        defaultPath: "aether-profilo.aeprofile",
        filters: [{ name: t("profile.file"), extensions: ["aeprofile"] }],
      });
      if (typeof scelta !== "string") return;
      setInVolo(true);
      const esito = await ipc.profiloEsporta(scelta);
      setUscito(esito);
      onNotizia(t("profile.exported", { n: esito.voci, dove: scelta }));
    } catch (e) {
      onErrore(e);
    } finally {
      setInVolo(false);
      setAvanzamento(null);
    }
  };

  const leggi = async () => {
    try {
      const scelta = await open({
        multiple: false,
        // Tutte e due le estensioni: il `.json` della 2.2 si legge ancora, e un
        // filtro che lo nascondesse renderebbe irraggiungibile un file che il
        // programma sa aprire benissimo.
        filters: [
          { name: t("profile.file"), extensions: ["aeprofile", "json"] },
        ],
      });
      if (typeof scelta !== "string") return;
      setInVolo(true);
      setUnisci(false);
      const piano = await ipc.profiloPiano(scelta);
      setRimappature(piano.rimappature);
      setDaApplicare({ percorso: scelta, piano });
    } catch (e) {
      onErrore(e);
    } finally {
      setInVolo(false);
      setAvanzamento(null);
    }
  };

  /** Rifà il piano con le destinazioni scelte finora. */
  const ripiana = async (
    prossime: RimappaturaRadice[],
    unisciComunque: boolean = unisci,
  ) => {
    if (!daApplicare) return;
    const mio = giro.current;
    const conto = (async () => {
      try {
        setRipianificando(true);
        const piano = await ipc.profiloPiano(
          daApplicare.percorso,
          prossime,
          unisciComunque,
        );
        if (mio === giro.current) {
          setDaApplicare({ percorso: daApplicare.percorso, piano });
        }
      } catch (e) {
        onErrore(e);
      } finally {
        setRipianificando(false);
      }
    })();
    pianoInVolo.current = conto;
    await conto;
    if (pianoInVolo.current === conto) pianoInVolo.current = null;
  };

  const applica = async () => {
    if (!daApplicare) return;
    try {
      setInVolo(true);
      // Il piano che sta tornando riguarda le stesse `rimappature` che si sta
      // per portare al nucleo: non cambia niente di quel che si fa, e si
      // aspetta solo perché due comandi sulla stessa libreria non si
      // accavallino.
      await pianoInVolo.current;
      giro.current += 1;
      const fatto = await ipc.profiloImporta(
        daApplicare.percorso,
        rimappature,
        unisci,
      );
      setDaApplicare(null);
      setAnnullabile(true);
      onNotizia(
        fatto.cambi.length === 0
          ? t("profile.nothingChanged")
          : t("profile.applied", { n: fatto.cambi.length }),
      );
      onImportato();
    } catch (e) {
      onErrore(e);
    } finally {
      setInVolo(false);
      setAvanzamento(null);
    }
  };

  const annulla = async () => {
    try {
      setInVolo(true);
      await ipc.profiloAnnulla();
      setAnnullabile(false);
      onNotizia(t("profile.undone"));
      onImportato();
    } catch (e) {
      onErrore(e);
    } finally {
      setInVolo(false);
      setAvanzamento(null);
    }
  };

  const piano = daApplicare?.piano;
  const portati = piano
    ? conta([
        ["profile.brings.listens", piano.portati.ascolti],
        ["profile.brings.history", piano.portati.cronologia],
        ["profile.brings.ratings", piano.portati.voti],
        ["profile.brings.loved", piano.portati.preferiti],
        ["profile.brings.positions", piano.portati.posizioni],
        ["profile.brings.playlists", piano.portati.playlist],
        ["profile.brings.folders", piano.portati.cartelle],
        ["profile.brings.fixes", piano.portati.correzioni],
        ["profile.brings.lyrics", piano.portati.testi],
        ["profile.brings.wanted", piano.portati.desiderati],
        ["profile.brings.covers", piano.portati.fileCopertine],
      ])
    : [];

  return (
    <>
      <p className="nota">
        <Trans
          k="profile.note"
          v={{ non: <strong>{t("profile.note.not")}</strong> }}
        />
      </p>

      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={inVolo || ripianificando}
          onClick={() => void esporta()}
        >
          <Icona nome="i-import" dim={15} />
          {t("profile.export")}
        </button>
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={inVolo || ripianificando}
          onClick={() => void leggi()}
        >
          <Icona nome="i-import" dim={15} />
          {t("profile.read")}
        </button>
        {annullabile && (
          <button
            type="button"
            className="bottone btn-ghost"
            disabled={inVolo || ripianificando}
            onClick={() => void annulla()}
          >
            {t("profile.undo")}
          </button>
        )}
      </div>

      {annullabile && <p className="nota">{t("profile.undo.note")}</p>}

      {avanzamento && (
        <>
          <div className="avanzamento">
            <div style={{ width: `${percentuale}%` }} />
          </div>
          <div className="conteggio">
            {avanzamento.fatti} / {avanzamento.totale}
          </div>
        </>
      )}

      {uscito && (
        <div className="scheda-anteprima">
          <h3 className="titoletto">{t("profile.exported.title")}</h3>
          <ul className="riepilogo-profilo">
            {conta([
              ["profile.brings.settings", uscito.voci],
              ["profile.brings.history", uscito.cronologia],
              ["profile.brings.covers", uscito.copertine],
              ["profile.brings.skins", uscito.skin],
              ["profile.brings.drafts", uscito.bozze],
            ]).map(([cosa, quanti]) => (
              <li className="voce-riepilogo" key={cosa}>
                {t(cosa, { n: quanti })}
              </li>
            ))}
          </ul>
          <p className="nota">
            {t("profile.weighs", { peso: inMegabyte(uscito.byte) })}
          </p>
          {uscito.pesante && (
            <p className="nota avviso-profilo">{t("profile.heavy")}</p>
          )}
          {uscito.lasciate.length > 0 && (
            <p className="nota">
              <Trans
                k="profile.leftHere"
                n={{ chiavi: uscito.lasciate.map(breve).join(", ") }}
                v={{
                  questo: <em>{t("profile.leftHere.this")}</em>,
                  questa: <em>{t("profile.leftHere.thisLib")}</em>,
                }}
              />
            </p>
          )}
        </div>
      )}

      {daApplicare && piano && (
        <div className="scheda-anteprima">
          <h3 className="titoletto">
            {t("profile.writtenOn", {
              quando: quandoScritto(piano.creatoMs),
            })}
          </h3>

          {piano.identitaDiversa && (
            <>
              <p className="nota avviso-profilo">
                {unisci
                  ? t("profile.otherLibrary.merging")
                  : t("profile.otherLibrary")}
              </p>
              {/* Il piano si rifà con la scelta, così i numeri qui sotto sono
                  quelli di quel che succederà davvero. */}
              <label className="unisci-comunque">
                <input
                  type="checkbox"
                  checked={unisci}
                  disabled={inVolo || ripianificando}
                  onChange={(e) => {
                    const scelta = e.target.checked;
                    setUnisci(scelta);
                    void ripiana(rimappature, scelta);
                  }}
                />
                <span>{t("profile.mergeAnyway")}</span>
              </label>
            </>
          )}

          {piano.rimappature.length > 0 && (
            <>
              <p className="nota">{t("profile.remap.note")}</p>
              <ul className="rimappature">
                {piano.rimappature.map((riga) => {
                  const scelta =
                    rimappature.find((r) => r.da === riga.da)?.a ?? "";
                  return (
                    <li className="rimappatura" key={riga.da}>
                      <code className="rimappatura-da">{breve(riga.da)}</code>
                      <span className="rimappatura-brani">
                        {t("profile.remap.tracks", { n: riga.brani })}
                      </span>
                      <input
                        type="text"
                        className="campo field-input"
                        value={scelta}
                        placeholder={t("profile.remap.placeholder")}
                        aria-label={t("profile.remap.aria", { da: riga.da })}
                        onChange={(e) =>
                          setRimappature((prima) =>
                            prima.map((r) =>
                              r.da === riga.da
                                ? { ...r, a: e.target.value }
                                : r,
                            ),
                          )
                        }
                        onBlur={() => void ripiana(rimappature)}
                      />
                    </li>
                  );
                })}
              </ul>
            </>
          )}

          {piano.cambi.length === 0 ? (
            <p className="niente empty-state">{t("profile.noChanges")}</p>
          ) : (
            <ul className="cartelle">
              {piano.cambi.map((c) => (
                <li className="cartella" key={c.chiave}>
                  <span
                    className="percorso"
                    title={`${c.prima ?? t("profile.absent")} → ${c.dopo}`}
                  >
                    <code>{c.chiave}</code>:{" "}
                    {c.prima === null ? "—" : breve(c.prima)} → {breve(c.dopo)}
                  </span>
                </li>
              ))}
            </ul>
          )}

          {portati.length > 0 && (
            <ul className="riepilogo-profilo">
              {portati.map(([cosa, quanti]) => (
                <li className="voce-riepilogo" key={cosa}>
                  {t(cosa, { n: quanti })}
                </li>
              ))}
            </ul>
          )}

          {piano.percorsiRiscritti > 0 && (
            <p className="nota">
              {t("profile.remap.rewritten", { n: piano.percorsiRiscritti })}
            </p>
          )}
          {piano.braniIrrintracciabili > 0 && (
            <p className="nota">
              {t("profile.remap.missing", { n: piano.braniIrrintracciabili })}
            </p>
          )}
          {piano.braniGiaPresenti > 0 && (
            <p className="nota">
              {t("profile.remap.already", { n: piano.braniGiaPresenti })}
            </p>
          )}

          {piano.invariate > 0 && (
            <p className="nota">{t("profile.same", { n: piano.invariate })}</p>
          )}

          {piano.percorsiMancanti.length > 0 && (
            <p className="nota">
              <Trans
                k="profile.missingPaths"
                n={{
                  percorsi: piano.percorsiMancanti.map(breve).join(", "),
                }}
                v={{
                  titolo: <strong>{t("profile.missingPaths.title")}</strong>,
                }}
              />
            </p>
          )}

          {piano.sconosciute.length > 0 && (
            <p className="nota">
              {t("profile.unknownKeys", {
                chiavi: piano.sconosciute.map(breve).join(", "),
              })}
            </p>
          )}

          <p className="nota">{t("profile.whenApplied")}</p>
          <p className="nota">{t("profile.additive")}</p>

          <div className="azioni">
            <button
              type="button"
              className="bottone primario btn-accent"
              disabled={inVolo}
              onClick={() => void applica()}
            >
              {inVolo ? t("profile.applying") : t("profile.apply")}
            </button>
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo || ripianificando}
              onClick={() => {
                giro.current += 1;
                setDaApplicare(null);
              }}
            >
              {t("profile.leaveIt")}
            </button>
          </div>
        </div>
      )}
    </>
  );
}
