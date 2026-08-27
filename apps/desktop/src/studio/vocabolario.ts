/**
 * Le descrizioni del registro, nella lingua attiva.
 *
 * # Perché non arrivano già tradotte
 *
 * Perché il registro non è interfaccia: è il **vocabolario** di quel che una
 * skin può ridisegnare, e vive in `aether-skin` — cinquantuno parti,
 * cinquantotto token, quattordici widget, le loro manopole e i parametri degli
 * effetti. Ogni voce porta una frase che dice a cosa serve, e quelle frasi si
 * leggono nello Studio: nell'albero, nell'ispettore, sulle manopole di un
 * livello. Sono interfaccia a tutti gli effetti.
 *
 * Farle attraversare l'IPC già tradotte vorrebbe dire portare la lingua
 * dell'interfaccia dentro un crate di dominio, che di lingue non sa niente e
 * non deve saperne: `parts.rs` è un elenco di dati, e la sua frase italiana è
 * insieme la documentazione della voce — il commento che spiega cosa sia
 * `progress-sheen` sta lì e non altrove.
 *
 * Quindi il nucleo continua a mandare la sua frase, e qui si guarda se il
 * catalogo delle lingue ne ha una per quel nome. La chiave la compone il nome,
 * che è stabile e attraversa già il confine; la frase del nucleo è il ripiego.
 * È la stessa forma con cui `testoErrore` legge `i18nKey` e ripiega su
 * `message`, e per la stessa ragione: una voce nuova nel registro si vede
 * subito, in italiano, invece di sparire.
 */
import { tSe } from "../lingue";

/** Cosa fa una parte: `skin.part.section-card`. */
export function descrizioneParte(nome: string, ripiego: string): string {
  return tSe(`skin.part.${nome}`, ripiego);
}

/** Cosa governa un token: `skin.token.color.surface.0`. */
export function descrizioneToken(id: string, ripiego: string): string {
  return tSe(`skin.token.${id}`, ripiego);
}

/** Cos'è un widget dello scafale: `skin.widget.navigation`. */
export function descrizioneWidget(nome: string, ripiego: string): string {
  return tSe(`skin.widget.${nome}`, ripiego);
}

/**
 * Cosa fa una manopola di widget: `skin.opt.transport.shuffle`.
 *
 * Il nome del widget entra nella chiave perché i nomi delle manopole non sono
 * unici da soli — `size` potrebbe comparire su due widget e voler dire due cose.
 */
export function descrizioneOpzione(
  widget: string,
  nome: string,
  ripiego: string,
): string {
  return tSe(`skin.opt.${widget}.${nome}`, ripiego);
}

/**
 * Cosa fa un parametro di effetto: `skin.par.vignette.color`.
 *
 * Stesso motivo: `color` esiste su sei effetti e ogni volta dice un'altra cosa
 * — «la tinta», «il colore delle linee», «il colore che si chiude sui bordi».
 */
export function descrizioneParametro(
  effetto: string,
  nome: string,
  ripiego: string,
): string {
  return tSe(`skin.par.${effetto}.${nome}`, ripiego);
}
