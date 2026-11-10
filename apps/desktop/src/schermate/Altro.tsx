/**
 * Altro: la quarta scheda del telefono.
 *
 * Quel che sul computer sta ai margini della finestra — «Ascolta insieme» nella
 * barra del lettore, le Impostazioni e le donazioni in fondo alla barra
 * laterale, le importazioni dietro un toast — sul telefono non ha un margine in
 * cui stare. Prima stava in un cassetto che scorreva da sinistra: una seconda
 * navigazione, nascosta dietro la prima, che si apriva sopra la pagina e la
 * copriva. Adesso è una pagina, con le sue righe raggruppate per cosa fanno, e
 * Indietro ne esce come da ogni altra scheda.
 *
 * Le righe sono grandi — cinquantasei pixel, un'icona su una tessera, una riga
 * che dice a cosa serve — perché qui si viene di rado, e chi ci viene deve
 * riconoscere la porta senza doverla aprire.
 */
import type { ReactNode } from "react";

import { useArchivio } from "../archivio";
import { t } from "../lingue";
import { archivioLontano, eTelefono, ordinaDispositivi } from "../lontano";
import { sottotitoloDispositivo } from "./Dispositivo";
import { Icona, type NomeIcona } from "../parti/Icone";
import { ConImportazioni } from "../parti/Importazioni";

function Riga({
  icona,
  nome,
  sotto,
  esce = false,
  onClick,
}: {
  icona: NomeIcona;
  nome: string;
  sotto?: ReactNode;
  /** Porta fuori dall'app (il browser): la freccia lo dice al posto del `>`. */
  esce?: boolean | undefined;
  onClick: () => void;
}) {
  return (
    <button type="button" className="riga-altro" onClick={onClick}>
      <span className="tessera-altro" aria-hidden="true">
        <Icona nome={icona} dim={20} />
      </span>
      <span className="testi-altro">
        <span className="nome-altro">{nome}</span>
        {sotto !== undefined && <span className="sotto-altro">{sotto}</span>}
      </span>
      <span className="freccia-altro" aria-hidden="true">
        <Icona nome={esce ? "i-external" : "i-chev-r"} dim={18} />
      </span>
    </button>
  );
}

function Gruppo({ titolo, children }: { titolo: string; children: ReactNode }) {
  return (
    <section className="gruppo-altro" aria-label={titolo}>
      <h2 className="titolo-gruppo-altro">{titolo}</h2>
      <div className="righe-altro">{children}</div>
    </section>
  );
}

export function Altro({
  onInsieme,
  onImportazioni,
  onIncollaLink,
  onCartelle,
  onImpostazioni,
  onDona,
  onDispositivo,
  onCollega,
}: {
  onInsieme: () => void;
  onImportazioni: () => void;
  onIncollaLink: () => void;
  onCartelle: () => void;
  onImpostazioni: () => void;
  onDona: () => void;
  /** Apre «Sfoglia» di un dispositivo collegato. */
  onDispositivo: (chiave: string) => void;
  /** Porta alla scheda «Dispositivi» delle Impostazioni, dove si collega. */
  onCollega: () => void;
}) {
  // I collegati che non ci hanno dimenticati, anche quelli che adesso non
  // rispondono: la pagina dice da quando, e il telefono è proprio il posto
  // da cui si guarda il PC rimasto a casa.
  const stato = useArchivio(archivioLontano);
  const dispositivi = ordinaDispositivi(stato?.dispositivi ?? []).filter((d) => !d.revocato);
  return (
    <div className="altro">
      <Gruppo titolo={t("more.group.listen")}>
        <Riga
          icona="i-insieme"
          nome={t("together.title")}
          sotto={t("more.together.sub")}
          onClick={onInsieme}
        />
      </Gruppo>
      <Gruppo titolo={t("more.group.devices")}>
        {dispositivi.map((d) => (
          <Riga
            key={d.chiave}
            icona={eTelefono(d.piattaforma) ? "i-telefono" : "i-computer"}
            nome={d.nome}
            sotto={sottotitoloDispositivo(d)}
            onClick={() => onDispositivo(d.chiave)}
          />
        ))}
        <Riga
          icona="i-qr"
          nome={t("more.devices.link")}
          sotto={t("more.devices.link.sub")}
          onClick={onCollega}
        />
      </Gruppo>
      <Gruppo titolo={t("more.group.library")}>
        <ConImportazioni>
          {(importazioni) => (
            <Riga
              icona="i-import"
              nome={t("more.imports")}
              sotto={
                importazioni.brano !== null
                  ? t("more.imports.running", { titolo: importazioni.brano.titolo })
                  : importazioni.elenco.length > 0
                    ? t("more.imports.count", { n: importazioni.elenco.length })
                    : t("more.imports.sub")
              }
              onClick={onImportazioni}
            />
          )}
        </ConImportazioni>
        <Riga
          icona="i-external"
          nome={t("imports.pasteLink")}
          sotto={t("more.link.sub")}
          onClick={onIncollaLink}
        />
        <Riga
          icona="i-folder"
          nome={t("more.folders")}
          sotto={t("more.folders.sub")}
          onClick={onCartelle}
        />
      </Gruppo>
      <Gruppo titolo={t("more.group.app")}>
        <Riga
          icona="i-settings"
          nome={t("nav.settings")}
          sotto={t("more.settings.sub")}
          onClick={onImpostazioni}
        />
        <Riga
          icona="i-donate"
          nome={t("nav.donate")}
          sotto={t("more.donate.sub")}
          esce
          onClick={onDona}
        />
      </Gruppo>
    </div>
  );
}
