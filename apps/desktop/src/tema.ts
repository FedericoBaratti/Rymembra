/**
 * Chiaro, scuro, o quel che dice il sistema.
 *
 * # Perché adesso un comando serve
 *
 * Il compilatore delle skin emette già la variante chiara sotto
 * `:root[data-skin='<id>'][data-theme='light']` — è in `compile.rs`, alla riga
 * che costruisce il selettore. Tutto quel che serve è che qualcuno scriva
 * quell'attributo, e questo file non fa altro.
 *
 * Dove la scelta si **ricorda** è invece cambiato: stava in `localStorage`,
 * adesso sta in `ui.theme`, nel database. La ragione sta scritta per esteso in
 * `core/aether-app/src/preferenze.rs`, e in breve è che nel frattempo sono nati
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

/** Le tre scelte. */
export type Tema = "scuro" | "chiaro" | "sistema";

/** La chiave in cui la scelta viveva prima che tornasse nel nucleo. */
const CHIAVE = "aether.tema";

/**
 * La scelta rimasta da prima dell'aggiornamento, se c'è.
 *
 * `null` quando non c'è niente da recuperare — che è il caso di chiunque apra
 * Aether per la prima volta, e di chiunque abbia già fatto questo passaggio.
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
 * Applica un tema. Non lo ricorda: quello lo fa `ipc.impostaTema`.
 *
 * `chiara` dice se la skin attiva ha davvero una variante chiara. Quando non ce
 * l'ha si resta sullo scuro anche se la preferenza dice altro: mettere
 * `data-theme='light'` senza un blocco che lo raccolga non cambierebbe niente,
 * e l'interfaccia mostrerebbe una scelta fatta e non avvenuta.
 *
 * Restituisce il tema che si sta **davvero** mostrando, che non è sempre quello
 * scelto — vedi il capoverso qui sopra. Serve a chi deve calcolare qualcosa
 * contro le superfici del tema in uso, come l'accento che segue la copertina:
 * chiederlo alla preferenza invece che al risultato darebbe la risposta
 * sbagliata proprio nel caso in cui i due divergono.
 */
export function applicaTema(tema: Tema, chiara: boolean): boolean {
  const vuoleChiaro =
    tema === "chiaro" ||
    (tema === "sistema" &&
      window.matchMedia("(prefers-color-scheme: light)").matches);

  const radice = document.documentElement;
  if (vuoleChiaro && chiara) {
    radice.dataset.theme = "light";
    return true;
  }
  delete radice.dataset.theme;
  return false;
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
  quando: () => { tema: Tema; chiara: boolean },
  avvisa?: (chiaro: boolean) => void,
): () => void {
  const media = window.matchMedia("(prefers-color-scheme: light)");
  const reagisci = () => {
    const { tema, chiara } = quando();
    if (tema === "sistema") avvisa?.(applicaTema(tema, chiara));
  };
  media.addEventListener("change", reagisci);
  return () => media.removeEventListener("change", reagisci);
}
