/**
 * Chiaro, scuro, o quel che dice il sistema.
 *
 * # Temi simmetrici
 *
 * Una skin nasce scura o chiara (`scheme`), e può avere la **variante** del
 * tema opposto. Il compilatore scrive la base sotto `:root[data-skin='<id>']`
 * e la variante sotto `:root[data-skin='<id>'][data-theme='<opposto>']`, quindi
 * tutto quel che serve è che qualcuno scriva quell'attributo — e questo file
 * non fa altro.
 *
 * `data-theme` dice sempre **il tema che si vede**, anche quando coincide con
 * la base: una skin chiara senza variante resta chiara con la preferenza sullo
 * scuro, e l'attributo scrive `light`. Così chi legge l'attributo — il blocco
 * chiaro di ripiego in `stile.css`, lo spettro che ricalcola i suoi colori —
 * legge la verità e non la preferenza.
 *
 * Dove la scelta si **ricorda** è invece cambiato: stava in `localStorage`,
 * adesso sta in `ui.theme`, nel database. La ragione sta scritta per esteso in
 * `core/rymembra-app/src/preferenze.rs`, e in breve è che nel frattempo sono nati
 * due lettori di tutte le preferenze — il backup su Drive e il profilo — e una
 * preferenza in `localStorage` non finisce in nessuno dei due.
 *
 * Quel che resta qui è il **ripiego della prima apertura**: chi aveva già
 * scelto un tema non deve ritrovarselo azzerato dall'aggiornamento. Si legge
 * una volta, si riscrive nel database, e la chiave vecchia si toglie — così il
 * ripiego non può risorgere e sovrascrivere una scelta fatta dopo.
 *
 * # Perché «sistema» non è il difetto silenzioso
 *
 * Seguire il sistema è la scelta giusta per la maggior parte delle persone e
 * **non** è quella che si prende senza dirlo: il segmentato mostra tutte e tre
 * le voci con «Sistema» selezionata, così chi vuole fissare il tema vede subito
 * che può, e chi non ci pensa ottiene comunque il comportamento sensato.
 *
 * # Cosa questo non risolve
 *
 * Il fotogramma che il sistema operativo dipinge **prima** della pagina resta
 * quello di `backgroundColor` in `tauri.conf.json`, cioè `#09090d`. Chi tiene il
 * tema chiaro vede un lampo scuro a ogni avvio. Non è risolvibile da qui: quel
 * colore va letto prima che esista un documento, quindi prima che esista
 * JavaScript. La nota sta in `apps/desktop/src-tauri/src/skin.rs`.
 */

import type { Schema } from "./ipc";

/** Le tre scelte. */
export type Tema = "scuro" | "chiaro" | "sistema";

/** Quel che di una skin serve a decidere il tema: la base, e se ha l'opposto. */
export interface TemiSkin {
  scheme: Schema;
  variante: boolean;
}

/** La chiave in cui la scelta viveva prima che tornasse nel nucleo. */
const CHIAVE = "rymembra.tema";

/**
 * La scelta rimasta da prima dell'aggiornamento, se c'è.
 *
 * `null` quando non c'è niente da recuperare — che è il caso di chiunque apra
 * Rymembra per la prima volta, e di chiunque abbia già fatto questo passaggio.
 */
export function temaDiRipiego(): Tema | null {
  const letto = window.localStorage.getItem(CHIAVE);
  return letto === "scuro" || letto === "chiaro" || letto === "sistema" ? letto : null;
}

/**
 * Toglie la chiave vecchia.
 *
 * Da chiamare **dopo** che il valore è arrivato nel database, mai prima: se la
 * scrittura fallisce, il ripiego deve restare lì per il tentativo successivo.
 */
export function dimenticaRipiego(): void {
  window.localStorage.removeItem(CHIAVE);
}

/**
 * Il tema che si vede, data la scelta e la skin.
 *
 * Quello scelto se la skin lo sa mostrare — perché è la sua base o perché ha
 * la variante — e altrimenti la sua base. Prima che la skin arrivi vale la
 * base di serie, scura: la finestra non si mostra finché skin e tema non sono
 * sul documento, quindi quel valore non si vede mai.
 */
export function temaMostrato(tema: Tema, skin: TemiSkin | null): Schema {
  const vuoleChiaro =
    tema === "chiaro" ||
    (tema === "sistema" &&
      window.matchMedia("(prefers-color-scheme: light)").matches);
  const voluto: Schema = vuoleChiaro ? "light" : "dark";
  const base = skin?.scheme ?? "dark";
  return voluto === base || skin?.variante === true ? voluto : base;
}

/**
 * Applica un tema. Non lo ricorda: quello lo fa `ipc.impostaTema`.
 *
 * Una skin che non sa mostrare il tema scelto resta nella sua base: scrivere
 * l'altro senza un blocco che lo raccolga non cambierebbe i colori, e
 * l'interfaccia mostrerebbe una scelta fatta e non avvenuta.
 *
 * Restituisce se si sta **davvero** mostrando il chiaro, che non è sempre
 * quel che si è scelto. Serve a chi deve calcolare qualcosa contro le
 * superfici del tema in uso, come l'accento che segue la copertina: chiederlo
 * alla preferenza invece che al risultato darebbe la risposta sbagliata
 * proprio nel caso in cui i due divergono.
 */
export function applicaTema(tema: Tema, skin: TemiSkin | null): boolean {
  const mostrato = temaMostrato(tema, skin);
  document.documentElement.dataset.theme = mostrato;
  return mostrato === "light";
}

/**
 * Segue il sistema finché la scelta è «sistema».
 *
 * Restituisce la funzione che smette di seguirlo. Il listener si registra
 * sempre e non solo quando serve: attaccarlo e staccarlo a ogni cambio di
 * preferenza vorrebbe dire tenere conto di quale versione è attiva, e
 * `matchMedia` non costa niente finché nessuno cambia tema.
 *
 * `avvisa` riceve il tema che è rimasto. Senza, un cambio deciso dal sistema
 * operativo mentre la scelta è «sistema» sarebbe l'unico modo di cambiare tema
 * che non passa da React, e chi deve ricalcolare qualcosa su quel tema non lo
 * saprebbe.
 */
export function seguiIlSistema(
  quando: () => { tema: Tema; skin: TemiSkin | null },
  avvisa?: (chiaro: boolean) => void,
): () => void {
  const media = window.matchMedia("(prefers-color-scheme: light)");
  const reagisci = () => {
    const { tema, skin } = quando();
    if (tema === "sistema") avvisa?.(applicaTema(tema, skin));
  };
  media.addEventListener("change", reagisci);
  return () => media.removeEventListener("change", reagisci);
}
