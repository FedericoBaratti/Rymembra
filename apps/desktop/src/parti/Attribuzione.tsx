/**
 * Chi ha pubblicato questo brano, sotto che licenza, e dove sta la sua pagina.
 *
 * # Perché è un componente e non tre righe dentro «In riproduzione»
 *
 * Perché il posto dove la si mostra è destinato a essere più di uno — la
 * schermata grande, e un domani il pannello laterale — e perché la regola che
 * decide **se** mostrarla è una sola e deve restare una sola: si mostra quando
 * il brano viene da un catalogo, cioè quando `fonte` non è nulla.
 *
 * # Perché si mostra, e non è una gentilezza
 *
 * Perché per certe Creative Commons e per i termini di Audius **il rimando
 * visibile è una condizione d'uso**. Il preambolo di `Esplora.tsx` lo dice per
 * i risultati della ricerca, e per un pezzo di tempo è stato vero solo lì: le
 * colonne `fonte_pagina` e `licenza` arrivavano dal database fino all'IPC e
 * nessuno le disegnava, quindi un brano *tenuto* in libreria — quello che
 * resta, quello che si riascolta — non portava nessuna attribuzione da nessuna
 * parte. Il README e il CHANGELOG promettevano il contrario.
 *
 * # Perché la pagina si apre per identificativo
 *
 * Perché `ipc.branoApriPagina` prende un `id` e non un indirizzo: l'indirizzo
 * lo legge il nucleo da `tracks.fonte_pagina` e lo ripassa dall'allowlist dei
 * cataloghi prima di darlo al browser. Vedi `comandi::brano_apri_pagina`.
 *
 * # Il degrado
 *
 * Le tre colonne sono indipendenti e una può mancare: un catalogo che non
 * dichiara la licenza lascia `licenza` nulla, uno che non ha una pagina
 * pubblica lascia `fonte_pagina` nulla. Si uniscono i pezzi **presenti** e non
 * si scrive mai un « · · », che è la stessa regola di `parti/Formato.tsx`.
 */
import { ipc, type Brano } from "../ipc";
import { Icona } from "./Icone";
import { nomeFonte, nomeLicenza } from "../formato";
import { t } from "../lingue";

export function Attribuzione({
  brano,
  onErrore,
}: {
  brano: Brano;
  onErrore: (e: unknown) => void;
}) {
  // Un file sul disco non ha niente da attribuire: l'ha messo lì chi ascolta.
  if (brano.fonte === null) return null;

  const pezzi = [nomeFonte(brano.fonte)];
  if (brano.licenza !== null) pezzi.push(nomeLicenza(brano.licenza));

  return (
    <div className="attribuzione-brano">
      <span className="chi-pubblica">{pezzi.join(" · ")}</span>
      {brano.fontePagina !== null && (
        <button
          type="button"
          className="bottone minuto btn-ghost"
          onClick={() => {
            ipc.branoApriPagina(brano.id).catch(onErrore);
          }}
        >
          <Icona nome="i-external" dim={13} />
          {t("track.openPage")}
        </button>
      )}
    </div>
  );
}
