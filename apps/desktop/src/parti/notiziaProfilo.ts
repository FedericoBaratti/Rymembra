/**
 * Quel che un profilo porta alla libreria, e la notizia di un profilo applicato.
 *
 * # Perché un modulo a parte
 *
 * La notizia dopo «Applica» contava soltanto le impostazioni: con le stesse
 * impostazioni dei due computer diceva «Il profilo non ha cambiato niente»
 * dopo aver scritto trecento ascolti nella cronologia e rifatto quattro
 * playlist, e con impostazioni diverse taceva il resto (R13c-5). La notizia si
 * compone qui, da `cambi` **e** da `portati`, come `notiziaDelRipristino` fa
 * per il ripristino: una funzione pura, che una prova può guardare senza un
 * DOM. Il riepilogo del piano usa le stesse voci, così piano e notizia non
 * possono contare due cose diverse.
 */
import type { Chiave } from "../lingue";
import type { PortatiProfilo } from "../ipc";

/**
 * Le voci del riepilogo della libreria che hanno un numero diverso da zero.
 *
 * Solo quelle: un elenco di dodici righe di cui dieci dicono «0» è un elenco
 * che non si legge, e la riga che conta ci si perde dentro.
 *
 * Si tiene la **chiave** e non la frase già tradotta, perché il numero deve
 * entrare dentro `t()`: è l'unico modo perché «1 ascolto» e «2 ascolti» siano
 * due frasi e non un numero appiccicato davanti a un plurale.
 */
export function vociPortate(portati: PortatiProfilo): [Chiave, number][] {
  const voci: [Chiave, number][] = [
    ["profile.brings.listens", portati.ascolti],
    ["profile.brings.history", portati.cronologia],
    ["profile.brings.ratings", portati.voti],
    ["profile.brings.loved", portati.preferiti],
    ["profile.brings.positions", portati.posizioni],
    ["profile.brings.playlists", portati.playlist],
    ["profile.brings.folders", portati.cartelle],
    ["profile.brings.fixes", portati.correzioni],
    ["profile.brings.lyrics", portati.testi],
    ["profile.brings.wanted", portati.desiderati],
    ["profile.brings.covers", portati.fileCopertine],
  ];
  return voci.filter(([, quanti]) => quanti > 0);
}

/** Un pezzo della notizia: una chiave del catalogo, con quel che le serve. */
export type PezzoNotizia =
  | { chiave: "profile.applied"; n: number }
  | { chiave: "profile.applied.library"; voci: [Chiave, number][] }
  | { chiave: "profile.skinNotWorn"; nome: string }
  | { chiave: "profile.nothingChanged" };

/**
 * La notizia di un profilo applicato: le impostazioni cambiate, poi quel che
 * è arrivato nella libreria. «Non ha cambiato niente» solo quando non è
 * cambiato davvero niente, né di qua né di là.
 *
 * E la skin che il profilo avrebbe indossato e che non ha passato l'esame:
 * è installata, ma non indossata, e va detto (decisione 21).
 */
export function notiziaDelProfilo(fatto: {
  cambi: readonly unknown[];
  portati: PortatiProfilo;
  skinNonIndossata?: string | null;
}): PezzoNotizia[] {
  const pezzi: PezzoNotizia[] = [];
  if (fatto.cambi.length > 0) {
    pezzi.push({ chiave: "profile.applied", n: fatto.cambi.length });
  }
  const voci = vociPortate(fatto.portati);
  if (voci.length > 0) pezzi.push({ chiave: "profile.applied.library", voci });
  if (fatto.skinNonIndossata) {
    pezzi.push({ chiave: "profile.skinNotWorn", nome: fatto.skinNonIndossata });
  }
  if (pezzi.length === 0) pezzi.push({ chiave: "profile.nothingChanged" });
  return pezzi;
}
