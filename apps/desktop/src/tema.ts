/**
 * Chiaro, scuro, o quel che dice il sistema.
 *
 * # Perché non serve nessun comando nuovo
 *
 * Il compilatore delle skin emette già la variante chiara sotto
 * `:root[data-skin='<id>'][data-theme='light']` — è in `compile.rs`, alla riga
 * che costruisce il selettore. Tutto quel che manca è che qualcuno scriva
 * quell'attributo. Non c'è niente da decidere nel nucleo: la scelta è una
 * preferenza della finestra, non un dato della libreria.
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

/** La chiave con cui la scelta sopravvive alla chiusura della finestra. */
const CHIAVE = "aether.tema";

/** La scelta salvata, o «sistema» se non ce n'è una. */
export function temaSalvato(): Tema {
  const letto = window.localStorage.getItem(CHIAVE);
  return letto === "scuro" || letto === "chiaro" ? letto : "sistema";
}

/**
 * Applica un tema, e ricorda la scelta.
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
  window.localStorage.setItem(CHIAVE, tema);
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
