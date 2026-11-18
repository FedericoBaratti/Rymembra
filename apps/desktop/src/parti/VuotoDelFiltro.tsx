/**
 * Un filtro per dispositivo che non ha niente da mostrare, e perché.
 *
 * Tre motivi diversi che a occhio sono lo stesso elenco vuoto: il dispositivo
 * non ha ancora mandato il catalogo (lo sta leggendo, o è spento), è di una
 * versione di Rymembra che non lo sa mandare («aggiorna Rymembra su <nome> per
 * vedere la sua musica»), o semplicemente non ha niente da condividere. Una
 * frase per ciascuno, al posto di un «nessun brano» che direbbe tutti e tre.
 */
import { useArchivio } from "../archivio";
import { numero } from "../formato";
import { ipc, type OrigineLibreria } from "../ipc";
import { t } from "../lingue";
import { archivioLontano, archivioSpecchio } from "../lontano";
import { dispositiviDiLibreria } from "../origine";
import { StatoVuoto } from "../schermate/Libreria";

export function VuotoDelFiltro({
  origine,
  onErrore,
}: {
  origine: OrigineLibreria;
  onErrore: (errore: unknown) => void;
}) {
  const avanzamenti = useArchivio(archivioSpecchio);
  const stato = useArchivio(archivioLontano);
  if (origine.tipo !== "dispositivo") {
    return (
      <StatoVuoto
        icona="i-computer"
        titolo={t("library.origin.empty.title")}
        testo={t("library.origin.empty.local")}
      />
    );
  }
  const dispositivo = dispositiviDiLibreria(stato).find((d) => d.chiave === origine.chiave);
  const nome = dispositivo?.nome ?? "";
  const icona = dispositivo?.telefono === true ? "i-telefono" : "i-computer";
  const avanzamento = avanzamenti[origine.chiave];
  if (avanzamento?.fase === "nonSupportato") {
    return (
      <StatoVuoto
        icona={icona}
        titolo={t("devices.mirror.unsupported", { nome })}
        testo={t("devices.mirror.unsupported.sub")}
      />
    );
  }
  if (avanzamento?.fase === "leggo") {
    return (
      <StatoVuoto
        icona={icona}
        titolo={t("devices.mirror.reading", {
          nome,
          fatti: numero(avanzamento.fatti),
          totali: numero(avanzamento.totali),
        })}
        testo={t("devices.mirror.reading.sub")}
      />
    );
  }
  if (dispositivo !== undefined && !dispositivo.online) {
    return (
      <StatoVuoto
        icona={icona}
        titolo={t("library.origin.offline", { nome })}
        testo={t("library.origin.offline.sub")}
        azione={{
          etichetta: t("devices.mirror.retryNow"),
          fai: () => {
            ipc.lontanoCercaOra().catch(onErrore);
          },
        }}
      />
    );
  }
  return (
    <StatoVuoto
      icona={icona}
      titolo={t("library.origin.empty.title")}
      testo={t("library.origin.empty.device", { nome })}
      azione={{
        etichetta: t("library.origin.reread"),
        fai: () => {
          ipc.lontanoSpecchioOra(origine.chiave).catch(onErrore);
        },
      }}
    />
  );
}
