/**
 * Il pacchetto e le sue istantanee.
 *
 * # L'albero dice quel che l'esportazione scriverebbe
 *
 * Non è l'elenco di una cartella: è l'elenco delle voci che il **formato**
 * ammette — `skin.json`, `preview.png`, `assets/` a un livello solo — e viene
 * dallo stesso posto da cui `studio_esporta` prende le risorse. Un albero che
 * mostrasse file che l'esportazione butta via sarebbe peggio di nessun albero.
 *
 * # Le istantanee non si battezzano
 *
 * Si prendono, prima di ogni prova, e portano il motivo per cui sono state
 * prese: «salvata», «esportata», «manuale». Inventare un nome a ogni prova è un
 * lavoro che nessuno fa, e un elenco di nomi non scelti è un elenco che non si
 * legge. Il documento resta testo, quindi per la cronologia seria c'è già git —
 * questo serve a tornare indietro di dieci minuti senza uscire dallo Studio.
 */
import type { Istantanea, VoceFile } from "../ipc";
import { Icona } from "../parti/Icone";

/** Quanto tempo fa, in parole corte. */
function quandoFa(quando: number, adesso: number): string {
  const secondi = Math.max(0, Math.round((adesso - quando) / 1000));
  if (secondi < 60) return "adesso";
  const minuti = Math.round(secondi / 60);
  if (minuti < 60) return `${minuti} min fa`;
  const ore = Math.round(minuti / 60);
  if (ore < 24) return `${ore} h fa`;
  return `${Math.round(ore / 24)} g fa`;
}

/** Quanto pesa, in unità che si leggono. */
function quantoPesa(byte: number): string {
  if (byte < 1024) return `${byte} B`;
  if (byte < 1024 * 1024) return `${Math.round(byte / 1024)} kB`;
  return `${(byte / (1024 * 1024)).toFixed(1)} MB`;
}

/** Come si chiama una causa, per chi legge. */
const COME_SI_LEGGE: Record<Istantanea["causa"], string> = {
  derivata: "Derivata",
  salvata: "Salvata e usata",
  esportata: "Esportata",
  manuale: "Presa a mano",
};

export function Pacchetto({
  id,
  voci,
  istantanee,
  parti,
  token,
  sporca,
  onIstantanea,
  onRipristina,
}: {
  id: string;
  voci: readonly VoceFile[];
  istantanee: readonly Istantanea[];
  /** Quante parti e token ha il documento **adesso**, non su disco. */
  parti: number;
  token: number;
  /** Il buffer non combacia col disco. */
  sporca: boolean;
  onIstantanea: () => void;
  onRipristina: (quando: number) => void;
}) {
  const adesso = Date.now();
  const risorse = voci.filter((v) => v.genere === "risorsa");

  return (
    <aside className="colonna-pacchetto">
      <div className="blocco-pacchetto">
        <div className="titolino">{id}.aeskin</div>
        <div className="albero-file">
          {voci
            .filter((v) => v.genere !== "risorsa")
            .map((voce) => (
              <div
                key={voce.nome}
                className="file"
                data-active={voce.genere === "manifest" || undefined}
              >
                <Icona nome={voce.genere === "manifest" ? "i-text" : "i-album"} dim={13} />
                <code className="nome">{voce.nome}</code>
                <span className="peso-file">{quantoPesa(voce.byte)}</span>
              </div>
            ))}
          {risorse.length > 0 && (
            <>
              <div className="file">
                <Icona nome="i-folder" dim={13} />
                <code className="nome">assets/</code>
                <span className="peso-file">{risorse.length}</span>
              </div>
              {risorse.map((voce) => (
                <div key={voce.nome} className="file dentro">
                  <code className="nome">{voce.nome.replace("assets/", "")}</code>
                  <span className="peso-file">{quantoPesa(voce.byte)}</span>
                </div>
              ))}
            </>
          )}
        </div>
      </div>

      <div className="blocco-pacchetto">
        <div className="testa-istantanee">
          <span className="titolino">Istantanee</span>
          <button
            type="button"
            className="prendi icon-btn"
            aria-label="Prendi un'istantanea adesso"
            title="Prendi un'istantanea adesso"
            onClick={onIstantanea}
          >
            <Icona nome="i-plus" dim={13} />
          </button>
        </div>
        <div className="elenco-istantanee">
          {/* La prima riga è sempre adesso: è l'unica che non è su disco, e
              dirlo è il modo di non far credere che sia già salvata. */}
          <div className="istantanea adesso">
            <div className="quale">Ora · {sporca ? "non salvata" : "come su disco"}</div>
            <div className="quanto">
              {parti} parti · {token} token
            </div>
          </div>
          {istantanee.map((presa) => (
            <button
              key={presa.quando}
              type="button"
              className="istantanea"
              onClick={() => onRipristina(presa.quando)}
            >
              <div className="quale">{COME_SI_LEGGE[presa.causa]}</div>
              <div className="quanto">
                {presa.parti} parti · {presa.token} token ·{" "}
                {quandoFa(presa.quando, adesso)}
              </div>
            </button>
          ))}
        </div>
      </div>

      <div className="spinta" />

      <p className="nota-pacchetto">
        Il pacchetto è uno zip con un rapporto di compressione massimo di 200:1 —{" "}
        <code>package.rs</code> rifiuta le bombe.
      </p>
    </aside>
  );
}
