/**
 * I nomi della versione precedente del programma, tenuti solo per riconoscerli.
 *
 * Chi aveva già installato la versione di prima ha chat, skin e profili scritti
 * con i nomi di allora, e questi nomi non cambiano da soli. Qui stanno i soli
 * che la finestra deve ancora **leggere**: il tag dei blocchi di modifica delle
 * chat salvate, le estensioni dei file che l'utente può ancora avere sul disco,
 * la cartella dell'app della 1.0 che l'importatore legacy va a cercare.
 *
 * La regola è una: la finestra non scrive mai questi nomi. Li accetta in
 * ingresso (filtri di scelta file, regex dei blocchi) e al modello, al disco e
 * alla finestra di salvataggio offre solo quelli di adesso. È il gemello di
 * `core/rymembra-domain/src/eredita.rs`, che tiene lo stesso elenco per il
 * nucleo: se cambia uno, controlla l'altro.
 */

/**
 * Il linguaggio del blocco di codice con cui l'IA dello Studio proponeva le
 * modifiche (il tag che segue i tre apici). Le chat salvate e la storia
 * rimandata al modello lo contengono ancora: si legge, non si emette.
 */
export const LINGUAGGIO_PATCH_PRECEDENTE = "aether-patch";

/**
 * L'estensione, senza il punto, dei pacchetti di skin di prima. Va aggiunta ai
 * filtri di scelta file in importazione; il salvataggio usa solo la nuova.
 */
export const ESTENSIONE_SKIN_PRECEDENTE = "aeskin";

/**
 * L'estensione, senza il punto, dei profili esportati di prima. Va aggiunta ai
 * filtri di scelta file in importazione; il salvataggio usa solo la nuova.
 */
export const ESTENSIONE_PROFILO_PRECEDENTE = "aeprofile";

/**
 * Il nome della cartella, dentro `%APPDATA%`, dell'app Electron della 1.0: dove
 * l'importatore legacy propone di cercare la vecchia libreria.
 */
export const CARTELLA_DELLA_1_0 = "Aether";
